//! Rotation de la CA de l'instance (`ciphera admin rotate-ca`,
//! agents::rotation) : CA suivante ecrite a cote de la courante, agents
//! signes par elle, lien et panneau par la courante jusqu'a la fin ; fin
//! refusee tant qu'un serveur non revoque presente un certificat d'une
//! autre CA. PostgreSQL requis (voir `common::database`).

mod common;

use std::path::Path;

use control_plane::agents::certs::{self, Renewals};
use control_plane::agents::rotation::{self, Start};
use control_plane::agents::servers;
use control_plane::agents::signer::{self, CertRequest, Usage};
use control_plane::agents::transport::{self, TrustedCas};
use pki::Ca;
use shared::ids::ServerId;

/// Dossier de configuration apres `ciphera init`, cle de la CA comprise
/// (root : commande lancee en root).
fn config_dir(ca: &Ca) -> common::TempDir {
    let tmp = common::TempDir::new();
    for sub in ["ca", "tls", "agents"] {
        std::fs::create_dir_all(tmp.0.join(sub)).unwrap();
    }
    std::fs::write(tmp.0.join(transport::CA_CRT), ca.cert_pem()).unwrap();
    let key = tmp.0.join(certs::CA_KEY);
    std::fs::write(&key, ca.key_pem().as_bytes()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let crt = tmp.0.join(transport::CA_CRT);
        std::fs::set_permissions(&crt, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    for (crt, key) in certs::OWN_CERTS {
        let mut b = ca
            .issue_server_cert(&["203.0.113.7".into(), "localhost".into()])
            .unwrap();
        std::fs::write(tmp.0.join(crt), &b.cert_pem).unwrap();
        std::fs::write(tmp.0.join(key), b.key_pem.take().unwrap().as_bytes()).unwrap();
    }
    tmp
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

fn der(pem: &str) -> Vec<u8> {
    pki::der_from_pem(pem).unwrap()
}

/// Agent enregistre (certificat emis par `ca`) ; renvoie son identifiant,
/// sa cle et son certificat DER.
async fn agent(db: &sqlx::PgPool, ca: &Ca) -> (ServerId, pki::AgentKey, Vec<u8>) {
    let id = ServerId::new();
    let key = pki::AgentKey::generate().unwrap();
    let cert = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    sqlx::query(
        "INSERT INTO servers (id, name, kind, cert_fingerprint) VALUES ($1, $2, 'remote', $3)",
    )
    .bind(id)
    .bind(format!("srv-{id}"))
    .bind(pki::fingerprint_sha256(&der(&cert.cert_pem)))
    .execute(db)
    .await
    .unwrap();
    (id, key, der(&cert.cert_pem))
}

/// `rotate-ca` : CA suivante ecrite (cle 0600, certificat 0640), CA
/// courante intacte ; relancee, rien ne change. Le service voit les deux
/// CA, la suivante signe les agents.
#[test]
fn rotate_ca_writes_the_next_ca() {
    let ca = Ca::generate("Ciphera CA").unwrap();
    let dir = config_dir(&ca);
    let before = read(&dir.0, transport::CA_CRT);
    let trust_before = TrustedCas::load(&dir.0).unwrap();
    assert_eq!(trust_before.next(), None);

    let Start::Started(fp) = rotation::start(&dir.0).unwrap() else {
        panic!("rotation commencee attendue");
    };
    let next_pem = read(&dir.0, transport::CA_NEXT_CRT);
    assert_eq!(pki::fingerprint_sha256(&der(&next_pem)), fp);
    assert_ne!(next_pem, before);
    assert_eq!(
        read(&dir.0, transport::CA_CRT),
        before,
        "CA courante intacte"
    );
    assert!(pki::is_root_ca(&der(&next_pem)));
    let key = read(&dir.0, signer::CA_NEXT_KEY);
    Ca::load(&next_pem, &key).expect("cle de la CA suivante");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |rel: &str| {
            std::fs::metadata(dir.0.join(rel))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777
        };
        assert_eq!(mode(signer::CA_NEXT_KEY), 0o600);
        assert_eq!(mode(transport::CA_NEXT_CRT), 0o640);
        assert_eq!(mode(signer::CA_NEXT_OWN), 0o600);
    }

    // Releve du lien et du panneau (cle publique et SAN).
    let own = signer::read_next_own(&dir.0).unwrap().expect("releve");
    for (crt, _) in certs::OWN_CERTS {
        let id = pki::server_identity(&read(&dir.0, crt)).unwrap();
        assert!(own.contains(&format!("{crt} {id}")), "{own}");
    }
    assert_eq!(rotation::start(&dir.0).unwrap(), Start::InProgress(fp));
    assert_eq!(read(&dir.0, transport::CA_NEXT_CRT), next_pem, "inchangee");

    let trust = TrustedCas::load(&dir.0).unwrap();
    assert_eq!(trust.next(), Some(der(&next_pem).as_slice()));
    assert_eq!(trust.active(), der(&next_pem).as_slice());
    assert_eq!(trust.ders().len(), 2);
    assert_ne!(trust.fingerprint(), trust_before.fingerprint());
    // Lien : les deux CA de confiance.
    let tls = transport::LinkTls::load(&dir.0).unwrap();
    assert_eq!(pki::ders_from_pem(&tls.ca_cert_pem).len(), 2);
    transport::server_config(&tls).unwrap();
    let cas = signer::load_cas(&dir.0).unwrap();
    assert!(cas.next.is_some());
}

/// Pendant la rotation : le signataire signe le certificat d'un agent avec
/// la CA suivante (meme cle), celui du lien avec la courante ; le service
/// redemande d'emblee le certificat d'un agent de l'ancienne CA, pas celui
/// du lien.
#[tokio::test]
async fn signer_moves_agents_to_the_next_ca() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("Ciphera CA").unwrap();
    let dir = config_dir(&ca);
    let data = common::TempDir::new();
    rotation::start(&dir.0).unwrap();
    let next_der = der(&read(&dir.0, transport::CA_NEXT_CRT));
    let (id, key, leaf) = agent(&db, &ca).await;

    let renewals = Renewals::new(&dir.0, &data.0);
    let now = certs::now_unix();
    assert!(
        renewals.request_agent(id, &leaf, now).unwrap(),
        "certificat neuf, mais de l'ancienne CA : redemande"
    );
    assert!(
        renewals.request_own(now).is_empty(),
        "lien et panneau gardes"
    );
    let link = read(&dir.0, transport::LINK_CRT);
    let request = CertRequest::own(Usage::Link, &link).unwrap();
    signer::write_request(&data.0, &request).unwrap();

    let report = signer::sign_requests(&db, &dir.0, &data.0).await.unwrap();
    assert_eq!(report.refused, vec![]);
    assert_eq!(report.signed.len(), 2);
    let (_, agent_pem) = signer::read_response(&data.0, &id.to_string())
        .unwrap()
        .unwrap();
    let agent_der = der(&agent_pem);
    assert!(pki::issued_by(&next_der, &agent_der), "agent : CA suivante");
    assert_eq!(
        pki::public_key_der(&agent_der).unwrap(),
        key.public_key_der()
    );
    let (_, link_pem) = signer::read_response(&data.0, "link").unwrap().unwrap();
    assert!(
        pki::issued_by(&der(ca.cert_pem()), &der(&link_pem)),
        "lien : CA courante"
    );
    // Le service accepte la reponse (CA de confiance) et l'enverra.
    let sent = renewals.agent_responses(now, &[id]);
    assert_eq!(sent.len(), 1);
    // Certificat de la CA suivante, pas a renouveler : plus de demande.
    assert!(!renewals.request_agent(id, &agent_der, now).unwrap());
}

/// `rotate-ca --finish` refuse tant qu'un serveur non revoque presente un
/// certificat d'une autre CA que la suivante (rien n'est change) ; un
/// serveur revoque ne compte pas. Ensuite : la suivante devient la CA de
/// l'instance (certificat et cle), le lien et le panneau sont re-signes par
/// elle (meme cle), les fichiers de la suivante disparaissent.
#[tokio::test]
async fn finish_refused_while_old_certs_remain() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("Ciphera CA").unwrap();
    let dir = config_dir(&ca);
    assert!(
        rotation::finish(&db, &dir.0).await.is_err(),
        "pas de rotation"
    );
    rotation::start(&dir.0).unwrap();
    let next_pem = read(&dir.0, transport::CA_NEXT_CRT);
    let next_der = der(&next_pem);
    let before = read(&dir.0, transport::CA_CRT);
    let link_before = read(&dir.0, transport::LINK_CRT);
    let (id, _, leaf) = agent(&db, &ca).await;
    let (revoked, _, _) = agent(&db, &ca).await;
    sqlx::query("UPDATE servers SET revoked_at = now() WHERE id = $1")
        .bind(revoked)
        .execute(&db)
        .await
        .unwrap();
    // Ancienne CA notee a la connexion.
    servers::set_issuer(
        &db,
        id,
        &pki::fingerprint_sha256(&leaf),
        &pki::sha256(&der(ca.cert_pem())),
    )
    .await
    .unwrap();

    let status = rotation::status(&db, &dir.0).await.unwrap();
    assert_eq!(status.next, Some(pki::fingerprint_sha256(&next_der)));
    assert_eq!(
        status.remaining.iter().map(|p| p.id).collect::<Vec<_>>(),
        vec![id]
    );
    let err = rotation::finish(&db, &dir.0).await.unwrap_err().to_string();
    assert!(err.contains(&id.to_string()), "{err}");
    assert!(!err.contains(&revoked.to_string()), "{err}");
    assert_eq!(read(&dir.0, transport::CA_CRT), before, "rien change");
    assert_eq!(read(&dir.0, transport::CA_NEXT_CRT), next_pem);
    assert_eq!(read(&dir.0, transport::LINK_CRT), link_before);

    // L'agent presente un certificat de la suivante.
    let next = Ca::load(&next_pem, &read(&dir.0, signer::CA_NEXT_KEY)).unwrap();
    let key = pki::public_key_der(&leaf).unwrap();
    let renewed = next.issue_agent_cert(id, &key).unwrap();
    let renewed_fp = pki::fingerprint_sha256(&der(&renewed.cert_pem));
    sqlx::query("UPDATE servers SET cert_fingerprint = $2 WHERE id = $1")
        .bind(id)
        .bind(&renewed_fp)
        .execute(&db)
        .await
        .unwrap();
    servers::set_issuer(&db, id, &renewed_fp, &pki::sha256(&next_der))
        .await
        .unwrap();
    assert!(
        rotation::status(&db, &dir.0)
            .await
            .unwrap()
            .remaining
            .is_empty()
    );

    let done = rotation::finish(&db, &dir.0).await.unwrap();
    assert_eq!(done.fingerprint, pki::fingerprint_sha256(&next_der));
    assert_eq!(read(&dir.0, transport::CA_CRT), next_pem);
    let loaded = certs::load_ca(&dir.0).expect("certificat et cle de la nouvelle CA");
    assert_eq!(loaded.cert_pem(), next_pem);
    assert!(!dir.0.join(transport::CA_NEXT_CRT).exists());
    assert!(!dir.0.join(signer::CA_NEXT_KEY).exists());
    for (crt, _) in certs::OWN_CERTS {
        let now = der(&read(&dir.0, crt));
        assert!(
            pki::issued_by(&next_der, &now),
            "{crt} re-signe par la nouvelle CA"
        );
    }
    assert_eq!(
        pki::public_key_der(&der(&read(&dir.0, transport::LINK_CRT))),
        pki::public_key_der(&der(&link_before)),
        "meme cle du lien"
    );
    let trust = TrustedCas::load(&dir.0).unwrap();
    assert_eq!(trust.ders(), vec![next_der]);
    transport::server_config(&transport::LinkTls::load(&dir.0).unwrap()).unwrap();
    let st = rotation::status(&db, &dir.0).await.unwrap();
    assert_eq!(st.next, None);
    assert!(st.remaining.is_empty());
}

