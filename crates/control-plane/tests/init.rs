//! Tests de `ciphera init` (secrets d'instance, PKI, jeton de setup) et du
//! coffre de secrets age. Ne demandent ni base ni Docker.

use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

use control_plane::init::{self, InitOptions};
use control_plane::secrets::SecretBox;
use x509_parser::prelude::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("ciphera-init-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const IP: &str = "203.0.113.7";

fn opts(dir: &Path) -> InitOptions {
    InitOptions {
        config_dir: dir.to_path_buf(),
        public_ip: IP.parse::<IpAddr>().unwrap(),
        panel_port: 8443,
        owner: None,
        agent_dir: dir.join(AGENT_DIR),
        agent_owner: None,
        postgres_owner: None,
    }
}

/// Dossier de l'agent local dans les tests (ailleurs `/etc/ciphera-agent`).
const AGENT_DIR: &str = "agent-home";

const FILES: [&str; 14] = [
    "db_password",
    "age.key",
    "ca/ca.crt",
    "ca/ca.key",
    "tls/panel.crt",
    "tls/panel.key",
    "tls/link.crt",
    "tls/link.key",
    "agents/local.crt",
    "setup_token",
    "postgres/db_password",
    "agent-home/agent.key",
    "agent-home/agent.crt",
    "agent-home/ca.crt",
];

fn read(dir: &Path, rel: &str) -> String {
    fs::read_to_string(dir.join(rel)).unwrap()
}

#[test]
fn init_writes_all_files() {
    let tmp = TempDir::new();
    init::run(&opts(tmp.path())).unwrap();
    for f in FILES {
        assert!(tmp.path().join(f).is_file(), "{f} absent");
    }
}

#[cfg(unix)]
#[test]
fn init_writes_files_0600() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new();
    init::run(&opts(tmp.path())).unwrap();
    for f in FILES {
        let mode = fs::metadata(tmp.path().join(f))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "{f} : {mode:o}");
    }
    for d in ["ca", "tls", "agents", "postgres", AGENT_DIR] {
        let mode = fs::metadata(tmp.path().join(d))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "{d}/ : {mode:o}");
    }
    // Aucun fichier temporaire ne subsiste.
    for d in ["", "ca", "tls", "agents", "postgres", AGENT_DIR] {
        for e in fs::read_dir(tmp.path().join(d)).unwrap() {
            let name = e.unwrap().file_name();
            assert!(!name.to_string_lossy().contains(".tmp"), "{name:?}");
        }
    }
}

#[test]
fn init_db_password_is_32_bytes_random() {
    use base64::Engine;
    let (a, b) = (TempDir::new(), TempDir::new());
    init::run(&opts(a.path())).unwrap();
    init::run(&opts(b.path())).unwrap();
    let (pa, pb) = (read(a.path(), "db_password"), read(b.path(), "db_password"));
    assert_ne!(pa, pb);
    for p in [&pa, &pb] {
        let raw = base64::engine::general_purpose::URL_SAFE.decode(p).unwrap();
        assert_eq!(raw.len(), 32);
        assert!(!p.contains('\n'));
    }
}

#[test]
fn init_writes_the_postgres_password_copy() {
    let tmp = TempDir::new();
    init::run(&opts(tmp.path())).unwrap();
    assert_eq!(
        read(tmp.path(), "postgres/db_password"),
        read(tmp.path(), "db_password")
    );
}

#[test]
fn init_is_idempotent_refuses_overwrite() {
    let tmp = TempDir::new();
    init::run(&opts(tmp.path())).unwrap();
    let before: Vec<String> = FILES.iter().map(|f| read(tmp.path(), f)).collect();
    let err = init::run(&opts(tmp.path())).unwrap_err();
    assert!(err.to_string().contains("existe deja"), "{err}");
    let after: Vec<String> = FILES.iter().map(|f| read(tmp.path(), f)).collect();
    assert_eq!(before, after, "rien ne doit etre ecrase");
}

