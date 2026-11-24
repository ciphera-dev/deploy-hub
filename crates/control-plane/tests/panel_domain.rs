//! Domaine du panneau : `GET|PUT|DELETE /api/settings/panel-domain` et
//! `POST /api/settings/panel-domain/confirm` (proprietaire seulement). DNS
//! verifie, certificat emis par la meme tache `AcmeIssue` que les domaines
//! des projets, route `panel` du proxy (443 -> ecoute locale du panneau).
//! L'acces 8443 auto-signe reste ouvert jusqu'a la confirmation depuis le
//! nouveau domaine, puis se ferme ; l'etat survit a un redemarrage.

mod common;

use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::domain::domains::{self, DnsResolver, panel::PanelSwitch};
use control_plane::jobs::{self, JobKind};
use control_plane::state::{AppState, Clock};
use proto::{AgentToControl, CertificateOutcome, ControlToAgent, Route, TlsMode};
use serde_json::{Value, json};
use shared::config::Config;
use shared::ids::ServerId;
use tokio::sync::mpsc;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const NODE_IP: &str = "203.0.113.7";
const HOST: &str = "panel.example.org";
const PATH: &str = "/api/settings/panel-domain";

type Session = (String, String);

#[derive(Default)]
struct FakeDns {
    answers: Mutex<Vec<(String, Vec<IpAddr>)>>,
    /// Reponses ponctuelles, servies avant les reponses fixes.
    once: Mutex<Vec<(String, Vec<IpAddr>)>>,
}

impl FakeDns {
    fn set(&self, host: &str, ips: &[&str]) {
        let mut answers = self.answers.lock().unwrap();
        answers.retain(|(h, _)| h != host);
        answers.push((
            host.into(),
            ips.iter().map(|i| i.parse().unwrap()).collect(),
        ));
    }
}

impl DnsResolver for FakeDns {
    fn resolve<'a>(
        &'a self,
        host: &'a str,
    ) -> Pin<Box<dyn Future<Output = io::Result<Vec<IpAddr>>> + Send + 'a>> {
        let queued = {
            let mut once = self.once.lock().unwrap();
            once.iter()
                .position(|(h, _)| h == host)
                .map(|i| once.remove(i).1)
        };
        let found = queued.unwrap_or_else(|| {
            self.answers
                .lock()
                .unwrap()
                .iter()
                .find(|(h, _)| h == host)
                .map(|(_, ips)| ips.clone())
                .unwrap_or_default()
        });
        Box::pin(async move { Ok(found) })
    }
}

struct Env {
    state: AppState,
    _pg: common::TestDb,
    dns: Arc<FakeDns>,
    server: Uuid,
    agent: mpsc::Receiver<ControlToAgent>,
}

impl Env {
    async fn new() -> Self {
        Self::with_github(None).await
    }

    /// `github` : API GitHub simulee (`CIPHERA_GITHUB_API_URL`).
    async fn with_github(github: Option<String>) -> Self {
        let (mut state, pg) = common::state().await;
        state.cfg = Arc::new(
            Config::from_lookup(|name| match name {
                "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".into()),
                "CIPHERA_PUBLIC_URL" => Some(format!("https://{NODE_IP}:8443")),
                "CIPHERA_PUBLIC_IP" => Some(NODE_IP.into()),
                "CIPHERA_GITHUB_API_URL" => github.clone(),
                _ => None,
            })
            .unwrap(),
        );
        let dns = Arc::new(FakeDns::default());
        state.dns = dns.clone();
        let server = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO servers (id, name, kind, cert_fingerprint) VALUES ($1, 'local', 'local', 'AA:BB')",
        )
        .bind(server)
        .execute(&state.db)
        .await
        .unwrap();
        let agent = state.agents.attach_channel(ServerId(server));
        Self {
            state,
            _pg: pg,
            dns,
            server,
            agent,
        }
    }
}

async fn login(state: &AppState, role: &str) -> Session {
    let email = format!("{role}-{}@example.com", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name) VALUES ($1, $2, $3, $4, 'Test')",
    )
    .bind(Uuid::new_v4())
    .bind(&email)
    .bind(password::hash(PASSWORD).unwrap())
    .bind(role)
    .execute(&state.db)
    .await
    .unwrap();
    let r = call(
        state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": email, "password": PASSWORD })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    (r.token(), r.csrf())
}

async fn send(
    state: &AppState,
    s: &Session,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: Some(&s.0),
            csrf: Some(&s.1),
            body,
            ..Req::default()
        },
    )
    .await
}