/// Fin du remplacement de `ca.crt` + `ca.key` simulee jusqu'au manifeste
/// (point de non-retour) : fichiers prepares et manifeste, comme
/// `certs::replace_pair` les laisse s'il est coupe juste apres.
fn interrupt_after_manifest(dir: &Path, next_pem: &str, next_key: &str) {
    let ca = dir.join("ca");
    std::fs::write(ca.join(".ca.crt.renew"), next_pem).unwrap();
    write_secret(&ca.join(".ca.key.renew"), next_key.as_bytes());
    std::fs::write(
        certs::pair_manifest(dir, transport::CA_CRT),
        format!("{}\n{}\n", transport::CA_CRT, certs::CA_KEY),
    )
    .unwrap();
}

/// Fichier secret de root (0600), comme les ecrit le signataire.
fn write_secret(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

/// Rotation commencee, agent unique deja passe a la CA suivante (les
/// serveurs d'un scenario precedent de la meme base sont revoques).
async fn ready_to_finish(db: &sqlx::PgPool, ca: &Ca, dir: &Path) -> (String, String) {
    sqlx::query("UPDATE servers SET revoked_at = now() WHERE revoked_at IS NULL")
        .execute(db)
        .await
        .unwrap();
    rotation::start(dir).unwrap();
    let next_pem = read(dir, transport::CA_NEXT_CRT);
    let next_key = read(dir, signer::CA_NEXT_KEY);
    let next = Ca::load(&next_pem, &next_key).unwrap();
    let (id, key, _) = agent(db, ca).await;
    let renewed = next.issue_agent_cert(id, key.public_key_der()).unwrap();
    let fp = pki::fingerprint_sha256(&der(&renewed.cert_pem));
    sqlx::query("UPDATE servers SET cert_fingerprint = $2 WHERE id = $1")
        .bind(id)
        .bind(&fp)
        .execute(db)
        .await
        .unwrap();
    servers::set_issuer(db, id, &fp, &pki::sha256(&der(&next_pem)))
        .await
        .unwrap();
    (next_pem, next_key)
}

/// Apres reprise : la nouvelle CA est la courante, plus aucun reste de la
/// suivante, le signataire lit la CA et `--status` ne voit plus de rotation.
async fn assert_settled(db: &sqlx::PgPool, dir: &Path, next_pem: &str) {
    assert_eq!(read(dir, transport::CA_CRT), next_pem);
    for rel in [
        transport::CA_NEXT_CRT,
        signer::CA_NEXT_KEY,
        signer::CA_NEXT_OWN,
    ] {
        assert!(!dir.join(rel).exists(), "{rel} retire");
    }
    assert!(!certs::pair_manifest(dir, transport::CA_CRT).exists());
    let cas = signer::load_cas(dir).expect("signataire non bloque");
    assert!(cas.next.is_none());
    assert_eq!(cas.current.cert_pem(), next_pem);
    certs::load_ca(dir).expect("paire de la nouvelle CA");
    // Paire reprise remise aux droits attendus.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |rel: &str| {
            std::fs::metadata(dir.join(rel))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777
        };
        assert_eq!(mode(certs::CA_KEY), 0o600);
        assert_eq!(mode(transport::CA_CRT), 0o640);
    }
    assert_eq!(rotation::status(db, dir).await.unwrap().next, None);
    assert_eq!(TrustedCas::load(dir).unwrap().ders(), vec![der(next_pem)]);
}

