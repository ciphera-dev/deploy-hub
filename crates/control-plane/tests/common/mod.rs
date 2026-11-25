#![allow(dead_code)]
//! Outils communs aux tests d'integration : PostgreSQL 17 (testcontainers),
//! certificat auto-signe jetable (rcgen), serveur HTTPS reel et client brut.

pub mod api;

use std::net::SocketAddr;
use std::sync::Arc;

use axum_server::Handle;
use axum_server::tls_rustls::RustlsConfig;
use control_plane::mail::Delivery;
use control_plane::secrets::SecretBox;
use control_plane::state::Clock;
use control_plane::{app, state::AppState};
use rustls::pki_types::{CertificateDer, ServerName};
use shared::config::Config;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use testcontainers::ContainerAsync;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Garde de la base de test : conteneur testcontainers (CI) ou base creee sur
/// le serveur de `CIPHERA_TEST_DATABASE_URL` (supprimee a la fin, au mieux).
pub struct TestDb {
    _container: Option<ContainerAsync<Postgres>>,
    drop_on: Option<(String, String)>, // (url admin, nom de la base)
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let Some((admin_url, name)) = self.drop_on.take() else {
            return;
        };
        // Meilleur effort : nouveau thread et nouvelle runtime (le test est fini).
        let _ = std::thread::spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(async {
                if let Ok(pool) = PgPoolOptions::new()
                    .max_connections(1)
                    .connect(&admin_url)
                    .await
                {
                    let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
                        "DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"
                    )))
                    .execute(&pool)
                    .await;
                }
            });
        })
        .join();
    }
}

/// Base PostgreSQL jetable, migrations appliquees. Garder la garde en vie.
/// Avec `CIPHERA_TEST_DATABASE_URL` : base neuve sur ce serveur ; sinon
/// PostgreSQL 17 via testcontainers.
pub async fn database() -> (PgPool, TestDb) {
    let (pool, guard) = bare_database().await;
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations");
    (pool, guard)
}

/// Comme `database`, sans aucune migration (tests de migration).
pub async fn bare_database() -> (PgPool, TestDb) {
    let (url, guard) = match std::env::var("CIPHERA_TEST_DATABASE_URL") {
        Ok(admin_url) if !admin_url.is_empty() => {
            let name = format!("ciphera_test_{}", uuid::Uuid::new_v4().simple());
            let admin = PgPoolOptions::new()
                .max_connections(1)
                .connect(&admin_url)
                .await
                .expect("connexion au serveur de test");
            sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE \"{name}\"")))
                .execute(&admin)
                .await
                .expect("creation de la base de test");
            admin.close().await;
            let (base, _) = admin_url
                .split_once('?')
                .unwrap_or((admin_url.as_str(), ""));
            let (prefix, _) = base.rsplit_once('/').expect("URL de base invalide");
            (
                format!("{prefix}/{name}"),
                TestDb {
                    _container: None,
                    drop_on: Some((admin_url, name)),
                },
            )
        }
        _ => {
            let container = Postgres::default()
                .with_tag("17")
                .start()
                .await
                .expect("demarrage de PostgreSQL 17 (Docker joignable ?)");
            let port = container.get_host_port_ipv4(5432).await.expect("port");
            (
                format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres"),
                TestDb {
                    _container: Some(container),
                    drop_on: None,
                },
            )
        }
    };
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connexion");
    (pool, guard)
}
pub fn test_config() -> Arc<Config> {
    Arc::new(
        Config::from_lookup(|name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
            // Port ferme : aucun test n'atteint le vrai GitHub.
            "CIPHERA_GITHUB_API_URL" => Some("http://127.0.0.1:9".to_string()),
            _ => None,
        })
        .unwrap(),
    )
}

