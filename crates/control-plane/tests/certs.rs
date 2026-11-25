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

/// `ciphera admin renew-certs` ne signe que le certificat du serveur local
/// enregistre (empreinte en base) et emis par la CA de l'instance : un
/// certificat d'une autre CA, ou de la bonne CA mais avec une autre cle,
/// pose dans le dossier de l'agent est refuse sans rien ecrire ni noter.
#[tokio::test]
async fn renew_certs_rejects_foreign_fingerprint() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    let agent_dir = common::TempDir::new();
    let id = ServerId::new();
    let key = pki::AgentKey::generate().unwrap();
    let registered = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    servers::register_local(&db, &registered.cert_pem)
        .await
        .unwrap();

    let other_ca = Ca::generate("instance").unwrap();
    let other_key = pki::AgentKey::generate().unwrap();
    let foreign_ca = other_ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    let foreign_key = ca.issue_agent_cert(id, other_key.public_key_der()).unwrap();
    for foreign in [&foreign_ca, &foreign_key] {
        std::fs::write(agent_dir.0.join("agent.crt"), &foreign.cert_pem).unwrap();
        let err = certs::renew_all(&db, &dir.0, &agent_dir.0)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("agent"), "{err}");
        assert_eq!(
            std::fs::read_to_string(agent_dir.0.join("agent.crt")).unwrap(),
            foreign.cert_pem,
            "rien n'est ecrit"
        );
        let next: Option<String> =
            sqlx::query_scalar("SELECT next_cert_fingerprint FROM servers WHERE id = $1")
                .bind(id)
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(next, None, "rien n'est note");
    }

    // Le certificat enregistre, lui, est renouvele.
    std::fs::write(agent_dir.0.join("agent.crt"), &registered.cert_pem).unwrap();
    let done = certs::renew_all(&db, &dir.0, &agent_dir.0).await.unwrap();
    assert_eq!(done.agent.map(|a| a.0), Some(id));
    // Relance avant la reconnexion de l'agent : le certificat suivant
    // (note en base) est reconnu aussi.
    certs::renew_all(&db, &dir.0, &agent_dir.0).await.unwrap();

    // Empreinte enregistree mais certificat d'une autre CA : refuse aussi.
    sqlx::query("DELETE FROM servers WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    servers::register_local(&db, &foreign_ca.cert_pem)
        .await
        .unwrap();
    std::fs::write(agent_dir.0.join("agent.crt"), &foreign_ca.cert_pem).unwrap();
    let err = certs::renew_all(&db, &dir.0, &agent_dir.0)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("CA de l'instance"), "{err}");
    assert_eq!(
        std::fs::read_to_string(agent_dir.0.join("agent.crt")).unwrap(),
        foreign_ca.cert_pem
    );
}

/// Paire certificat + cle ecrite avec un manifeste : une coupure apres le
/// manifeste est terminee a la reprise, une coupure avant laisse
/// l'ancienne paire intacte. Aucun fichier temporaire ne reste.
#[test]
fn key_pair_write_is_completed_or_rolled_back() {
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    let (crt, key) = (transport::LINK_CRT, transport::LINK_KEY);
    let old_crt = std::fs::read_to_string(dir.0.join(crt)).unwrap();
    let old_key = std::fs::read_to_string(dir.0.join(key)).unwrap();
    let mut new = ca.issue_server_cert(&["localhost".into()]).unwrap();
    let new_key = new.key_pem.take().unwrap();
    let staged = |name: &str| {
        let p = std::path::Path::new(name);
        dir.0.join(p.parent().unwrap()).join(format!(
            ".{}.renew",
            p.file_name().unwrap().to_string_lossy()
        ))
    };
    let manifest = certs::pair_manifest(&dir.0, crt);

    // Coupure avant le manifeste : fichiers prepares abandonnes.
    std::fs::write(staged(crt), &new.cert_pem).unwrap();
    std::fs::write(staged(key), new_key.as_bytes()).unwrap();
    certs::recover_pairs(&dir.0).unwrap();
    assert_eq!(std::fs::read_to_string(dir.0.join(crt)).unwrap(), old_crt);
    assert_eq!(std::fs::read_to_string(dir.0.join(key)).unwrap(), old_key);
    assert!(!staged(crt).exists() && !staged(key).exists());

    // Coupure apres le manifeste et le certificat deja en place : la cle
    // suit a la reprise.
    std::fs::write(staged(key), new_key.as_bytes()).unwrap();
    std::fs::write(&manifest, "").unwrap();
    std::fs::write(dir.0.join(crt), &new.cert_pem).unwrap();
    certs::recover_pairs(&dir.0).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.0.join(crt)).unwrap(),
        new.cert_pem
    );
    assert_eq!(
        std::fs::read_to_string(dir.0.join(key)).unwrap(),
        new_key.as_str()
    );
    assert!(!manifest.exists() && !staged(key).exists());
    transport::server_config(&transport::LinkTls::load(&dir.0).unwrap()).unwrap();

    // Ecriture complete : paire coherente, ni manifeste ni temporaire.
    let loaded = certs::load_ca(&dir.0).unwrap();
    assert!(certs::renew_server(&dir.0, &loaded, (crt, key), certs::now_unix(), true).unwrap());
    assert!(!manifest.exists() && !staged(crt).exists() && !staged(key).exists());
    transport::server_config(&transport::LinkTls::load(&dir.0).unwrap()).unwrap();
}