#[test]
fn init_with_one_existing_file_writes_nothing() {
    let tmp = TempDir::new();
    fs::write(tmp.path().join("setup_token"), "ancien").unwrap();
    let err = init::run(&opts(tmp.path())).unwrap_err();
    assert!(err.to_string().contains("setup_token"), "{err}");
    assert_eq!(read(tmp.path(), "setup_token"), "ancien");
    for f in FILES.iter().filter(|f| **f != "setup_token") {
        assert!(!tmp.path().join(f).exists(), "{f} ne doit pas etre cree");
    }
}

#[test]
fn init_with_an_existing_agent_file_writes_nothing() {
    let tmp = TempDir::new();
    fs::create_dir(tmp.path().join(AGENT_DIR)).unwrap();
    fs::write(tmp.path().join(AGENT_DIR).join("agent.key"), "ancienne").unwrap();
    let err = init::run(&opts(tmp.path())).unwrap_err();
    assert!(err.to_string().contains("agent.key"), "{err}");
    assert_eq!(read(tmp.path(), "agent-home/agent.key"), "ancienne");
    for f in FILES.iter().filter(|f| **f != "agent-home/agent.key") {
        assert!(!tmp.path().join(f).exists(), "{f} ne doit pas etre cree");
    }
}

fn der(pem: &str) -> Vec<u8> {
    x509_parser::pem::parse_x509_pem(pem.as_bytes())
        .unwrap()
        .1
        .contents
}

#[test]
fn init_enrols_the_local_agent() {
    let tmp = TempDir::new();
    init::run(&opts(tmp.path())).unwrap();
    let ca_pem = read(tmp.path(), "ca/ca.crt");
    let ca_der = der(&ca_pem);
    let (_, ca) = X509Certificate::from_der(&ca_der).unwrap();

    // Identite de l'agent lisible par l'agent, certificat signe par la CA.
    let identity = ciphera_agent::config::Identity::load(&tmp.path().join(AGENT_DIR)).unwrap();
    let agent_pem = read(tmp.path(), "agent-home/agent.crt");
    let agent_der = der(&agent_pem);
    let (_, agent) = X509Certificate::from_der(&agent_der).unwrap();
    agent.verify_signature(Some(ca.public_key())).unwrap();
    assert_eq!(pki::agent_id_from_cert(&agent_der), Some(identity.agent_id));
    assert_eq!(read(tmp.path(), "agent-home/ca.crt"), ca_pem);
    // Copie publique pour l'enregistrement du serveur local.
    assert_eq!(read(tmp.path(), "agents/local.crt"), agent_pem);
    // La cle privee de l'agent correspond au certificat, et n'est nulle part
    // ailleurs que dans son dossier.
    let key = rcgen::KeyPair::from_pem(&read(tmp.path(), "agent-home/agent.key")).unwrap();
    assert_eq!(
        agent.public_key().raw,
        rcgen::PublicKeyData::subject_public_key_info(&key).as_slice()
    );
    for f in FILES.iter().filter(|f| !f.starts_with(AGENT_DIR)) {
        assert!(!read(tmp.path(), f).contains(&read(tmp.path(), "agent-home/agent.key")));
    }

    // Certificat serveur du lien : CA de l'instance, SAN localhost et IP.
    let link_der = der(&read(tmp.path(), "tls/link.crt"));
    let (_, link) = X509Certificate::from_der(&link_der).unwrap();
    link.verify_signature(Some(ca.public_key())).unwrap();
    let sans = &link
        .subject_alternative_name()
        .unwrap()
        .unwrap()
        .value
        .general_names;
    assert!(
        sans.iter()
            .any(|n| matches!(n, GeneralName::DNSName("localhost")))
    );
    assert_ne!(link_der, der(&read(tmp.path(), "tls/panel.crt")));
}