/// Requete arrivee par le proxy du noeud (pair en boucle locale) sous
/// l'hote `host`.
async fn via_proxy(state: &AppState, s: &Session, method: &str, path: &str, host: &str) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: Some(&s.0),
            csrf: Some(&s.1),
            peer: Some("127.0.0.1:40000"),
            host: Some(host),
            headers: vec![("x-forwarded-proto", "https".into())],
            ..Req::default()
        },
    )
    .await
}

/// Derniere table des routes en attente pour l'agent (les autres ordres
/// sont ignores).
fn last_routes(env: &mut Env) -> Option<Vec<Route>> {
    let mut last = None;
    while let Ok(msg) = env.agent.try_recv() {
        if let ControlToAgent::ApplyRoutes(r) = msg {
            last = Some(r);
        }
    }
    last
}

fn unix(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

/// Prochain `ApplyRoutes` recu par l'agent, puis repond a la demande de
/// certificat suivante.
async fn issue(env: &mut Env) -> Vec<Route> {
    let job = jobs::claim(&env.state.db, &Clock::system(), "test-worker")
        .await
        .unwrap()
        .expect("tache acme_issue due");
    assert_eq!(job.kind, JobKind::AcmeIssue);
    let job_id = job.id;
    let state = env.state.clone();
    let handler = tokio::spawn(async move { domains::acme::issue(&state, job).await });
    let mut routes = Vec::new();
    let host = loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), env.agent.recv())
            .await
            .expect("ordre a l'agent")
            .unwrap();
        match msg {
            ControlToAgent::ApplyRoutes(r) => routes = r,
            ControlToAgent::IssueCertificate { host } => break host,
            _ => {}
        }
    };
    assert_eq!(host, HOST);
    env.state.agents.publish(
        ServerId(env.server),
        AgentToControl::Certificate {
            host,
            outcome: CertificateOutcome::Issued {
                not_after: unix(SystemTime::now()) + 90 * 86_400,
                issuer: "R11".into(),
            },
        },
    );
    handler.await.unwrap().expect("emission reussie");
    assert!(
        jobs::complete(&env.state.db, &Clock::system(), job_id, "test-worker")
            .await
            .unwrap()
    );
    routes
}

