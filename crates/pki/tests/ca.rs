//! Tests de l'autorite de certification d'instance.

use pki::Ca;
use rcgen::{KeyPair, PublicKeyData};
use shared::ids::ServerId;
use x509_parser::prelude::*;

fn parse_pem(pem: &str) -> Vec<u8> {
    x509_parser::pem::parse_x509_pem(pem.as_bytes())
        .expect("PEM valide")
        .1
        .contents
}

#[test]
fn agent_cert_chains_to_ca() {
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let agent_key = KeyPair::generate().unwrap();
    let id = ServerId::new();
    let bundle = ca
        .issue_agent_cert(id, &agent_key.subject_public_key_info())
        .unwrap();
    assert!(bundle.key_pem.is_none(), "la cle de l'agent reste chez lui");

    let ca_der = parse_pem(ca.cert_pem());
    let agent_der = parse_pem(&bundle.cert_pem);
    let (_, ca_cert) = X509Certificate::from_der(&ca_der).unwrap();
    let (_, agent_cert) = X509Certificate::from_der(&agent_der).unwrap();

    assert_eq!(agent_cert.issuer(), ca_cert.subject());
    agent_cert
        .verify_signature(Some(ca_cert.public_key()))
        .expect("signature de l'agent verifiee par la CA");
    assert!(ca_cert.is_ca());
    assert!(!agent_cert.is_ca());
    // Le certificat porte bien la cle publique fournie, et l'identifiant.
    assert_eq!(
        agent_cert.public_key().raw,
        agent_key.subject_public_key_info().as_slice()
    );
    let cn = agent_cert
        .subject()
        .iter_common_name()
        .next()
        .unwrap()
        .as_str()
        .unwrap();
    assert_eq!(cn, id.to_string());
    assert_eq!(
        bundle.fingerprint_sha256,
        pki::fingerprint_sha256(&agent_der)
    );
}

#[test]
fn other_ca_does_not_verify_agent_cert() {
    let ca = Ca::generate("CA A").unwrap();
    let other = Ca::generate("CA B").unwrap();
    let agent_key = KeyPair::generate().unwrap();
    let bundle = ca
        .issue_agent_cert(ServerId::new(), &agent_key.subject_public_key_info())
        .unwrap();
    let agent_der = parse_pem(&bundle.cert_pem);
    let other_der = parse_pem(other.cert_pem());
    let (_, agent_cert) = X509Certificate::from_der(&agent_der).unwrap();
    let (_, other_cert) = X509Certificate::from_der(&other_der).unwrap();
    assert!(
        agent_cert
            .verify_signature(Some(other_cert.public_key()))
            .is_err()
    );
}

#[test]
fn server_cert_has_requested_sans_and_chains_to_ca() {
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let bundle = ca
        .issue_server_cert(&["203.0.113.7".to_string(), "localhost".to_string()])
        .unwrap();
    assert!(bundle.key_pem.as_deref().unwrap().contains("PRIVATE KEY"));
    let der = parse_pem(&bundle.cert_pem);
    let (_, cert) = X509Certificate::from_der(&der).unwrap();
    let ca_der = parse_pem(ca.cert_pem());
    let (_, ca_cert) = X509Certificate::from_der(&ca_der).unwrap();
    cert.verify_signature(Some(ca_cert.public_key())).unwrap();
    let sans = cert.subject_alternative_name().unwrap().unwrap();
    let names = &sans.value.general_names;
    assert!(
        names
            .iter()
            .any(|n| matches!(n, GeneralName::IPAddress(ip) if *ip == [203, 0, 113, 7]))
    );
    assert!(
        names
            .iter()
            .any(|n| matches!(n, GeneralName::DNSName("localhost")))
    );
    // Format de l'empreinte : 32 octets, majuscules, separes par ':'.
    let fp = &bundle.fingerprint_sha256;
    assert_eq!(fp.len(), 32 * 3 - 1);
    assert!(fp.split(':').all(|p| {
        p.len() == 2
            && p.chars()
                .all(|c| c.is_ascii_digit() || ('A'..='F').contains(&c))
    }));
    assert_eq!(*fp, pki::fingerprint_sha256(&der));
}

