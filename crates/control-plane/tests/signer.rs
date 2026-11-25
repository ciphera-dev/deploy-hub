//! Signataire de certificats root (`ciphera admin sign-requests`,
//! agents::signer) et demandes du plan de controle (agents::certs::Renewals) :
//! le service ecrit une demande, le signataire ne signe que pour un serveur
//! enregistre et non revoque, et seulement pour la meme cle publique.
//! PostgreSQL requis (voir `common::database`).

mod common;

use std::path::Path;

use control_plane::agents::certs::{self, Renewals};
use control_plane::agents::signer::{self, CertRequest, Usage};
use control_plane::agents::{servers, transport};
use pki::Ca;
use shared::ids::ServerId;

/// Dossier de configuration du service apres `ciphera init` : certificat de
/// la CA, lien, panneau ; la cle de la CA n'y est posee que par
/// [`with_ca_key`] (cote root).
fn config_dir(ca: &Ca) -> common::TempDir {
    let tmp = common::TempDir::new();
    for sub in ["ca", "tls", "agents"] {
        std::fs::create_dir_all(tmp.0.join(sub)).unwrap();
    }
    std::fs::write(tmp.0.join("ca/ca.crt"), ca.cert_pem()).unwrap();
    for (crt, key) in certs::OWN_CERTS {
        let mut b = ca
            .issue_server_cert(&["203.0.113.7".into(), "localhost".into()])
            .unwrap();
        std::fs::write(tmp.0.join(crt), &b.cert_pem).unwrap();
        std::fs::write(tmp.0.join(key), b.key_pem.take().unwrap().as_bytes()).unwrap();
    }
    tmp
}