#[tokio::test]
async fn panel_domain_owner_only() {
    let env = Env::new().await;
    env.dns.set(HOST, &[NODE_IP]);
    for role in ["admin", "ops", "developer", "viewer"] {
        let s = login(&env.state, role).await;
        for (method, path, body) in [
            ("GET", PATH.to_string(), None),
            ("PUT", PATH.to_string(), Some(json!({ "domain": HOST }))),
            ("POST", format!("{PATH}/confirm"), None),
            ("DELETE", PATH.to_string(), None),
        ] {
            let r = send(&env.state, &s, method, &path, body).await;
            assert_eq!(r.status, 403, "{role} {method} {path} : {}", r.raw);
        }
    }
    let owner = login(&env.state, "owner").await;
    // Sans jeton CSRF : 403, rien n'est enregistre.
    let r = call(
        &env.state,
        "PUT",
        PATH,
        Req {
            cookie: Some(&owner.0),
            body: Some(json!({ "domain": HOST })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    let r = send(&env.state, &owner, "GET", PATH, None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["status"], "none");
    assert!(r.json()["domain"].is_null());
}

/// Corps limite a 16 Kio (comme les autres routeurs), pas aux 2 Mo par defaut
/// d'axum : 17 Kio sur l'ecriture du domaine -> 413, rien n'est enregistre.
#[tokio::test]
async fn panel_domain_body_over_16_kib_rejected() {
    let env = Env::new().await;
    env.dns.set(HOST, &[NODE_IP]);
    let owner = login(&env.state, "owner").await;
    let mut body = br#"{"domain":"panel.example.org"}"#.to_vec();
    body.resize(17 * 1024, b' ');
    let r = call(
        &env.state,
        "PUT",
        PATH,
        Req {
            cookie: Some(&owner.0),
            csrf: Some(&owner.1),
            bytes: Some(("application/json", body)),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 413, "{}", r.raw);
    let r = send(&env.state, &owner, "GET", PATH, None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["status"], "none");
}

#[tokio::test]
async fn panel_domain_switch_keeps_8443_until_confirmed() {
    let mut env = Env::new().await;
    let owner = login(&env.state, "owner").await;
    assert!(env.state.panel.listens_8443());

    // DNS faux : 422, rien n'est enregistre.
    let r = send(
        &env.state,
        &owner,
        "PUT",
        PATH,
        Some(json!({ "domain": HOST })),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    assert_eq!(r.code(), "dns_error");
    // Nom invalide ou zone joker : 422.
    for bad in ["1.2.3.4", "panel.203.0.113.7.sslip.io", "x"] {
        let r = send(
            &env.state,
            &owner,
            "PUT",
            PATH,
            Some(json!({ "domain": bad })),
        )
        .await;
        assert_eq!(r.status, 422, "{bad} : {}", r.raw);
    }

    // DNS correct : certificat demande (meme tache que les domaines).
    env.dns.set(HOST, &[NODE_IP]);
    let r = send(
        &env.state,
        &owner,
        "PUT",
        PATH,
        Some(json!({ "domain": HOST })),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(r.json()["domain"], HOST);
    assert_eq!(r.json()["status"], "issuing");
    assert!(env.state.panel.listens_8443());

    // L'hote est desormais reserve au panneau.
    let dev = login(&env.state, "developer").await;
    let p = send(
        &env.state,
        &dev,
        "POST",
        "/api/projects",
        Some(
            json!({ "name": "web", "source": { "type": "image", "image": "ghcr.io/acme/app:1" } }),
        ),
    )
    .await;
    let project = p.json()["id"].as_str().unwrap().to_string();
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains?project={project}"),
        Some(json!({ "host": HOST, "internal_port": 3000 })),
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "reserved_host");
    // ... et n'apparait dans aucun projet.
    let l = send(
        &env.state,
        &dev,
        "GET",
        &format!("/api/domains?project={project}"),
        None,
    )
    .await;
    assert!(!l.raw.contains(HOST), "{}", l.raw);

    // Avant le certificat : route `panel` sans amont (le proxy n'emet que
    // pour une route HTTPS ; le defi HTTP-01 passe avant la table), donc
    // aucun panneau en HTTP clair sur le port 80.
    let routes = issue(&mut env).await;
    let panel = routes
        .iter()
        .find(|r| r.host == HOST)
        .expect("route du panneau");
    assert_eq!(panel.path_prefix, "/");
    assert_eq!(panel.tls, TlsMode::Acme);
    assert!(panel.service.is_none() && panel.redirect_to.is_none());
    assert!(panel.upstreams.is_empty(), "{panel:?}");
    // Certificat actif : la route mene a l'ecoute locale du panneau.
    let routes = last_routes(&mut env).expect("table renvoyee apres le certificat");
    let panel = routes
        .iter()
        .find(|r| r.host == HOST)
        .expect("route du panneau");
    assert_eq!(
        panel.upstreams,
        vec![std::net::SocketAddr::from((
            [127, 0, 0, 1],
            env.state.cfg.panel_local_port
        ))]
    );

    let r = send(&env.state, &owner, "GET", PATH, None).await;
    assert_eq!(r.json()["status"], "awaiting_confirmation", "{}", r.raw);
    // Toujours joignable en 8443 tant que rien n'est confirme.
    assert!(env.state.panel.listens_8443());
    assert_eq!(
        env.state.public_url().as_deref(),
        Some("https://203.0.113.7:8443")
    );

    // Confirmation depuis l'ancien acces : refusee.
    let r = send(&env.state, &owner, "POST", &format!("{PATH}/confirm"), None).await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "confirm_from_domain");
    assert!(env.state.panel.listens_8443());

    // L'origine du nouveau domaine est deja admise (WebSocket des logs).
    let ws = call(
        &env.state,
        "GET",
        "/api/ws/logs",
        Req {
            cookie: Some(&owner.0),
            headers: vec![("origin", format!("https://{HOST}"))],
            ..Req::default()
        },
    )
    .await;
    assert_ne!(ws.code(), "origin", "{}", ws.raw);

    // Redemarrage avant confirmation : 8443 reste ouvert.
    let reloaded = PanelSwitch::load(&env.state.db).await.unwrap();
    assert!(reloaded.listens_8443());

    // Confirmation depuis le nouveau domaine : 8443 se ferme.
    let r = via_proxy(&env.state, &owner, "POST", &format!("{PATH}/confirm"), HOST).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["status"], "active");
    assert!(!env.state.panel.listens_8443());
    assert_eq!(
        env.state.public_url().as_deref(),
        Some("https://panel.example.org")
    );
    // Et l'etat survit a un redemarrage.
    let reloaded = PanelSwitch::load(&env.state.db).await.unwrap();
    assert!(!reloaded.listens_8443());

    // Un autre domaine ne remplace pas un domaine actif sans retrait.
    env.dns.set("other.example.org", &[NODE_IP]);
    let r = send(
        &env.state,
        &owner,
        "PUT",
        PATH,
        Some(json!({ "domain": "other.example.org" })),
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
}

/// Domaine du panneau enregistre, certificat emis et confirme depuis le
/// domaine.
async fn confirmed_panel(env: &mut Env, owner: &Session) {
    env.dns.set(HOST, &[NODE_IP]);
    let r = send(
        &env.state,
        owner,
        "PUT",
        PATH,
        Some(json!({ "domain": HOST })),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    issue(env).await;
    let r = via_proxy(&env.state, owner, "POST", &format!("{PATH}/confirm"), HOST).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    while env.agent.try_recv().is_ok() {}
}

/// Passage de la tache du domaine a l'heure `at` ; `reply` repond au
/// controle du certificat si le DNS est bon (`None` : verdict negatif, aucun
/// ordre attendu).
async fn run_check(env: &mut Env, at: SystemTime, reply: bool) {
    let job = jobs::claim(&env.state.db, &Clock::from_fn(move || at), "test-worker")
        .await
        .unwrap()
        .expect("tache acme_issue due");
    assert_eq!(job.kind, JobKind::AcmeIssue);
    let job_id = job.id;
    let state = env.state.clone();
    let handler = tokio::spawn(async move { domains::acme::issue(&state, job).await });
    if reply {
        let host = loop {
            let msg = tokio::time::timeout(Duration::from_secs(10), env.agent.recv())
                .await
                .expect("ordre a l'agent")
                .unwrap();
            if let ControlToAgent::CheckCertificate { host } = msg {
                break host;
            }
        };
        env.state.agents.publish(
            ServerId(env.server),
            AgentToControl::Certificate {
                host,
                outcome: CertificateOutcome::Issued {
                    not_after: unix(SystemTime::now()) + 90 * 86_400,
                    issuer: "R11".into(),
                },
            },
        );
    }
    handler.await.unwrap().expect("tache conclue");
    assert!(
        jobs::complete(&env.state.db, &Clock::system(), job_id, "test-worker")
            .await
            .unwrap()
    );
}

/// Domaine du panneau confirme : un verdict negatif (ou une panne) ne le met
/// jamais en erreur DNS ; l'erreur reste visible, la reprise est planifiee, et
/// « Revérifier » l'efface une fois le DNS corrige.
#[tokio::test]
async fn panel_confirmed_never_dns_error() {
    let mut env = Env::new().await;
    let owner = login(&env.state, "owner").await;
    confirmed_panel(&mut env, &owner).await;

    env.dns.set(HOST, &["198.51.100.9"]);
    let day = |n: u64| SystemTime::now() + Duration::from_secs(n * 86_400);
    for n in 1..=4 {
        run_check(&mut env, day(n * 2), false).await;
        let dns_status: String = sqlx::query_scalar("SELECT dns_status FROM domains WHERE panel")
            .fetch_one(&env.state.db)
            .await
            .unwrap();
        assert_eq!(dns_status, "ok", "verdict {n}");
        let r = send(&env.state, &owner, "GET", PATH, None).await;
        assert_eq!(r.json()["status"], "active", "{}", r.raw);
        assert!(
            r.json()["error"].as_str().unwrap().contains("198.51.100.9"),
            "{}",
            r.raw
        );
        let queued: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM jobs WHERE kind = 'acme_issue' AND status = 'queued'",
        )
        .fetch_one(&env.state.db)
        .await
        .unwrap();
        assert_eq!(queued, 1, "reprise planifiee");
    }
    assert!(!env.state.panel.listens_8443());
    let routes = last_routes(&mut env);
    assert!(routes.is_none_or(|r| r.iter().any(|x| x.host == HOST)));
    // Le proprietaire est prevenu : une notification de certificat en file.
    let notices: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'notify'")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert!(notices >= 1, "notification en file");

    // DNS corrige + « Revérifier » : tache due tout de suite, erreur effacee.
    env.dns.set(HOST, &[NODE_IP]);
    let r = send(&env.state, &owner, "POST", &format!("{PATH}/recheck"), None).await;
    assert_eq!(r.status, 202, "{}", r.raw);
    run_check(&mut env, SystemTime::now(), true).await;
    let r = send(&env.state, &owner, "GET", PATH, None).await;
    assert_eq!(r.json()["status"], "active", "{}", r.raw);
    assert!(r.json()["error"].is_null(), "{}", r.raw);
}

#[tokio::test]
async fn recheck_owner_only() {
    let mut env = Env::new().await;
    let recheck = format!("{PATH}/recheck");
    for role in ["admin", "ops", "developer", "viewer"] {
        let s = login(&env.state, role).await;
        let r = send(&env.state, &s, "POST", &recheck, None).await;
        assert_eq!(r.status, 403, "{role} : {}", r.raw);
    }
    let owner = login(&env.state, "owner").await;
    // Sans jeton CSRF : 403.
    let r = call(
        &env.state,
        "POST",
        &recheck,
        Req {
            cookie: Some(&owner.0),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    // Aucun domaine : 404.
    let r = send(&env.state, &owner, "POST", &recheck, None).await;
    assert_eq!(r.status, 404, "{}", r.raw);

    env.dns.set(HOST, &[NODE_IP]);
    let r = send(
        &env.state,
        &owner,
        "PUT",
        PATH,
        Some(json!({ "domain": HOST })),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    // DNS faux : 422, rien n'est modifie.
    env.dns.set(HOST, &["198.51.100.9"]);
    let r = send(&env.state, &owner, "POST", &recheck, None).await;
    assert_eq!(r.status, 422, "{}", r.raw);
    assert_eq!(r.code(), "dns_error");
    // DNS correct : replanifie, une seule tache due maintenant.
    env.dns.set(HOST, &[NODE_IP]);
    let r = send(&env.state, &owner, "POST", &recheck, None).await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(r.json()["domain"], HOST);
    let due: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE kind = 'acme_issue' AND status = 'queued' AND run_at <= now()",
    )
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert_eq!(due, 1);
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE action = 'panel_domain.recheck'",
    )
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert_eq!(audit, 1);
    while env.agent.try_recv().is_ok() {}
}

#[tokio::test]
async fn panel_domain_can_be_cancelled_before_confirmation() {
    let mut env = Env::new().await;
    let owner = login(&env.state, "owner").await;
    env.dns.set(HOST, &[NODE_IP]);
    let r = send(
        &env.state,
        &owner,
        "PUT",
        PATH,
        Some(json!({ "domain": HOST })),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    issue(&mut env).await;
    while env.agent.try_recv().is_ok() {}

    // Annuler (depuis 8443) : route et certificat retires, 8443 reste.
    let r = send(&env.state, &owner, "DELETE", PATH, None).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    let mut removed = false;
    let mut last_routes = None;
    while let Ok(msg) = env.agent.try_recv() {
        match msg {
            ControlToAgent::RemoveCertificate { host } => removed |= host == HOST,
            ControlToAgent::ApplyRoutes(r) => last_routes = Some(r),
            _ => {}
        }
    }
    assert!(removed, "certificat retire");
    assert!(
        last_routes
            .expect("table renvoyee")
            .iter()
            .all(|r| r.host != HOST)
    );
    assert!(env.state.panel.listens_8443());
    let r = send(&env.state, &owner, "GET", PATH, None).await;
    assert_eq!(r.json()["status"], "none");
    let audit: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action LIKE 'panel_domain.%'")
            .fetch_one(&env.state.db)
            .await
            .unwrap();
    assert_eq!(audit, 2);
}

/// Confirmation telle que le panneau la recoit : pair TCP `peer`, en-tete
/// `Host` et en-tetes supplementaires (ceux que le client aurait pu poser).
async fn confirm_as(
    state: &AppState,
    s: &Session,
    peer: &str,
    host: &str,
    headers: Vec<(&str, String)>,
    body: Option<Value>,
) -> Reply {
    call(
        state,
        "POST",
        &format!("{PATH}/confirm"),
        Req {
            cookie: Some(&s.0),
            csrf: Some(&s.1),
            peer: Some(peer),
            host: Some(host),
            headers,
            body,
            ..Req::default()
        },
    )
    .await
}

/// Domaine enregistre et certificat actif : en attente de confirmation.
async fn awaiting(env: &mut Env, owner: &Session) {
    env.dns.set(HOST, &[NODE_IP]);
    let r = send(
        &env.state,
        owner,
        "PUT",
        PATH,
        Some(json!({ "domain": HOST })),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    issue(env).await;
    while env.agent.try_recv().is_ok() {}
    let r = send(&env.state, owner, "GET", PATH, None).await;
    assert_eq!(r.json()["status"], "awaiting_confirmation", "{}", r.raw);
}

#[tokio::test]
async fn panel_domain_confirm_only_through_the_proxy_on_the_panel_host() {
    let mut env = Env::new().await;
    let owner = login(&env.state, "owner").await;
    awaiting(&mut env, &owner).await;
    let https = || ("x-forwarded-proto", "https".to_string());
    // (pair TCP, Host, en-tetes, cas)
    type Case<'a> = (&'a str, &'a str, Vec<(&'a str, String)>, &'a str);
    let refused: Vec<Case> = vec![
        // Pair hors boucle locale, meme avec tout le reste correct.
        ("203.0.113.50:40000", HOST, vec![https()], "pair distant"),
        (
            "[2001:db8::5]:40000",
            HOST,
            vec![https()],
            "pair distant IPv6",
        ),
        // Boucle locale sans X-Forwarded-Proto, ou en HTTP clair.
        ("127.0.0.1:40000", HOST, vec![], "sans X-Forwarded-Proto"),
        (
            "127.0.0.1:40000",
            HOST,
            vec![("x-forwarded-proto", "http".into())],
            "X-Forwarded-Proto http",
        ),
        (
            "127.0.0.1:40000",
            HOST,
            vec![("x-forwarded-proto", "HTTPS, http".into())],
            "X-Forwarded-Proto douteux",
        ),
        // Boucle locale en HTTPS, mais pas sous l'hote du panneau.
        ("127.0.0.1:40000", NODE_IP, vec![https()], "hote IP"),
        (
            "127.0.0.1:40000",
            "other.example.org",
            vec![https()],
            "autre hote",
        ),
        (
            "127.0.0.1:40000",
            "panel.example.org.evil.test",
            vec![https()],
            "hote prolonge",
        ),
        (
            "127.0.0.1:40000",
            NODE_IP,
            vec![https(), ("x-forwarded-host", HOST.into())],
            "X-Forwarded-Host ignore",
        ),
        // En-tetes X-Forwarded-* poses par le client lui-meme (acces direct
        // en 8443) : jamais pris pour le proxy.
        (
            "203.0.113.50:40000",
            HOST,
            vec![
                https(),
                ("x-forwarded-for", "127.0.0.1".into()),
                ("x-forwarded-host", HOST.into()),
                ("x-real-ip", "127.0.0.1".into()),
                ("forwarded", "for=127.0.0.1;proto=https".into()),
            ],
            "X-Forwarded-* du client",
        ),
    ];
    for (peer, host, headers, why) in refused {
        let r = confirm_as(&env.state, &owner, peer, host, headers, None).await;
        assert_eq!(r.status, 409, "{why} : {}", r.raw);
        assert_eq!(r.code(), "confirm_from_domain", "{why}");
        assert!(env.state.panel.listens_8443(), "{why}");
    }
    // Le proxy (boucle locale, HTTPS, hote du panneau, port compris).
    let r = confirm_as(
        &env.state,
        &owner,
        "[::1]:40000",
        "PANEL.example.org:443",
        vec![https()],
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(!env.state.panel.listens_8443());
}

#[tokio::test]
async fn panel_domain_switch_stuck_in_issuing_expires_back_to_8443() {
    let mut env = Env::new().await;
    let owner = login(&env.state, "owner").await;
    let start = SystemTime::now();
    env.state.clock = Clock::from_fn(move || start);
    env.dns.set(HOST, &[NODE_IP]);
    let r = send(
        &env.state,
        &owner,
        "PUT",
        PATH,
        Some(json!({ "domain": HOST })),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    while env.agent.try_recv().is_ok() {}

    // Toujours en cours juste avant l'echeance.
    env.state.clock = Clock::from_fn(move || start + Duration::from_secs(23 * 3600));
    let r = send(&env.state, &owner, "GET", PATH, None).await;
    assert_eq!(r.json()["status"], "issuing", "{}", r.raw);
    assert!(last_routes(&mut env).is_none());

    // Echeance passee sans certificat : erreur visible, route retiree,
    // emission arretee, 8443 seul acces.
    env.state.clock = Clock::from_fn(move || start + Duration::from_secs(24 * 3600 + 60));
    let r = send(&env.state, &owner, "GET", PATH, None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["status"], "error", "{}", r.raw);
    assert_eq!(r.json()["domain"], HOST);
    let error = r.json()["error"].as_str().unwrap_or_default().to_string();
    assert!(error.contains("8443"), "{error}");
    let routes = last_routes(&mut env).expect("table renvoyee");
    assert!(routes.iter().all(|r| r.host != HOST), "{routes:?}");
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE dedupe_key LIKE 'acme:%' AND status IN ('queued', 'running')",
    )
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert_eq!(queued, 0);
    assert!(env.state.panel.listens_8443());
    // La table recalculee (reconnexion d'un agent) n'a plus la route.
    let mut conn = env.state.db.acquire().await.unwrap();
    let table = domains::route_table(&mut conn, &env.state.cfg, env.server)
        .await
        .unwrap();
    assert!(table.iter().all(|r| r.host != HOST));
    drop(conn);
    // Confirmer est impossible ; enregistrer a nouveau relance la bascule.
    let r = via_proxy(&env.state, &owner, "POST", &format!("{PATH}/confirm"), HOST).await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "not_ready");
    let r = send(
        &env.state,
        &owner,
        "PUT",
        PATH,
        Some(json!({ "domain": HOST })),
    )
    .await;
    assert_eq!(r.status, 202, "{}", r.raw);
    assert_eq!(r.json()["status"], "issuing", "{}", r.raw);
}

/// Cle RSA 2048 jetable (PKCS#1 PEM) de la GitHub App simulee.
fn test_key() -> String {
    use base64::Engine;
    use rsa::pkcs1::EncodeRsaPrivateKey;
    let mut rng = rand::rand_core::UnwrapErr(rand::rngs::SysRng);
    let key = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let der = key.to_pkcs1_der().unwrap();
    let body = base64::engine::general_purpose::STANDARD.encode(der.as_bytes());
    // En-tete PEM factice coupe par concat! : aucune ligne de la source n'imite une vraie cle (scanner de secrets).
    let mut pem = String::from(concat!("-----BEGIN RSA ", "PRIVATE KEY-----\n"));
    for line in body.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(line).unwrap());
        pem.push('\n');
    }
    pem.push_str("-----END RSA PRIVATE KEY-----\n");
    pem
}

#[tokio::test]
async fn panel_domain_confirm_moves_the_github_app_to_the_new_origin() {
    use wiremock::matchers::{body_json, header_regex, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let github = MockServer::start().await;
    let mut env = Env::with_github(Some(github.uri())).await;
    sqlx::query(
        "INSERT INTO github_app (app_id, slug, name, owner_login, html_url, \
         private_key_sealed, webhook_secret_sealed) \
         VALUES (123, 'ciphera-ab12cd', 'Ciphera ab12cd', 'acme', \
         'https://github.com/apps/ciphera-ab12cd', $1, $2)",
    )
    .bind(
        env.state
            .secrets
            .seal_bound(
                control_plane::secrets::labels::GITHUB_PRIVATE_KEY,
                test_key().as_bytes(),
            )
            .unwrap(),
    )
    .bind(
        env.state
            .secrets
            .seal_bound(
                control_plane::secrets::labels::GITHUB_WEBHOOK_SECRET,
                b"whsec-test-value",
            )
            .unwrap(),
    )
    .execute(&env.state.db)
    .await
    .unwrap();
    let owner = login(&env.state, "owner").await;
    awaiting(&mut env, &owner).await;
    let hook = format!("https://{HOST}/v1/webhooks/github");
    let via = |body| {
        confirm_as(
            &env.state,
            &owner,
            "127.0.0.1:40000",
            HOST,
            vec![("x-forwarded-proto", "https".into())],
            body,
        )
    };

    // L'URL de configuration (Setup URL) de l'App ne se change pas par
    // l'API : confirmation bloquee avec la consigne, aucun appel a GitHub.
    let r = via(None).await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "github_setup_url");
    let message = r.json()["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        message.contains(&format!("https://{HOST}/connectors")),
        "{message}"
    );
    assert!(message.contains("github_setup_url_updated"), "{message}");
    assert!(env.state.panel.listens_8443());
    assert!(github.received_requests().await.unwrap().is_empty());

    // GitHub refuse la mise a jour du webhook : rien n'est confirme.
    Mock::given(method("PATCH"))
        .and(path("/app/hook/config"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&github)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/app/hook/config"))
        .and(header_regex(
            "authorization",
            r"^Bearer [\w-]+\.[\w-]+\.[\w-]+$",
        ))
        .and(body_json(json!({ "url": hook })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "url": hook, "content_type": "json", "insecure_ssl": "0" })),
        )
        .with_priority(2)
        .mount(&github)
        .await;
    let ack = Some(json!({ "github_setup_url_updated": true }));
    let r = via(ack.clone()).await;
    assert_eq!(r.status, 502, "{}", r.raw);
    assert_eq!(r.code(), "github_hook_not_updated");
    assert!(env.state.panel.listens_8443());
    let r = send(&env.state, &owner, "GET", PATH, None).await;
    assert_eq!(r.json()["status"], "awaiting_confirmation", "{}", r.raw);

    // Webhook deplace vers le nouveau domaine, puis 8443 ferme.
    let r = via(ack).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["status"], "active");
    assert!(!env.state.panel.listens_8443());
    let patched = github
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|q| q.method.as_str() == "PATCH")
        .count();
    assert_eq!(patched, 2);
    // Le manifest d'une future App suit aussi la nouvelle origine.
    assert_eq!(
        env.state.public_url().as_deref(),
        Some("https://panel.example.org")
    );
}

/// GitHub App simulee enregistree dans l'instance.
async fn github_app(env: &Env) {
    sqlx::query(
        "INSERT INTO github_app (app_id, slug, name, owner_login, html_url, \
         private_key_sealed, webhook_secret_sealed) \
         VALUES (123, 'ciphera-ab12cd', 'Ciphera ab12cd', 'acme', \
         'https://github.com/apps/ciphera-ab12cd', $1, $2)",
    )
    .bind(
        env.state
            .secrets
            .seal_bound(
                control_plane::secrets::labels::GITHUB_PRIVATE_KEY,
                test_key().as_bytes(),
            )
            .unwrap(),
    )
    .bind(
        env.state
            .secrets
            .seal_bound(
                control_plane::secrets::labels::GITHUB_WEBHOOK_SECRET,
                b"whsec-test-value",
            )
            .unwrap(),
    )
    .execute(&env.state.db)
    .await
    .unwrap();
}

/// Domaine confirme avec une GitHub App : webhook sur le domaine.
async fn confirmed_with_github(env: &mut Env, github: &wiremock::MockServer) -> Session {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    github_app(env).await;
    let owner = login(&env.state, "owner").await;
    awaiting(env, &owner).await;
    let hook = format!("https://{HOST}/v1/webhooks/github");
    Mock::given(method("PATCH"))
        .and(path("/app/hook/config"))
        .and(wiremock::matchers::body_json(json!({ "url": hook })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "url": hook })))
        .mount(github)
        .await;
    let r = confirm_as(
        &env.state,
        &owner,
        "127.0.0.1:40000",
        HOST,
        vec![("x-forwarded-proto", "https".into())],
        Some(json!({ "github_setup_url_updated": true })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(!env.state.panel.listens_8443());
    owner
}

/// URL des `PATCH /app/hook/config` recus par GitHub.
async fn patched_urls(github: &wiremock::MockServer) -> Vec<String> {
    github
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|q| q.method.as_str() == "PATCH")
        .map(|q| {
            serde_json::from_slice::<Value>(&q.body).unwrap()["url"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn panel_domain_delete_moves_the_github_app_back_to_8443() {
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let github = MockServer::start().await;
    let mut env = Env::with_github(Some(github.uri())).await;
    let owner = confirmed_with_github(&mut env, &github).await;
    let back = format!("https://{NODE_IP}:8443/v1/webhooks/github");

    // GitHub en panne : rien n'est retire (sinon les livraisons echoueraient
    // sans bruit sur un domaine qui ne sert plus le panneau).
    Mock::given(method("PATCH"))
        .and(path("/app/hook/config"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&github)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/app/hook/config"))
        .and(body_json(json!({ "url": back })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "url": back })))
        .with_priority(2)
        .mount(&github)
        .await;
    let r = send(&env.state, &owner, "DELETE", PATH, None).await;
    assert_eq!(r.status, 502, "{}", r.raw);
    assert_eq!(r.code(), "github_hook_not_updated");
    assert!(!env.state.panel.listens_8443());
    let r = send(&env.state, &owner, "GET", PATH, None).await;
    assert_eq!(r.json()["status"], "active", "{}", r.raw);

    // Webhook ramene sur l'origine 8443, puis domaine retire.
    let r = send(&env.state, &owner, "DELETE", PATH, None).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert!(env.state.panel.listens_8443());
    let urls = patched_urls(&github).await;
    assert_eq!(urls.last(), Some(&back), "{urls:?}");
}

#[tokio::test]
async fn reset_panel_domain_cli_moves_the_github_app_back_to_8443() {
    use control_plane::cli::admin;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let github = MockServer::start().await;
    let mut env = Env::with_github(Some(github.uri())).await;
    confirmed_with_github(&mut env, &github).await;
    let back = format!("https://{NODE_IP}:8443/v1/webhooks/github");
    Mock::given(method("PATCH"))
        .and(path("/app/hook/config"))
        .and(body_json(json!({ "url": back })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "url": back })))
        .with_priority(1)
        .mount(&github)
        .await;
    let reset = admin::reset_panel_domain(
        &env.state.db,
        &env.state.cfg,
        Some(&env.state.secrets),
        Some(&env.state.github),
    )
    .await
    .unwrap();
    assert_eq!(reset.removed.as_deref(), Some(HOST));
    assert_eq!(reset.github, admin::WebhookMove::Moved(back.clone()));
    let urls = patched_urls(&github).await;
    assert_eq!(urls.last(), Some(&back), "{urls:?}");
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM domains WHERE panel")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn reset_panel_domain_cli_still_resets_without_key_or_github_client() {
    use control_plane::cli::admin;

    let github = wiremock::MockServer::start().await;
    let mut env = Env::with_github(Some(github.uri())).await;
    confirmed_with_github(&mut env, &github).await;
    let before = patched_urls(&github).await.len();
    let back = format!("https://{NODE_IP}:8443/v1/webhooks/github");

    // Ni age.key ni client GitHub : domaine retire, webhook signale a
    // deplacer a la main (aucun appel a GitHub).
    let reset = admin::reset_panel_domain(&env.state.db, &env.state.cfg, None, None)
        .await
        .unwrap();
    assert_eq!(reset.removed.as_deref(), Some(HOST));
    assert_eq!(reset.github, admin::WebhookMove::Failed(Some(back)));
    assert_eq!(patched_urls(&github).await.len(), before);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM domains WHERE panel")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(left, 0);
}