#[test]
fn reissuing_for_the_same_key_gives_distinct_serials() {
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let key = KeyPair::generate().unwrap();
    let spki = key.subject_public_key_info();
    let id = ServerId::new();
    let serial = |pem: &str| {
        let der = parse_pem(pem);
        let (_, c) = X509Certificate::from_der(&der).unwrap();
        c.raw_serial().to_vec()
    };
    let a = serial(&ca.issue_agent_cert(id, &spki).unwrap().cert_pem);
    let b = serial(&ca.issue_agent_cert(id, &spki).unwrap().cert_pem);
    assert_ne!(a, b);
    assert!(a.len() <= 16 && a[0] & 0x80 == 0, "{a:?}");
    let s1 = serial(
        &ca.issue_server_cert(&["localhost".into()])
            .unwrap()
            .cert_pem,
    );
    let s2 = serial(
        &ca.issue_server_cert(&["localhost".into()])
            .unwrap()
            .cert_pem,
    );
    assert_ne!(s1, s2);
}

#[test]
fn agent_key_is_generated_on_the_agent_side() {
    let key = pki::AgentKey::generate().unwrap();
    // La CA ne recoit que la cle publique.
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let id = ServerId::new();
    let bundle = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    let der = parse_pem(&bundle.cert_pem);
    let (_, cert) = X509Certificate::from_der(&der).unwrap();
    assert_eq!(cert.public_key().raw, key.public_key_der());
    // La cle privee PEM correspond a cette cle publique.
    let reloaded = KeyPair::from_pem(key.key_pem()).unwrap();
    assert_eq!(
        reloaded.subject_public_key_info().as_slice(),
        key.public_key_der()
    );
    assert!(!format!("{key:?}").contains("PRIVATE"));
}

#[test]
fn agent_id_is_read_back_from_the_san() {
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let key = pki::AgentKey::generate().unwrap();
    let id = ServerId::new();
    let bundle = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    let der = parse_pem(&bundle.cert_pem);
    assert_eq!(pki::agent_id_from_cert(&der), Some(id));
    // Un certificat serveur n'a pas de SAN d'agent.
    let server = ca.issue_server_cert(&["localhost".into()]).unwrap();
    assert_eq!(pki::agent_id_from_cert(&parse_pem(&server.cert_pem)), None);
    assert_eq!(pki::agent_id_from_cert(b"pas un certificat"), None);
}

#[test]
fn server_cert_ip_is_read_back_from_pem() {
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let bundle = ca
        .issue_server_cert(&["203.0.113.9".to_string(), "localhost".to_string()])
        .unwrap();
    assert_eq!(
        pki::ip_from_cert_pem(&bundle.cert_pem),
        Some("203.0.113.9".parse().unwrap())
    );
}

#[test]
fn cert_without_single_ip_san_gives_none() {
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let names = ca
        .issue_server_cert(&["panel.example.com".to_string()])
        .unwrap();
    assert_eq!(pki::ip_from_cert_pem(&names.cert_pem), None);
    let two = ca
        .issue_server_cert(&["203.0.113.9".to_string(), "2001:db8::9".to_string()])
        .unwrap();
    assert_eq!(pki::ip_from_cert_pem(&two.cert_pem), None);
    assert_eq!(pki::ip_from_cert_pem("pas un certificat"), None);
}

