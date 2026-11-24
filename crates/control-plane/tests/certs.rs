//! Renouvellement des certificats de l'instance (agents::certs) : lien et
//! panneau renouveles par le plan de controle, reprise manuelle `ciphera
//! admin renew-certs` d'un certificat d'agent deja expire. PostgreSQL requis
//! (voir `common::database`).

mod common;

use control_plane::agents::certs;
use control_plane::agents::{servers, transport};
use pki::Ca;
use shared::ids::ServerId;

/// Dossier de configuration comme apres `ciphera init` : CA, lien, panneau.
fn config_dir(ca: &Ca) -> common::TempDir {
    let tmp = common::TempDir::new();
    for sub in ["ca", "tls", "agents"] {
        std::fs::create_dir_all(tmp.0.join(sub)).unwrap();
    }
    std::fs::write(tmp.0.join("ca/ca.crt"), ca.cert_pem()).unwrap();
    std::fs::write(tmp.0.join("ca/ca.key"), ca.key_pem().as_bytes()).unwrap();
    for (crt, key) in certs::OWN_CERTS {
        let mut b = ca
            .issue_server_cert(&["203.0.113.7".into(), "localhost".into()])
            .unwrap();
        std::fs::write(tmp.0.join(crt), &b.cert_pem).unwrap();
        std::fs::write(tmp.0.join(key), b.key_pem.take().unwrap().as_bytes()).unwrap();
    }
    tmp
}

#[test]
fn fresh_server_certs_are_kept_and_forced_ones_reload() {
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    let loaded = certs::load_ca(&dir.0).unwrap();
    let before = std::fs::read_to_string(dir.0.join(transport::LINK_CRT)).unwrap();
    assert!(certs::renew_own(&dir.0, &loaded, certs::now_unix()).is_empty());
    assert_eq!(
        std::fs::read_to_string(dir.0.join(transport::LINK_CRT)).unwrap(),
        before
    );
    // Deux ans plus tard (simulation) : renouveles.
    let later = certs::now_unix() + 700 * 86_400;
    assert_eq!(
        certs::renew_own(&dir.0, &loaded, later),
        vec![transport::LINK_CRT, certs::PANEL_CRT]
    );
    let after = std::fs::read_to_string(dir.0.join(transport::LINK_CRT)).unwrap();
    assert_ne!(after, before);
    assert_eq!(
        pki::ips_from_cert_pem(&after),
        vec!["203.0.113.7".parse::<std::net::IpAddr>().unwrap()]
    );
    // Certificat et cle coherents : la configuration du lien se charge.
    let tls = transport::LinkTls::load(&dir.0).unwrap();
    transport::server_config(&tls).unwrap();
}

/// `ciphera admin renew-certs` : certificat de l'agent local deja expire
/// (machine eteinte plus d'un an). Nouveau certificat pour la meme cle,
/// accepte en plus de l'ancien jusqu'a la premiere connexion qui le
/// presente, ou il le remplace.
#[tokio::test]
async fn admin_renews_an_expired_local_agent_cert() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    let agent_dir = common::TempDir::new();
    let key = pki::AgentKey::generate().unwrap();
    let id = ServerId::new();
    let now = certs::now_unix();
    let day = 86_400;
    let expired = ca
        .issue_agent_cert_between(id, key.public_key_der(), now - 400 * day, now - 35 * day)
        .unwrap();
    std::fs::write(agent_dir.0.join("agent.crt"), &expired.cert_pem).unwrap();
    std::fs::write(dir.0.join(servers::LOCAL_AGENT_CERT), &expired.cert_pem).unwrap();
    servers::register_local(&db, &expired.cert_pem)
        .await
        .unwrap();
    let link_before = std::fs::read_to_string(dir.0.join(transport::LINK_CRT)).unwrap();

    let done = certs::renew_all(&db, &dir.0, &agent_dir.0).await.unwrap();
    assert_eq!(
        done.server_certs,
        vec![transport::LINK_CRT, certs::PANEL_CRT]
    );
    assert_eq!(done.agent.as_ref().map(|a| a.0), Some(id));
    assert_ne!(
        std::fs::read_to_string(dir.0.join(transport::LINK_CRT)).unwrap(),
        link_before,
        "renouvele meme s'il n'etait pas du"
    );
    let pem = std::fs::read_to_string(agent_dir.0.join("agent.crt")).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.0.join(servers::LOCAL_AGENT_CERT)).unwrap(),
        pem,
        "copie locale a jour"
    );
    let der = pki::der_from_pem(&pem).unwrap();
    assert_eq!(pki::renewal_due(&der, now), Some(false));
    assert_eq!(pki::public_key_der(&der).unwrap(), key.public_key_der());
    let new_fp = pki::fingerprint_sha256(&der);
    assert!(servers::is_authorized(&db, id, &new_fp).await.unwrap());
    assert!(
        servers::is_authorized(&db, id, &expired.fingerprint_sha256)
            .await
            .unwrap()
    );
    assert!(servers::promote_next_cert(&db, id, &new_fp).await.unwrap());
    assert!(
        !servers::is_authorized(&db, id, &expired.fingerprint_sha256)
            .await
            .unwrap(),
        "ancien certificat retire"
    );

    // Serveur revoque : rien n'est renouvele pour lui.
    sqlx::query("UPDATE servers SET revoked_at = now() WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    let err = certs::renew_all(&db, &dir.0, &agent_dir.0)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("revoque"), "{err}");
    assert_eq!(
        std::fs::read_to_string(agent_dir.0.join("agent.crt")).unwrap(),
        pem
    );
}