/// Cle de la CA posee comme par root : 0600, proprietaire du processus.
fn with_ca_key(dir: &Path, ca: &Ca) {
    let path = dir.join(certs::CA_KEY);
    std::fs::write(&path, ca.key_pem().as_bytes()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn request_path(data: &Path, name: &str) -> std::path::PathBuf {
    data.join(signer::REQUESTS_DIR).join(format!("{name}.json"))
}

fn response_path(data: &Path, name: &str) -> std::path::PathBuf {
    data.join(signer::RESPONSES_DIR).join(format!("{name}.crt"))
}

async fn next_fingerprint(db: &sqlx::PgPool, id: ServerId) -> Option<String> {
    sqlx::query_scalar("SELECT next_cert_fingerprint FROM servers WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
        .unwrap()
}

/// Agent enregistre : la demande est signee pour la meme cle, l'empreinte
/// du nouveau certificat est notee en base, la reponse porte l'identifiant
/// de la demande et la demande est supprimee.
#[tokio::test]
async fn signer_signs_a_registered_agent_for_the_same_key() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    with_ca_key(&dir.0, &ca);
    let data = common::TempDir::new();
    let id = ServerId::new();
    let key = pki::AgentKey::generate().unwrap();
    let cert = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    servers::register_local(&db, &cert.cert_pem).await.unwrap();
    let der = pki::der_from_pem(&cert.cert_pem).unwrap();
    let request = CertRequest::agent(id, &der).unwrap();
    signer::write_request(&data.0, &request).unwrap();
    assert!(signer::has_requests(&data.0).unwrap());

    let report = signer::sign_requests(&db, &dir.0, &data.0).await.unwrap();
    assert_eq!(report.signed, vec![id.to_string()]);
    assert!(report.refused.is_empty(), "{:?}", report.refused);
    assert!(!request_path(&data.0, &id.to_string()).exists());
    assert!(!signer::has_requests(&data.0).unwrap());

    let (answered, pem) = signer::read_response(&data.0, &id.to_string())
        .unwrap()
        .unwrap();
    assert_eq!(answered, request.request_id);
    let new_der = pki::der_from_pem(&pem).unwrap();
    assert_eq!(pki::agent_id_from_cert(&new_der), Some(id));
    assert_eq!(pki::public_key_der(&new_der).unwrap(), key.public_key_der());
    assert!(pki::issued_by(
        &pki::der_from_pem(ca.cert_pem()).unwrap(),
        &new_der
    ));
    assert_eq!(
        next_fingerprint(&db, id).await,
        Some(pki::fingerprint_sha256(&new_der))
    );
}

/// Jamais de nouvelle cle publique : empreinte de cle differente du
/// certificat joint, certificat d'une autre cle (meme CA, meme identifiant,
/// empreinte non enregistree), certificat du lien d'une autre cle. Rien
/// n'est signe ni note, la demande est supprimee.
#[tokio::test]
async fn signer_refuses_new_public_key() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    with_ca_key(&dir.0, &ca);
    let data = common::TempDir::new();
    let id = ServerId::new();
    let key = pki::AgentKey::generate().unwrap();
    let registered = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    servers::register_local(&db, &registered.cert_pem)
        .await
        .unwrap();
    let other_key = pki::AgentKey::generate().unwrap();
    let other = ca.issue_agent_cert(id, other_key.public_key_der()).unwrap();
    let other_der = pki::der_from_pem(&other.cert_pem).unwrap();

    // Empreinte de cle annoncee differente de celle du certificat joint.
    let mut mismatch =
        CertRequest::agent(id, &pki::der_from_pem(&registered.cert_pem).unwrap()).unwrap();
    mismatch.public_key_sha256 = signer::public_key_sha256(&other_der).unwrap();
    // Certificat d'une autre cle, coherent mais non enregistre.
    let new_key = CertRequest::agent(id, &other_der).unwrap();
    for request in [mismatch, new_key] {
        signer::write_request(&data.0, &request).unwrap();
        let report = signer::sign_requests(&db, &dir.0, &data.0).await.unwrap();
        assert!(report.signed.is_empty());
        assert_eq!(report.refused.len(), 1, "{:?}", report.refused);
        assert!(!request_path(&data.0, &id.to_string()).exists());
        assert!(!response_path(&data.0, &id.to_string()).exists());
        assert_eq!(next_fingerprint(&db, id).await, None);
    }

    // Lien : cle annoncee autre que celle de `tls/link.crt`.
    let link_pem = std::fs::read_to_string(dir.0.join(transport::LINK_CRT)).unwrap();
    let mut link = CertRequest::own(Usage::Link, &link_pem).unwrap();
    link.public_key_sha256 = signer::public_key_sha256(&other_der).unwrap();
    signer::write_request(&data.0, &link).unwrap();
    let report = signer::sign_requests(&db, &dir.0, &data.0).await.unwrap();
    assert!(report.signed.is_empty());
    assert_eq!(report.refused.len(), 1);
    assert!(!response_path(&data.0, "link").exists());
    assert!(!request_path(&data.0, "link").exists());
}

/// Serveur revoque : meme le certificat enregistre n'est pas re-signe.
#[tokio::test]
async fn signer_refuses_revoked_server() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    with_ca_key(&dir.0, &ca);
    let data = common::TempDir::new();
    let id = ServerId::new();
    let key = pki::AgentKey::generate().unwrap();
    let cert = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    servers::register_local(&db, &cert.cert_pem).await.unwrap();
    sqlx::query("UPDATE servers SET revoked_at = now() WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    let request = CertRequest::agent(id, &pki::der_from_pem(&cert.cert_pem).unwrap()).unwrap();
    signer::write_request(&data.0, &request).unwrap();
    let report = signer::sign_requests(&db, &dir.0, &data.0).await.unwrap();
    assert!(report.signed.is_empty());
    assert_eq!(report.refused.len(), 1);
    assert!(
        report.refused[0].1.contains("revoque"),
        "{:?}",
        report.refused
    );
    assert!(!request_path(&data.0, &id.to_string()).exists());
    assert!(!response_path(&data.0, &id.to_string()).exists());
    assert_eq!(next_fingerprint(&db, id).await, None);
}

/// Demande en lien symbolique (vers une demande valide) : refusee sans
/// suivre le lien ; le lien est retire, sa cible reste.
#[cfg(unix)]
#[tokio::test]
async fn signer_refuses_symlink_request() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    with_ca_key(&dir.0, &ca);
    let data = common::TempDir::new();
    let elsewhere = common::TempDir::new();
    let id = ServerId::new();
    let key = pki::AgentKey::generate().unwrap();
    let cert = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    servers::register_local(&db, &cert.cert_pem).await.unwrap();
    let request = CertRequest::agent(id, &pki::der_from_pem(&cert.cert_pem).unwrap()).unwrap();
    signer::write_request(&elsewhere.0, &request).unwrap();
    let target = request_path(&elsewhere.0, &id.to_string());
    std::fs::create_dir_all(data.0.join(signer::REQUESTS_DIR)).unwrap();
    let link = request_path(&data.0, &id.to_string());
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let report = signer::sign_requests(&db, &dir.0, &data.0).await.unwrap();
    assert!(report.signed.is_empty());
    assert_eq!(report.refused.len(), 1, "{:?}", report.refused);
    assert!(std::fs::symlink_metadata(&link).is_err(), "lien retire");
    assert!(target.is_file(), "cible intacte");
    assert!(!response_path(&data.0, &id.to_string()).exists());
    assert_eq!(next_fingerprint(&db, id).await, None);

    // Dossier des demandes remplace par un lien : rien n'est lu.
    std::fs::remove_dir(data.0.join(signer::REQUESTS_DIR)).unwrap();
    std::os::unix::fs::symlink(
        elsewhere.0.join(signer::REQUESTS_DIR),
        data.0.join(signer::REQUESTS_DIR),
    )
    .unwrap();
    assert!(signer::sign_requests(&db, &dir.0, &data.0).await.is_err());
    assert!(target.is_file());
    assert_eq!(next_fingerprint(&db, id).await, None);
}