/// CA rechargee depuis ses fichiers PEM : les certificats renouveles
/// chainent toujours a la meme CA, l'agent garde sa cle et son identifiant.
#[test]
fn reloaded_ca_renews_agent_and_server_certs() {
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let loaded = Ca::load(ca.cert_pem(), &ca.key_pem()).unwrap();
    let agent_key = KeyPair::generate().unwrap();
    let id = ServerId::new();
    let old = ca
        .issue_agent_cert(id, &agent_key.subject_public_key_info())
        .unwrap();
    let old_der = parse_pem(&old.cert_pem);
    let new = loaded.renew_agent_cert(&old_der).unwrap();
    assert_ne!(new.fingerprint_sha256, old.fingerprint_sha256);
    let new_der = parse_pem(&new.cert_pem);
    assert_eq!(pki::agent_id_from_cert(&new_der), Some(id));
    assert_eq!(
        pki::public_key_der(&new_der).unwrap(),
        agent_key.subject_public_key_info()
    );
    let ca_der = parse_pem(ca.cert_pem());
    let (_, ca_cert) = X509Certificate::from_der(&ca_der).unwrap();
    let (_, renewed) = X509Certificate::from_der(&new_der).unwrap();
    renewed
        .verify_signature(Some(ca_cert.public_key()))
        .expect("signe par la meme CA");
    assert_eq!(renewed.issuer(), ca_cert.subject());

    let link = ca
        .issue_server_cert(&["203.0.113.7".into(), "localhost".into()])
        .unwrap();
    let again = loaded.renew_server_cert(&link.cert_pem).unwrap();
    assert!(again.key_pem.is_some(), "nouvelle cle");
    let srv_der = parse_pem(&again.cert_pem);
    let (_, srv) = X509Certificate::from_der(&srv_der).unwrap();
    srv.verify_signature(Some(ca_cert.public_key())).unwrap();
    assert_eq!(
        pki::ips_from_cert_pem(&again.cert_pem),
        vec!["203.0.113.7".parse::<std::net::IpAddr>().unwrap()]
    );
    let names: Vec<String> = srv
        .subject_alternative_name()
        .unwrap()
        .unwrap()
        .value
        .general_names
        .iter()
        .filter_map(|n| match n {
            GeneralName::DNSName(d) => Some((*d).to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["localhost".to_string()]);
}

/// Simulation du temps : un certificat d'agent (365 j) est a renouveler a
/// partir des deux tiers de sa validite, et bien sur une fois expire.
#[test]
fn renewal_is_due_after_two_thirds_of_the_validity() {
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let key = KeyPair::generate().unwrap();
    let cert = ca
        .issue_agent_cert(ServerId::new(), &key.subject_public_key_info())
        .unwrap();
    let der = parse_pem(&cert.cert_pem);
    let (from, to) = pki::validity_unix(&der).unwrap();
    let day = 86_400;
    // Debut un jour avant l'emission (decalage d'horloge tolere).
    assert_eq!(to - from, 366 * day);
    let now = from + day;
    assert_eq!(pki::renewal_due(&der, now), Some(false));
    assert_eq!(pki::renewal_due(&der, now + 200 * day), Some(false));
    assert_eq!(pki::renewal_due(&der, now + 244 * day), Some(true));
    assert_eq!(pki::renewal_due(&der, to + day), Some(true), "expire");
    assert_eq!(pki::renewal_due(b"pas un certificat", now), None);
}

/// Certificats feuilles (agent et serveur) : extension BasicConstraints
/// presente, CA:false, critique.
#[test]
fn leaf_has_ca_false() {
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let agent_key = KeyPair::generate().unwrap();
    let agent = ca
        .issue_agent_cert(ServerId::new(), &agent_key.subject_public_key_info())
        .unwrap();
    let server = ca.issue_server_cert(&["localhost".into()]).unwrap();
    for pem in [&agent.cert_pem, &server.cert_pem] {
        let der = parse_pem(pem);
        let (_, cert) = X509Certificate::from_der(&der).unwrap();
        let bc = cert
            .basic_constraints()
            .unwrap()
            .expect("BasicConstraints explicite");
        assert!(!bc.value.ca, "CA:false");
        assert!(bc.critical, "extension critique");
    }
}

/// `issued_by` : signature de la CA verifiee (dates ignorees : un
/// certificat expire reste reconnu), autre CA ou DER illisible refuses.
#[test]
fn issued_by_checks_the_ca_signature() {
    let ca = Ca::generate("CA A").unwrap();
    let other = Ca::generate("CA A").unwrap();
    let key = KeyPair::generate().unwrap();
    let id = ServerId::new();
    let now = 1_900_000_000;
    let expired = ca
        .issue_agent_cert_between(
            id,
            &key.subject_public_key_info(),
            now - 400 * 86_400,
            now - 35 * 86_400,
        )
        .unwrap();
    let der = parse_pem(&expired.cert_pem);
    let ca_der = parse_pem(ca.cert_pem());
    assert!(pki::issued_by(&ca_der, &der));
    // Meme nom de CA, autre cle : refuse.
    assert!(!pki::issued_by(&parse_pem(other.cert_pem()), &der));
    assert!(!pki::issued_by(b"pas une CA", &der));
    assert!(!pki::issued_by(&ca_der, b"pas un certificat"));
}

/// Certificat serveur re-signe (signataire root) : meme cle publique, memes
/// SAN, 730 jours, aucune cle generee ; signe par la CA.
#[test]
fn reissued_server_cert_keeps_the_key_and_the_names() {
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let mut link = ca
        .issue_server_cert(&["203.0.113.7".into(), "localhost".into()])
        .unwrap();
    let _ = link.key_pem.take();
    let old_der = parse_pem(&link.cert_pem);
    let again = ca.reissue_server_cert(&link.cert_pem).unwrap();
    assert!(again.key_pem.is_none(), "aucune nouvelle cle");
    let new_der = parse_pem(&again.cert_pem);
    assert_ne!(again.fingerprint_sha256, link.fingerprint_sha256);
    assert_eq!(
        pki::public_key_der(&new_der).unwrap(),
        pki::public_key_der(&old_der).unwrap(),
        "meme cle"
    );
    assert!(pki::issued_by(&parse_pem(ca.cert_pem()), &new_der));
    assert_eq!(
        pki::ips_from_cert_pem(&again.cert_pem),
        vec!["203.0.113.7".parse::<std::net::IpAddr>().unwrap()]
    );
    let (from, to) = pki::validity_unix(&new_der).unwrap();
    assert_eq!(to - from, 731 * 86_400);
    // Certificat d'agent (sans SAN DNS ni IP) : refuse.
    let key = KeyPair::generate().unwrap();
    let agent = ca
        .issue_agent_cert(ServerId::new(), &key.subject_public_key_info())
        .unwrap();
    assert!(ca.reissue_server_cert(&agent.cert_pem).is_err());
}

/// CA chargee avec une cle qui ne correspond pas a son certificat : refusee.
#[test]
fn load_refuses_a_key_that_does_not_match_the_certificate() {
    let ca = Ca::generate("Ciphera test CA").unwrap();
    let other = Ca::generate("Ciphera test CA").unwrap();
    assert!(Ca::load(ca.cert_pem(), &other.key_pem()).is_err());
    assert!(Ca::load(ca.cert_pem(), &ca.key_pem()).is_ok());
}

/// Jeu de CA (rotation) : tous les certificats d'un PEM sont lus, dans
/// l'ordre ; un certificat est reconnu s'il est emis par l'une d'elles.
#[test]
fn a_ca_bundle_lists_every_certificate() {
    let current = Ca::generate("Ciphera CA").unwrap();
    let next = Ca::generate("Ciphera CA 2").unwrap();
    let bundle = format!("{}{}", current.cert_pem(), next.cert_pem());
    let ders = pki::ders_from_pem(&bundle);
    assert_eq!(
        ders,
        vec![parse_pem(current.cert_pem()), parse_pem(next.cert_pem())]
    );
    assert!(pki::ders_from_pem("pas un PEM").is_empty());
    let key = KeyPair::generate().unwrap();
    let issued = next
        .issue_agent_cert(ServerId::new(), &key.subject_public_key_info())
        .unwrap();
    let der = parse_pem(&issued.cert_pem);
    assert!(pki::issued_by_any(&ders, &der));
    assert!(!pki::issued_by_any(&ders[..1], &der));
    assert!(!pki::issued_by_any(&[], &der));
}

/// Une CA racine (signee par elle-meme, CA:true) est reconnue ; un
/// certificat d'agent ou serveur ne l'est pas.
#[test]
fn self_signed_ca_is_recognised() {
    let ca = Ca::generate("Ciphera CA").unwrap();
    assert!(pki::is_root_ca(&parse_pem(ca.cert_pem())));
    let mut server = ca.issue_server_cert(&["localhost".into()]).unwrap();
    let _ = server.key_pem.take();
    assert!(!pki::is_root_ca(&parse_pem(&server.cert_pem)));
    assert!(!pki::is_root_ca(b"pas un certificat"));
    assert_eq!(pki::sha256(b"abc").len(), 32);
    assert_ne!(pki::sha256(b"abc"), pki::sha256(b"abd"));
}