/// Dossier temporaire supprime a la fin du test.
pub struct TempDir(pub std::path::PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("ciphera-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Configuration dont `config_dir` et `data_dir` sont les dossiers donnes.
pub fn test_config_in(config_dir: &std::path::Path, data_dir: &std::path::Path) -> Arc<Config> {
    let config = config_dir.to_string_lossy().into_owned();
    let data = data_dir.to_string_lossy().into_owned();
    Arc::new(
        Config::from_lookup(|name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
            "CIPHERA_CONFIG_DIR" => Some(config.clone()),
            "CIPHERA_DATA_DIR" => Some(data.clone()),
            "CIPHERA_GITHUB_API_URL" => Some("http://127.0.0.1:9".to_string()),
            _ => None,
        })
        .unwrap(),
    )
}

/// Etat dont les dossiers de configuration et de donnees sont donnes.
pub async fn state_in(
    config_dir: &std::path::Path,
    data_dir: &std::path::Path,
) -> (AppState, TestDb) {
    let (mut state, guard) = state().await;
    state.cfg = test_config_in(config_dir, data_dir);
    (state, guard)
}

pub async fn state() -> (AppState, TestDb) {
    let (db, container) = database().await;
    let agents = control_plane::agents::registry::AgentRegistry::new();
    let secrets = Arc::new(SecretBox::from_identity(age::x25519::Identity::generate()));
    // Masquage reel : valeurs secretes des variables du projet.
    let redaction = Arc::new(control_plane::domain::variables::VariableSecrets::new(
        db.clone(),
        secrets.clone(),
    ));
    (
        AppState {
            db,
            cfg: test_config(),
            secrets,
            clock: Clock::system(),
            mail: Delivery::smtp(),
            github: Arc::new(control_plane::connectors::github::app::GithubClient::new().unwrap()),
            logs: control_plane::domain::logs::hub::LogHub::new(agents.clone(), redaction),
            agents,
            dns: Arc::new(control_plane::domain::domains::SystemResolver),
            panel: control_plane::domain::domains::panel::PanelSwitch::default(),
            ip_allowlist: Default::default(),
            access: Default::default(),
            releases: Default::default(),
            update_request_dir: shared::update::REQUEST_DIR.into(),
            public_limits: Default::default(),
            login_flights: Default::default(),
        },
        container,
    )
}

pub struct Cert {
    pub cert_pem: String,
    pub key_pem: String,
    pub cert_der: CertificateDer<'static>,
}

pub fn self_signed() -> Cert {
    let ck = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    Cert {
        cert_pem: ck.cert.pem(),
        key_pem: ck.signing_key.serialize_pem(),
        cert_der: ck.cert.der().clone(),
    }
}

/// Lance le routeur derriere un vrai TLS sur un port libre.
pub async fn serve_https(state: AppState, cert: &Cert) -> (SocketAddr, Handle<SocketAddr>) {
    let config = RustlsConfig::from_pem(
        cert.cert_pem.clone().into_bytes(),
        cert.key_pem.clone().into_bytes(),
    )
    .await
    .unwrap();
    let handle = Handle::new();
    let server = axum_server::bind_rustls("127.0.0.1:0".parse().unwrap(), config)
        .handle(handle.clone())
        .serve(app::build_router(state).into_make_service_with_connect_info::<SocketAddr>());
    tokio::spawn(server);
    let addr = handle.listening().await.expect("adresse d'ecoute");
    (addr, handle)
}

/// Lance l'ecouteur de redirection sur un port libre.
pub async fn serve_redirect(https_port: u16) -> (SocketAddr, Handle<SocketAddr>) {
    serve_redirect_with(https_port, Default::default()).await
}

/// Ecouteur de redirection qui suit le domaine du panneau `panel`.
#[allow(dead_code)]
pub async fn serve_redirect_with(
    https_port: u16,
    panel: control_plane::domain::domains::panel::PanelSwitch,
) -> (SocketAddr, Handle<SocketAddr>) {
    let handle = Handle::new();
    let server = axum_server::bind("127.0.0.1:0".parse().unwrap())
        .handle(handle.clone())
        .serve(control_plane::http::redirect::router(https_port, panel).into_make_service());
    tokio::spawn(server);
    let addr = handle.listening().await.expect("adresse d'ecoute");
    (addr, handle)
}

fn parse(raw: &[u8]) -> Resp {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("fin des en-tetes");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    Resp {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

fn request(path: &str, host: &str) -> String {
    format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")
}

/// GET en clair (ecouteur de redirection), avec l'en-tete Host fourni.
pub async fn plain_get(addr: SocketAddr, host: &str, path: &str) -> Resp {
    let mut tcp = TcpStream::connect(addr).await.unwrap();
    tcp.write_all(request(path, host).as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    tcp.read_to_end(&mut raw).await.unwrap();
    parse(&raw)
}

/// GET HTTPS reel, en ne faisant confiance qu'au certificat de test.
pub async fn tls_get(addr: SocketAddr, cert: &Cert, path: &str) -> Resp {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.cert_der.clone()).unwrap();
    let client = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client));
    let tcp = TcpStream::connect(addr).await.unwrap();
    let mut tls = connector
        .connect(ServerName::try_from("localhost").unwrap(), tcp)
        .await
        .expect("poignee de main TLS");
    tls.write_all(request(path, "localhost").as_bytes())
        .await
        .unwrap();
    let mut raw = Vec::new();
    // Le serveur coupe parfois sans close_notify : la fin brutale apres les donnees est acceptee.
    let _ = tls.read_to_end(&mut raw).await;
    parse(&raw)
}