/// Base restauree depuis une sauvegarde anterieure a un renouvellement :
/// elle ne connait que l'ancienne empreinte, l'agent presente deja le
/// certificat renouvele. `renew-certs` le re-signe quand meme (emis par la
/// CA, identifiant du serveur local, meme cle que `agents/local.crt`) et
/// l'agent se reconnecte.
#[tokio::test]
async fn renew_certs_after_database_restore() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    let agent_dir = common::TempDir::new();
    let id = ServerId::new();
    let key = pki::AgentKey::generate().unwrap();
    let before = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    std::fs::write(dir.0.join(servers::LOCAL_AGENT_CERT), &before.cert_pem).unwrap();
    servers::register_local(&db, &before.cert_pem)
        .await
        .unwrap();
    // Renouvele en ligne puis base restauree : seule `before` est connue.
    let renewed = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    std::fs::write(agent_dir.0.join("agent.crt"), &renewed.cert_pem).unwrap();
    assert!(
        !servers::is_authorized(&db, id, &renewed.fingerprint_sha256)
            .await
            .unwrap()
    );

    let done = certs::renew_all(&db, &dir.0, &agent_dir.0).await.unwrap();
    assert_eq!(done.agent.map(|a| a.0), Some(id));
    let pem = std::fs::read_to_string(agent_dir.0.join("agent.crt")).unwrap();
    assert_ne!(pem, renewed.cert_pem);
    let der = pki::der_from_pem(&pem).unwrap();
    assert_eq!(pki::public_key_der(&der).unwrap(), key.public_key_der());
    let new_fp = pki::fingerprint_sha256(&der);
    assert!(servers::is_authorized(&db, id, &new_fp).await.unwrap());
    assert!(servers::promote_next_cert(&db, id, &new_fp).await.unwrap());
}

/// Repli « base restauree » : refuse pour un certificat d'une autre CA,
/// d'une autre cle, d'un autre identifiant, ou d'un serveur revoque ; rien
/// n'est ecrit ni note.
#[tokio::test]
async fn renew_certs_restore_fallback_refusals() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    let agent_dir = common::TempDir::new();
    let id = ServerId::new();
    let key = pki::AgentKey::generate().unwrap();
    let registered = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    std::fs::write(dir.0.join(servers::LOCAL_AGENT_CERT), &registered.cert_pem).unwrap();
    servers::register_local(&db, &registered.cert_pem)
        .await
        .unwrap();

    let other_ca = Ca::generate("instance").unwrap();
    let other_key = pki::AgentKey::generate().unwrap();
    let cases = [
        (
            other_ca.issue_agent_cert(id, key.public_key_der()).unwrap(),
            "CA de l'instance",
        ),
        (
            ca.issue_agent_cert(id, other_key.public_key_der()).unwrap(),
            "cle differente",
        ),
        (
            ca.issue_agent_cert(ServerId::new(), key.public_key_der())
                .unwrap(),
            "serveur local",
        ),
    ];
    let refused = async |pem: &str, expected: &str| {
        std::fs::write(agent_dir.0.join("agent.crt"), pem).unwrap();
        let err = certs::renew_all(&db, &dir.0, &agent_dir.0)
            .await
            .unwrap_err();
        assert!(err.to_string().contains(expected), "{err}");
        assert_eq!(
            std::fs::read_to_string(agent_dir.0.join("agent.crt")).unwrap(),
            pem
        );
        let next: Option<String> =
            sqlx::query_scalar("SELECT next_cert_fingerprint FROM servers WHERE id = $1")
                .bind(id)
                .fetch_one(&db)
                .await
                .unwrap();
        assert_eq!(next, None);
    };
    for (cert, expected) in &cases {
        refused(&cert.cert_pem, expected).await;
    }

    // Serveur revoque : meme le bon certificat (meme cle) est refuse.
    sqlx::query("UPDATE servers SET revoked_at = now() WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    let renewed = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    refused(&renewed.cert_pem, "revoque").await;
}

/// Verrou des paires : un renouvellement attend que l'autre (service ou
/// `renew-certs`) ait fini.
#[test]
fn pair_writes_are_serialized() {
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    let lock = certs::lock_pairs(&dir.0, transport::LINK_CRT).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let path = dir.0.clone();
    let writer = std::thread::spawn(move || {
        let loaded = certs::load_ca(&path).unwrap();
        let renewed = certs::renew_server(
            &path,
            &loaded,
            (transport::LINK_CRT, transport::LINK_KEY),
            certs::now_unix(),
            true,
        );
        tx.send(renewed.unwrap()).unwrap();
    });
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(500))
            .is_err(),
        "renouvellement fait malgre le verrou"
    );
    drop(lock);
    assert!(rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap());
    writer.join().unwrap();
    transport::server_config(&transport::LinkTls::load(&dir.0).unwrap()).unwrap();
}