/// Toute autre forme est refusee et supprimee : champ inconnu, nom de
/// fichier different du contenu, usage du lien avec un serveur, JSON
/// illisible, fichier trop long.
#[tokio::test]
async fn signer_refuses_other_shapes() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    with_ca_key(&dir.0, &ca);
    let data = common::TempDir::new();
    let id = ServerId::new();
    let key = pki::AgentKey::generate().unwrap();
    let cert = ca.issue_agent_cert(id, key.public_key_der()).unwrap();
    servers::register_local(&db, &cert.cert_pem).await.unwrap();
    let request = CertRequest::agent(id, &pki::der_from_pem(&cert.cert_pem).unwrap()).unwrap();
    let mut good = serde_json::to_value(&request).unwrap();
    let requests = data.0.join(signer::REQUESTS_DIR);
    std::fs::create_dir_all(&requests).unwrap();

    good["extra"] = serde_json::json!(1);
    std::fs::write(
        requests.join(format!("{id}.json")),
        serde_json::to_vec(&good).unwrap(),
    )
    .unwrap();
    good.as_object_mut().unwrap().remove("extra");
    std::fs::write(
        requests.join(format!("{}.json", ServerId::new())),
        serde_json::to_vec(&good).unwrap(),
    )
    .unwrap();
    good["usage"] = serde_json::json!("link");
    std::fs::write(
        requests.join("link.json"),
        serde_json::to_vec(&good).unwrap(),
    )
    .unwrap();
    std::fs::write(requests.join("panel.json"), b"pas du json").unwrap();
    std::fs::write(requests.join("other.json"), vec![b' '; 64 * 1024]).unwrap();

    let report = signer::sign_requests(&db, &dir.0, &data.0).await.unwrap();
    assert!(report.signed.is_empty(), "{:?}", report.signed);
    assert_eq!(report.refused.len(), 5, "{:?}", report.refused);
    assert!(!signer::has_requests(&data.0).unwrap());
    assert_eq!(next_fingerprint(&db, id).await, None);
    let responses = data.0.join(signer::RESPONSES_DIR);
    assert!(
        !responses.exists() || std::fs::read_dir(&responses).unwrap().next().is_none(),
        "aucune reponse"
    );
}