#[test]
fn init_report_matches_files() {
    use base64::Engine;
    let tmp = TempDir::new();
    let report = init::run(&opts(tmp.path())).unwrap();

    // URL : jeton lu dans le fichier (dans le fragment), port et IP demandes ;
    // echeance a 24 heures.
    let stored = control_plane::setup_token::parse(&read(tmp.path(), "setup_token")).unwrap();
    let token = stored.token.to_string();
    assert_eq!(
        report.setup_url,
        format!("https://{IP}:8443/setup#token={token}")
    );
    let left = stored.expires_at.unwrap() - control_plane::setup_token::unix_now();
    assert!((24 * 3600 - 60..=24 * 3600).contains(&left), "{left}");
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&token)
        .unwrap();
    assert_eq!(raw.len(), 32);

    // Empreinte : celle du certificat du panneau.
    let pem = read(tmp.path(), "tls/panel.crt");
    let der = x509_parser::pem::parse_x509_pem(pem.as_bytes())
        .unwrap()
        .1
        .contents;
    assert_eq!(report.fingerprint, pki::fingerprint_sha256(&der));

    // Le certificat est emis par la CA de l'instance, avec l'IP en SAN.
    let ca_pem = read(tmp.path(), "ca/ca.crt");
    let ca_der = x509_parser::pem::parse_x509_pem(ca_pem.as_bytes())
        .unwrap()
        .1
        .contents;
    let (_, ca) = X509Certificate::from_der(&ca_der).unwrap();
    let (_, cert) = X509Certificate::from_der(&der).unwrap();
    cert.verify_signature(Some(ca.public_key())).unwrap();
    let sans = &cert
        .subject_alternative_name()
        .unwrap()
        .unwrap()
        .value
        .general_names;
    assert!(
        sans.iter()
            .any(|n| matches!(n, GeneralName::IPAddress(ip) if *ip == [203, 0, 113, 7]))
    );
    assert!(
        sans.iter()
            .any(|n| matches!(n, GeneralName::DNSName("localhost")))
    );
}

#[test]
fn init_rendered_output_has_url_then_fingerprint_and_no_secret() {
    let tmp = TempDir::new();
    let report = init::run(&opts(tmp.path())).unwrap();
    let out = report.render();
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines.iter().any(|l| *l == report.setup_url));
    let fp_line = format!("Empreinte SHA-256 : {}", report.fingerprint);
    assert!(lines.iter().any(|l| *l == fp_line), "{out}");
    let url_at = lines.iter().position(|l| *l == report.setup_url).unwrap();
    let fp_at = lines.iter().position(|l| *l == fp_line).unwrap();
    assert!(url_at < fp_at);
    assert!(!out.contains(&read(tmp.path(), "db_password")));
    assert!(!out.contains(&read(tmp.path(), "age.key").trim().to_string()));
    assert!(!out.contains("PRIVATE KEY"));
}

#[test]
fn init_writes_a_usable_age_identity() {
    let tmp = TempDir::new();
    init::run(&opts(tmp.path())).unwrap();
    let sb = SecretBox::load(&tmp.path().join("age.key")).unwrap();
    let sealed = sb.seal(b"valeur secrete").unwrap();
    assert_eq!(&*sb.open(&sealed).unwrap(), b"valeur secrete");
}

#[test]
fn init_brackets_ipv6_in_url() {
    let tmp = TempDir::new();
    let mut o = opts(tmp.path());
    o.public_ip = "2001:db8::1".parse().unwrap();
    let report = init::run(&o).unwrap();
    assert!(
        report
            .setup_url
            .starts_with("https://[2001:db8::1]:8443/setup#token="),
        "{}",
        report.setup_url
    );
}

#[test]
fn cli_init_prints_url_and_fingerprint() {
    let tmp = TempDir::new();
    let out = Command::new(env!("CARGO_BIN_EXE_ciphera"))
        .args(["init", "--config-dir"])
        .arg(tmp.path())
        .args(["--public-ip", IP])
        .arg("--agent-dir")
        .arg(tmp.path().join(AGENT_DIR))
        .env_remove("CIPHERA_DATABASE_URL")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains(&format!("https://{IP}:8443/setup#token=")),
        "{stdout}"
    );
    assert!(stdout.contains("Empreinte SHA-256 : "), "{stdout}");
    assert!(!stdout.contains(&read(tmp.path(), "db_password")));

    // Deuxieme execution : echec net, code non nul.
    let again = Command::new(env!("CARGO_BIN_EXE_ciphera"))
        .args(["init", "--config-dir"])
        .arg(tmp.path())
        .args(["--public-ip", IP])
        .arg("--agent-dir")
        .arg(tmp.path().join(AGENT_DIR))
        .output()
        .unwrap();
    assert!(!again.status.success());
}