/// `--finish` interrompu, puis relance : il termine la fin, quel que soit
/// le point de la coupure (manifeste de la paire de la CA present ;
/// `ca-next.crt` seul ; `ca-next.key` seule). Le signataire n'est jamais
/// bloque. Une cle seule d'un debut interrompu est retiree sans rien
/// terminer.
#[tokio::test]
async fn finish_resumes_after_interruption() {
    let (db, _guard) = common::database().await;

    // 1. Coupure apres le manifeste de la paire de la CA (lien et panneau
    //    deja re-signes, `ca-next.*` encore la).
    let ca = Ca::generate("Ciphera CA").unwrap();
    let dir = config_dir(&ca);
    let (next_pem, next_key) = ready_to_finish(&db, &ca, &dir.0).await;
    let next = Ca::load(&next_pem, &next_key).unwrap();
    let cas = signer::load_cas(&dir.0).unwrap();
    for pair in certs::OWN_CERTS {
        certs::renew_server_with(&dir.0, &next, &cas.trusted(), pair, certs::now_unix(), true)
            .unwrap();
    }
    interrupt_after_manifest(&dir.0, &next_pem, &next_key);
    let done = rotation::finish(&db, &dir.0).await.expect("reprise");
    assert!(done.resumed);
    assert_eq!(done.fingerprint, pki::fingerprint_sha256(&der(&next_pem)));
    assert_settled(&db, &dir.0, &next_pem).await;
    for (crt, _) in certs::OWN_CERTS {
        assert!(pki::issued_by(&der(&next_pem), &der(&read(&dir.0, crt))));
    }
    assert!(
        rotation::finish(&db, &dir.0).await.is_err(),
        "plus rien a faire"
    );

    // 2. Coupure apres l'installation de la nouvelle CA, `ca-next.crt`
    //    encore la (seul) ; le signataire passe avant l'operateur et
    //    termine la fin : `--finish` relance le constate (sans erreur).
    let ca = Ca::generate("Ciphera CA").unwrap();
    let dir = config_dir(&ca);
    let (next_pem, _) = ready_to_finish(&db, &ca, &dir.0).await;
    rotation::finish(&db, &dir.0).await.unwrap();
    std::fs::write(dir.0.join(transport::CA_NEXT_CRT), &next_pem).unwrap();
    assert!(
        signer::load_cas(&dir.0).unwrap().next.is_none(),
        "signataire"
    );
    assert!(!dir.0.join(transport::CA_NEXT_CRT).exists());
    let done = rotation::finish(&db, &dir.0)
        .await
        .expect("fin deja terminee par le signataire");
    assert!(done.resumed);
    assert_settled(&db, &dir.0, &next_pem).await;
    assert!(
        rotation::finish(&db, &dir.0).await.is_err(),
        "constat consomme"
    );

    // 3. `ca-next.key` seule (celle de la nouvelle CA courante).
    let ca = Ca::generate("Ciphera CA").unwrap();
    let dir = config_dir(&ca);
    let (next_pem, next_key) = ready_to_finish(&db, &ca, &dir.0).await;
    rotation::finish(&db, &dir.0).await.unwrap();
    write_secret(&dir.0.join(signer::CA_NEXT_KEY), next_key.as_bytes());
    let done = rotation::finish(&db, &dir.0).await.expect("reprise");
    assert!(done.resumed);
    assert_settled(&db, &dir.0, &next_pem).await;

    // Cle seule d'un debut interrompu (autre cle) : retiree, aucune
    // rotation terminee.
    let orphan = Ca::generate("orpheline").unwrap();
    write_secret(
        &dir.0.join(signer::CA_NEXT_KEY),
        orphan.key_pem().as_bytes(),
    );
    assert!(rotation::finish(&db, &dir.0).await.is_err());
    assert!(!dir.0.join(signer::CA_NEXT_KEY).exists());
}

/// `--finish` refuse de re-signer un lien dont la cle (ou les SAN) a change
/// depuis le debut de la rotation : rien n'est change.
#[tokio::test]
async fn finish_refuses_a_changed_link() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("Ciphera CA").unwrap();
    let dir = config_dir(&ca);
    let (next_pem, _) = ready_to_finish(&db, &ca, &dir.0).await;
    let before = read(&dir.0, transport::CA_CRT);
    let mut other = ca
        .issue_server_cert(&["203.0.113.7".into(), "evil.example".into()])
        .unwrap();
    let _ = other.key_pem.take();
    std::fs::write(dir.0.join(transport::LINK_CRT), &other.cert_pem).unwrap();
    let e = rotation::finish(&db, &dir.0).await.unwrap_err().to_string();
    assert!(e.contains("ont change"), "{e}");
    assert_eq!(read(&dir.0, transport::CA_CRT), before);
    assert_eq!(read(&dir.0, transport::CA_NEXT_CRT), next_pem);
    assert_eq!(read(&dir.0, transport::LINK_CRT), other.cert_pem);
}