/// Le plan de controle ne lit jamais `ca/ca.key` : l'unite la lui rend
/// inaccessible, le code du service ne la nomme pas, et ses
/// renouvellements (lien, panneau) passent par une demande au signataire
/// sur un dossier de configuration qui n'a pas cette cle.
#[tokio::test]
async fn control_plane_never_opens_ca_key() {
    // Configuration : cle masquee au service par systemd.
    let unit = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/systemd/ciphera.service"),
    )
    .unwrap();
    assert!(
        unit.lines()
            .map(str::trim)
            .any(|l| l == "InaccessiblePaths=-/etc/ciphera/ca/ca.key"),
        "ciphera.service doit masquer ca.key"
    );
    // Chemins : ni le service ni le lien ne chargent la CA.
    for src in ["src/main.rs", "src/agents/transport.rs"] {
        let text =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(src)).unwrap();
        for forbidden in ["load_ca", "CA_KEY", "ca.key"] {
            assert!(!text.contains(forbidden), "{src} contient {forbidden}");
        }
    }

    // Execution : renouvellement du lien et du panneau sans la cle.
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    let data = common::TempDir::new();
    let key_before = std::fs::read_to_string(dir.0.join(transport::LINK_KEY)).unwrap();
    let crt_before = std::fs::read_to_string(dir.0.join(transport::LINK_CRT)).unwrap();
    let renewals = Renewals::new(&dir.0, &data.0);
    assert!(renewals.request_own(certs::now_unix()).is_empty(), "pas du");
    let later = certs::now_unix() + 700 * 86_400;
    assert_eq!(
        renewals.request_own(later),
        vec![transport::LINK_CRT, certs::PANEL_CRT]
    );
    // Deja demandes : pas de doublon.
    assert!(renewals.request_own(later).is_empty());
    assert!(request_path(&data.0, "link").is_file());
    assert!(request_path(&data.0, "panel").is_file());
    assert!(!dir.0.join(certs::CA_KEY).exists());

    // Signataire (root) : la cle n'existe que le temps de signer.
    with_ca_key(&dir.0, &ca);
    let report = signer::sign_requests(&db, &dir.0, &data.0).await.unwrap();
    assert_eq!(report.signed.len(), 2, "{:?}", report.refused);
    std::fs::remove_file(dir.0.join(certs::CA_KEY)).unwrap();

    assert_eq!(
        renewals.install_own_responses(certs::now_unix()),
        vec![transport::LINK_CRT, certs::PANEL_CRT]
    );
    let crt_after = std::fs::read_to_string(dir.0.join(transport::LINK_CRT)).unwrap();
    assert_ne!(crt_after, crt_before);
    assert_eq!(
        std::fs::read_to_string(dir.0.join(transport::LINK_KEY)).unwrap(),
        key_before,
        "meme cle"
    );
    transport::server_config(&transport::LinkTls::load(&dir.0).unwrap()).unwrap();
    // Reponse deja appliquee : rien de plus.
    assert!(renewals.install_own_responses(certs::now_unix()).is_empty());
}

/// Reponse d'une autre demande (course : ancienne reponse restee sur
/// disque) : ignoree, le certificat n'est pas installe.
#[tokio::test]
async fn response_to_another_request_is_ignored() {
    let (db, _guard) = common::database().await;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    let data = common::TempDir::new();
    let renewals = Renewals::new(&dir.0, &data.0);
    let later = certs::now_unix() + 700 * 86_400;
    assert_eq!(renewals.request_own(later).len(), 2);
    // Reponse signee pour une demande anterieure (autre identifiant).
    let link_pem = std::fs::read_to_string(dir.0.join(transport::LINK_CRT)).unwrap();
    let stale = CertRequest::own(Usage::Link, &link_pem).unwrap();
    std::fs::remove_file(request_path(&data.0, "link")).unwrap();
    signer::write_request(&data.0, &stale).unwrap();
    with_ca_key(&dir.0, &ca);
    signer::sign_requests(&db, &dir.0, &data.0).await.unwrap();
    let before = std::fs::read_to_string(dir.0.join(transport::LINK_CRT)).unwrap();
    assert_eq!(
        renewals.install_own_responses(certs::now_unix()),
        vec![certs::PANEL_CRT]
    );
    assert_eq!(
        std::fs::read_to_string(dir.0.join(transport::LINK_CRT)).unwrap(),
        before
    );
}

/// Cle de la CA d'une installation anterieure (compte du service, 0640) :
/// remise au proprietaire du signataire en 0600 ; un lien est refuse.
#[cfg(unix)]
#[test]
fn ca_key_is_hardened_in_place() {
    use std::os::unix::fs::PermissionsExt;
    let ca = Ca::generate("instance").unwrap();
    let dir = config_dir(&ca);
    let path = dir.0.join(certs::CA_KEY);
    std::fs::write(&path, ca.key_pem().as_bytes()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(signer::harden_ca_key(&dir.0).unwrap());
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert!(!signer::harden_ca_key(&dir.0).unwrap(), "deja fait");
    certs::load_ca(&dir.0).unwrap();

    std::fs::remove_file(&path).unwrap();
    let elsewhere = common::TempDir::new();
    std::fs::write(elsewhere.0.join("k"), "x").unwrap();
    std::os::unix::fs::symlink(elsewhere.0.join("k"), &path).unwrap();
    assert!(signer::harden_ca_key(&dir.0).is_err());
    assert!(certs::load_ca(&dir.0).is_err());
}