#[test]
fn cli_init_rejects_a_non_ip_address() {
    let tmp = TempDir::new();
    let out = Command::new(env!("CARGO_BIN_EXE_ciphera"))
        .args(["init", "--config-dir"])
        .arg(tmp.path())
        .args(["--public-ip", "pas-une-ip; rm -rf /"])
        .arg("--agent-dir")
        .arg(tmp.path().join(AGENT_DIR))
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!tmp.path().join("setup_token").exists());
}

#[test]
fn seal_open_roundtrip() {
    let tmp = TempDir::new();
    init::run(&opts(tmp.path())).unwrap();
    let sb = SecretBox::load(&tmp.path().join("age.key")).unwrap();
    let sealed = sb.seal(b"").unwrap();
    assert_eq!(sb.open(&sealed).unwrap().len(), 0);
    let sealed = sb.seal(b"hello").unwrap();
    assert_ne!(sealed.as_slice(), b"hello");
    assert_eq!(&*sb.open(&sealed).unwrap(), b"hello");
}

#[test]
fn open_with_other_identity_fails() {
    let (a, b) = (TempDir::new(), TempDir::new());
    init::run(&opts(a.path())).unwrap();
    init::run(&opts(b.path())).unwrap();
    let sa = SecretBox::load(&a.path().join("age.key")).unwrap();
    let sb = SecretBox::load(&b.path().join("age.key")).unwrap();
    let sealed = sa.seal(b"secret").unwrap();
    assert!(sb.open(&sealed).is_err());
    assert!(sa.open(b"pas du age").is_err());
}

#[test]
fn load_rejects_garbage_identity() {
    let tmp = TempDir::new();
    let p = tmp.path().join("age.key");
    fs::write(&p, "pas une identite\n").unwrap();
    assert!(SecretBox::load(&p).is_err());
    fs::write(&p, "# juste un commentaire\n").unwrap();
    assert!(SecretBox::load(&p).is_err());
    assert!(SecretBox::load(&tmp.path().join("absent")).is_err());
}

#[cfg(unix)]
#[test]
fn init_refuses_a_symlinked_ca_dir() {
    let tmp = TempDir::new();
    let elsewhere = TempDir::new();
    std::os::unix::fs::symlink(elsewhere.path(), tmp.path().join("ca")).unwrap();
    let err = init::run(&opts(tmp.path())).unwrap_err();
    assert!(err.to_string().contains("lien symbolique"), "{err}");
    assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    assert!(!tmp.path().join("setup_token").exists());
}

#[cfg(unix)]
#[test]
fn init_refuses_a_world_writable_tls_dir() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new();
    let tls = tmp.path().join("tls");
    fs::create_dir(&tls).unwrap();
    fs::set_permissions(&tls, fs::Permissions::from_mode(0o777)).unwrap();
    let err = init::run(&opts(tmp.path())).unwrap_err();
    assert!(err.to_string().contains("modifiable"), "{err}");
    assert!(!tmp.path().join("setup_token").exists());
    assert!(!tmp.path().join("ca").exists());
}

/// Cle des demandes de mise a jour : 32 octets aleatoires bruts, propres a
/// chaque instance, sans lien avec l'identite age ; lisible par
/// `shared::update::load_key` ; jamais reecrite par une seconde init.
#[test]
fn init_writes_the_update_hmac_key() {
    let (a, b) = (TempDir::new(), TempDir::new());
    init::run(&opts(a.path())).unwrap();
    init::run(&opts(b.path())).unwrap();
    let path = a.path().join(shared::update::KEY_FILE);
    let ka = fs::read(&path).unwrap();
    let kb = fs::read(b.path().join(shared::update::KEY_FILE)).unwrap();
    assert_eq!(ka.len(), 32);
    assert_ne!(ka, kb);
    assert_eq!(&*shared::update::load_key(&path).unwrap(), &ka[..]);
    assert!(
        !read(a.path(), "age.key")
            .as_bytes()
            .windows(32)
            .any(|w| w == ka)
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "sans --owner : 0600 (0640 root:ciphera sinon)");
    }
    assert!(init::run(&opts(a.path())).is_err());
    assert_eq!(fs::read(&path).unwrap(), ka, "jamais reecrite");
}
