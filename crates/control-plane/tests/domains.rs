//! Domaines : validation des noms, hotes uniques sur l'instance et reserves
//! au panneau, verification DNS (resolveur injecte), domaine principal,
//! redirection 308, port interne requis, et emission ACME (tache
//! `AcmeIssue`) avec renouvellement planifie a J-30.

mod common;

use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::domain::domains::{self, DnsResolver};
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

type Session = (String, String);

/// Resolveur de test : reponses fixees par nom.
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

impl FakeDns {
    /// Prochaine reponse pour `host` seulement (puis retour aux reponses fixes).
    fn once(&self, host: &str, ips: &[&str]) {
        self.once.lock().unwrap().push((
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
        Self::with(&[]).await
    }

    /// Configuration de base, completee ou remplacee par `extra`.
    async fn with(extra: &'static [(&'static str, &'static str)]) -> Self {
        let (mut state, pg) = common::state().await;
        state.cfg = Arc::new(
            Config::from_lookup(|name| {
                if let Some((_, v)) = extra.iter().find(|(k, _)| *k == name) {
                    return Some((*v).into());
                }
                match name {
                    "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".into()),
                    "CIPHERA_PUBLIC_URL" => Some("https://panel.example.com:8443".into()),
                    "CIPHERA_PANEL_ALIASES" => Some("admin.example.com".into()),
                    "CIPHERA_PUBLIC_IP" => Some(NODE_IP.into()),
                    _ => None,
                }
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

    /// Prochain `ApplyRoutes` envoye a l'agent (les autres ordres sont
    /// ignores).
    async fn next_routes(&mut self) -> Vec<Route> {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(10), self.agent.recv())
                .await
                .expect("table envoyee a l'agent")
                .expect("agent attache");
            if let ControlToAgent::ApplyRoutes(routes) = msg {
                return routes;
            }
        }
    }

    /// Derniere table envoyee (vide la file).
    async fn last_routes(&mut self) -> Vec<Route> {
        let mut last = self.next_routes().await;
        while let Ok(msg) = self.agent.try_recv() {
            if let ControlToAgent::ApplyRoutes(routes) = msg {
                last = routes;
            }
        }
        last
    }
}

async fn login(state: &AppState, role: &str) -> Session {
    let email = format!("{role}-{}@example.com", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, $4, 'Test', now())",
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

/// Projet dont les taches `acme_issue` du domaine genere sont mises de cote :
/// les tests des domaines personnalises prennent ainsi la leur (la file est
/// partagee). Les tests du domaine genere utilisent [`project_raw`].
async fn project(state: &AppState, s: &Session, name: &str) -> Uuid {
    let id = project_raw(state, s, name).await;
    sqlx::query(
        "UPDATE jobs SET status = 'done' WHERE kind = 'acme_issue' AND status = 'queued' \
         AND payload->>'domain_id' IN \
           (SELECT id::text FROM domains WHERE generated AND project_id = $1)",
    )
    .bind(id)
    .execute(&state.db)
    .await
    .unwrap();
    id
}

/// Projet tel que cree, avec la tache d'emission de son domaine genere.
async fn project_raw(state: &AppState, s: &Session, name: &str) -> Uuid {
    let r = send(
        state,
        s,
        "POST",
        "/api/projects",
        Some(json!({ "name": name, "source": { "type": "image", "image": "ghcr.io/acme/app:1" } })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    Uuid::parse_str(r.json()["id"].as_str().unwrap()).unwrap()
}

async fn add(state: &AppState, s: &Session, project: Uuid, body: Value) -> Reply {
    send(
        state,
        s,
        "POST",
        &format!("/api/domains?project={project}"),
        Some(body),
    )
    .await
}

async fn add_ok(state: &AppState, s: &Session, project: Uuid, host: &str) -> String {
    let r = add(
        state,
        s,
        project,
        json!({ "host": host, "internal_port": 3000 }),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    r.json()["id"].as_str().unwrap().to_string()
}

async fn list(state: &AppState, s: &Session, project: Uuid) -> Value {
    let r = send(
        state,
        s,
        "GET",
        &format!("/api/domains?project={project}"),
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    r.json()
}

fn domain<'a>(list: &'a Value, host: &str) -> &'a Value {
    list["domains"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["host"] == host)
        .unwrap_or_else(|| panic!("{host} absent de {list}"))
}

fn unix(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

#[tokio::test]
async fn domain_rejects_invalid_fqdn_and_ip_literals() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "fqdn").await;
    for host in [
        "",
        "localhost",
        "203.0.113.9",
        "[2001:db8::1]",
        "2001:db8::1",
        "app..example.com",
        "-app.example.com",
        "app-.example.com",
        "app_1.example.com",
        "*.example.com",
        "exa mple.com",
        "app.example.com:8080",
        "https://app.example.com",
        "app.example.com/path",
        "app.123",
        &format!("{}.example.com", "a".repeat(64)),
        &format!("{}com", "abcdefghi.".repeat(26)),
        "ex\u{e9}mple.com",
    ] {
        let r = add(
            &env.state,
            &dev,
            p,
            json!({ "host": host, "internal_port": 3000 }),
        )
        .await;
        assert_eq!(r.status, 422, "{host:?} : {}", r.raw);
    }
    // Normalisation : minuscules, point final retire, punycode accepte.
    let r = add(
        &env.state,
        &dev,
        p,
        json!({ "host": "App.Example.COM.", "internal_port": 3000 }),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    assert_eq!(r.json()["host"], "app.example.com");
    let r = add(
        &env.state,
        &dev,
        p,
        json!({ "host": "xn--bcher-kva.example", "internal_port": 3000 }),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
}

#[tokio::test]
async fn host_already_used_by_other_project_409() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let a = project(&env.state, &dev, "alpha").await;
    let b = project(&env.state, &dev, "beta").await;
    add_ok(&env.state, &dev, a, "shop.example.com").await;
    for host in ["shop.example.com", "SHOP.example.com"] {
        let r = add(
            &env.state,
            &dev,
            b,
            json!({ "host": host, "internal_port": 3000 }),
        )
        .await;
        assert_eq!(r.status, 409, "{}", r.raw);
        assert_eq!(r.code(), "host_taken");
    }
    // Rien n'a ete cree dans le projet b.
    let l = list(&env.state, &dev, b).await;
    assert!(
        l["domains"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["host"] != "shop.example.com")
    );
}

#[tokio::test]
async fn panel_domain_reserved_409() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "panel-grab").await;
    for host in ["panel.example.com", "Admin.Example.com"] {
        let r = add(
            &env.state,
            &dev,
            p,
            json!({ "host": host, "internal_port": 3000 }),
        )
        .await;
        assert_eq!(r.status, 409, "{}", r.raw);
        assert_eq!(r.code(), "reserved_host");
    }
}

/// Appel sous l'hote `host` (en-tete `Host` de la requete).
async fn send_as_host(
    state: &AppState,
    s: &Session,
    host: &str,
    path: &str,
    body: Option<Value>,
) -> Reply {
    call(
        state,
        "POST",
        path,
        Req {
            cookie: Some(&s.0),
            csrf: Some(&s.1),
            host: Some(host),
            body,
            ..Req::default()
        },
    )
    .await
}

#[tokio::test]
async fn domain_create_refuses_current_host() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "by-dns").await;
    let path = format!("/api/domains?project={p}");
    // Le nom par lequel le panneau est joint (8443) ne devient pas un
    // domaine de projet, y compris par l'option www.
    for (host, body) in [
        (
            "panel-by-dns.example.org:8443",
            json!({ "host": "Panel-By-Dns.example.org", "internal_port": 3000 }),
        ),
        (
            "www.by-dns.example.org",
            json!({ "host": "by-dns.example.org", "www": true, "internal_port": 3000 }),
        ),
    ] {
        let r = send_as_host(&env.state, &dev, host, &path, Some(body)).await;
        assert_eq!(r.status, 409, "{host} : {}", r.raw);
        assert_eq!(r.code(), "reserved_host");
    }
    // Depuis l'IP du noeud : accepte.
    let r = send_as_host(
        &env.state,
        &dev,
        "203.0.113.7:8443",
        &path,
        Some(json!({ "host": "shop.by-dns.example.org", "internal_port": 3000 })),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let id = r.json()["id"].as_str().unwrap().to_string();
    env.dns.set("shop.by-dns.example.org", &[NODE_IP]);
    // La verification refuse aussi l'hote de la requete...
    let verify_path = format!("/api/domains/{id}/verify");
    let r = send_as_host(
        &env.state,
        &dev,
        "shop.by-dns.example.org:8443",
        &verify_path,
        None,
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "reserved_host");
    // ... et un nom reserve au panneau apres la creation du domaine.
    let mut reserved = env.state.clone();
    reserved.cfg = Arc::new(
        Config::from_lookup(|name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".into()),
            "CIPHERA_PUBLIC_IP" => Some(NODE_IP.into()),
            "CIPHERA_PANEL_ALIASES" => Some("shop.by-dns.example.org".into()),
            _ => None,
        })
        .unwrap(),
    );
    let r = send_as_host(&reserved, &dev, "203.0.113.7:8443", &verify_path, None).await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "reserved_host");
    let status: String = sqlx::query_scalar("SELECT dns_status FROM domains WHERE id = $1")
        .bind(Uuid::parse_str(&id).unwrap())
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(status, "pending", "rien d'enregistre");
    // Sous un autre hote : verification normale.
    let r = send_as_host(&env.state, &dev, "203.0.113.7:8443", &verify_path, None).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["dns_status"], "ok");
    drain(&mut env);
    // En ligne, puis declare plus tard comme nom du panneau : plus route vers
    // le projet.
    let mut conn = env.state.db.acquire().await.unwrap();
    let routes = domains::route_table(&mut conn, &env.state.cfg, env.server)
        .await
        .unwrap();
    assert!(has_route(&routes, "shop.by-dns.example.org"));
    let routes = domains::route_table(&mut conn, &reserved.cfg, env.server)
        .await
        .unwrap();
    assert!(!has_route(&routes, "shop.by-dns.example.org"));
}

#[tokio::test]
async fn internal_port_required_before_a_domain_goes_live_422() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "noport").await;
    let r = add(&env.state, &dev, p, json!({ "host": "noport.example.com" })).await;
    assert_eq!(r.status, 422, "{}", r.raw);
    assert!(r.raw.contains("port"), "{}", r.raw);
    // Port fourni : enregistre sur le service.
    let id = add_ok(&env.state, &dev, p, "noport.example.com").await;
    let port: Option<i32> = sqlx::query_scalar(
        "SELECT s.internal_port FROM domains d JOIN services s ON s.id = d.service_id WHERE d.id = $1::uuid",
    )
    .bind(&id)
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert_eq!(port, Some(3000));
    // Port retire ensuite (service sans port) : la verification refuse.
    sqlx::query("UPDATE services SET internal_port = NULL")
        .execute(&env.state.db)
        .await
        .unwrap();
    env.dns.set("noport.example.com", &[NODE_IP]);
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{id}/verify"),
        None,
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
}

#[tokio::test]
async fn verify_dns_reports_wrong_ip() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "dnscheck").await;
    let id = add_ok(&env.state, &dev, p, "www.dnscheck.example").await;

    env.dns.set("www.dnscheck.example", &["198.51.100.9"]);
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{id}/verify"),
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let d = r.json();
    assert_eq!(d["dns_status"], "error");
    let why = d["dns_error"].as_str().unwrap();
    assert!(
        why.contains("198.51.100.9") && why.contains(NODE_IP),
        "{why}"
    );
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE kind = 'acme_issue' AND status = 'queued'",
    )
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert_eq!(queued, 0, "aucun certificat sans DNS correct");

    // Aucun enregistrement : erreur explicite.
    env.dns.set("www.dnscheck.example", &[]);
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{id}/verify"),
        None,
    )
    .await;
    assert_eq!(r.json()["dns_status"], "error");

    // AAAA sans IPv6 du noeud : refuse (Let's Encrypt essaierait l'IPv6).
    env.dns
        .set("www.dnscheck.example", &["2001:db8::5", NODE_IP]);
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{id}/verify"),
        None,
    )
    .await;
    assert_eq!(r.json()["dns_status"], "error", "{}", r.raw);
    let why = r.json()["dns_error"].as_str().unwrap().to_string();
    assert!(
        why.contains("supprimez l'enregistrement AAAA ou configurez l'IPv6 du nœud"),
        "{why}"
    );

    // Correct : en ligne, route poussee, emission demandee.
    env.dns.set("www.dnscheck.example", &[NODE_IP]);
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{id}/verify"),
        None,
    )
    .await;
    assert_eq!(r.json()["dns_status"], "ok", "{}", r.raw);
    assert_eq!(r.json()["dns_error"], Value::Null);
    let routes = env.last_routes().await;
    let route = routes
        .iter()
        .find(|r| r.host == "www.dnscheck.example")
        .expect("route du domaine");
    assert_eq!(route.tls, TlsMode::Acme);
    assert_eq!(route.redirect_to, None);
    assert!(route.service.is_some());
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE kind = 'acme_issue' AND status = 'queued' AND payload->>'domain_id' = $1",
    )
    .bind(&id)
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert_eq!(queued, 1);
}

#[tokio::test]
async fn primary_domain_unique_per_env() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "prim").await;
    let a = add_ok(&env.state, &dev, p, "a.prim.example").await;
    let b = add_ok(&env.state, &dev, p, "b.prim.example").await;
    // Le premier domaine devient principal.
    let l = list(&env.state, &dev, p).await;
    assert_eq!(domain(&l, "a.prim.example")["primary"], true);
    assert_eq!(domain(&l, "b.prim.example")["primary"], false);

    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{b}/primary"),
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let l = r.json();
    assert_eq!(domain(&l, "a.prim.example")["primary"], false);
    assert_eq!(domain(&l, "b.prim.example")["primary"], true);
    let primaries: i64 = sqlx::query_scalar("SELECT count(*) FROM domains WHERE is_primary")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(primaries, 1);

    // La base refuse un second principal dans le meme environnement.
    let r = sqlx::query("UPDATE domains SET is_primary = true WHERE id = $1::uuid")
        .bind(&a)
        .execute(&env.state.db)
        .await;
    assert!(r.is_err(), "index unique du principal");

    // Un autre projet a son propre principal.
    let q = project(&env.state, &dev, "prim-two").await;
    add_ok(&env.state, &dev, q, "c.prim.example").await;
    let l = list(&env.state, &dev, q).await;
    assert_eq!(domain(&l, "c.prim.example")["primary"], true);
}

#[tokio::test]
async fn redirect_creates_308_route() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "redir").await;
    let main = add_ok(&env.state, &dev, p, "redir.example").await;
    // Apex + www : www.redir.example ajoute, redirige vers le principal.
    let l = list(&env.state, &dev, p).await;
    assert!(
        l["domains"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["host"] != "www.redir.example")
    );
    let r = add(
        &env.state,
        &dev,
        p,
        json!({ "host": "old.redir.example", "internal_port": 3000 }),
    )
    .await;
    let old = r.json()["id"].as_str().unwrap().to_string();
    for (id, host) in [(&main, "redir.example"), (&old, "old.redir.example")] {
        env.dns.set(host, &[NODE_IP]);
        let r = send(
            &env.state,
            &dev,
            "POST",
            &format!("/api/domains/{id}/verify"),
            None,
        )
        .await;
        assert_eq!(r.json()["dns_status"], "ok", "{}", r.raw);
    }

    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{old}/redirect"),
        Some(json!({ "enabled": true })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["redirect"], true);
    assert_eq!(r.json()["redirect_to"], "redir.example");
    let routes = env.last_routes().await;
    let route = routes
        .iter()
        .find(|r| r.host == "old.redir.example")
        .unwrap();
    assert_eq!(route.redirect_to.as_deref(), Some("https://redir.example"));
    assert_eq!(route.tls, TlsMode::Acme);
    let main_route = routes.iter().find(|r| r.host == "redir.example").unwrap();
    assert_eq!(main_route.redirect_to, None);

    // Le principal ne peut pas se rediriger vers lui-meme.
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{main}/redirect"),
        Some(json!({ "enabled": true })),
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);

    // Retrait : sert de nouveau son service.
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{old}/redirect"),
        Some(json!({ "enabled": false })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let routes = env.last_routes().await;
    let route = routes
        .iter()
        .find(|r| r.host == "old.redir.example")
        .unwrap();
    assert_eq!(route.redirect_to, None);

    // www a l'ajout d'un apex : redirection 308 vers le principal.
    let q = project(&env.state, &dev, "apex").await;
    let r = add(
        &env.state,
        &dev,
        q,
        json!({ "host": "apex.example", "internal_port": 3000, "www": true }),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let l = list(&env.state, &dev, q).await;
    assert_eq!(domain(&l, "www.apex.example")["redirect"], true);
    assert_eq!(
        domain(&l, "www.apex.example")["redirect_to"],
        "apex.example"
    );
}

#[tokio::test]
async fn generated_domain_is_on_by_default_and_can_be_turned_off() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "gen").await;
    let l = list(&env.state, &dev, p).await;
    assert_eq!(l["node_ip"], NODE_IP);
    assert_eq!(l["generated_domain"], true);
    let g = domain(&l, "gen.203.0.113.7.sslip.io");
    assert_eq!(g["generated"], true);

    let r = send(
        &env.state,
        &dev,
        "PATCH",
        &format!("/api/domains?project={p}"),
        Some(json!({ "generated_domain": false })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["generated_domain"], false);
    assert!(r.json()["domains"].as_array().unwrap().is_empty());
    // Toujours desactive a la relecture.
    let l = list(&env.state, &dev, p).await;
    assert_eq!(l["generated_domain"], false);
    assert!(l["domains"].as_array().unwrap().is_empty());

    // Un domaine genere ne se supprime pas : il se desactive.
    let r = send(
        &env.state,
        &dev,
        "PATCH",
        &format!("/api/domains?project={p}"),
        Some(json!({ "generated_domain": true })),
    )
    .await;
    let gid = domain(&r.json(), "gen.203.0.113.7.sslip.io")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = send(
        &env.state,
        &dev,
        "DELETE",
        &format!("/api/domains/{gid}"),
        None,
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
}

#[tokio::test]
async fn viewer_reads_but_cannot_change_domains() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let viewer = login(&env.state, "viewer").await;
    let p = project(&env.state, &dev, "viewed").await;
    let id = add_ok(&env.state, &dev, p, "viewed.example.com").await;
    list(&env.state, &viewer, p).await;
    let r = add(
        &env.state,
        &viewer,
        p,
        json!({ "host": "v2.example.com", "internal_port": 3000 }),
    )
    .await;
    assert_eq!(r.status, 403);
    for (m, path) in [
        ("POST", format!("/api/domains/{id}/verify")),
        ("POST", format!("/api/domains/{id}/renew")),
        ("POST", format!("/api/domains/{id}/primary")),
        ("DELETE", format!("/api/domains/{id}")),
    ] {
        let r = send(&env.state, &viewer, m, &path, None).await;
        assert_eq!(r.status, 403, "{m} {path}");
    }
}

async fn service_port(state: &AppState, domain: &str) -> Option<i32> {
    sqlx::query_scalar(
        "SELECT s.internal_port FROM domains d JOIN services s ON s.id = d.service_id WHERE d.id = $1::uuid",
    )
    .bind(domain)
    .fetch_one(&state.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn ops_cannot_change_internal_port_via_domains() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let ops = login(&env.state, "ops").await;
    let p = project(&env.state, &dev, "portguard").await;
    let id = add_ok(&env.state, &dev, p, "portguard.example.com").await;
    // Port et healthcheck : Projects:Write exige, Ops n'a que la lecture.
    for body in [
        json!({ "internal_port": 4000 }),
        json!({ "healthcheck_path": "/up" }),
    ] {
        let r = send(
            &env.state,
            &ops,
            "PATCH",
            &format!("/api/domains/{id}"),
            Some(body),
        )
        .await;
        assert_eq!(r.status, 403, "{}", r.raw);
    }
    let r = add(
        &env.state,
        &ops,
        p,
        json!({ "host": "ops-new.example.com", "internal_port": 4000 }),
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(service_port(&env.state, &id).await, Some(3000));
    // Le meme port renvoye (ce que fait le panneau) n'est pas un changement.
    let r = send(
        &env.state,
        &ops,
        "PATCH",
        &format!("/api/domains/{id}"),
        Some(json!({ "path_prefix": "/same", "internal_port": 3000 })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    // Creation sans port (celui du service est repris) ou avec le meme port.
    for (host, body) in [
        ("ops-a.example.com", json!({ "host": "ops-a.example.com" })),
        (
            "ops-b.example.com",
            json!({ "host": "ops-b.example.com", "internal_port": 3000 }),
        ),
    ] {
        let r = add(&env.state, &ops, p, body).await;
        assert_eq!(r.status, 201, "{host} {}", r.raw);
    }
    assert_eq!(service_port(&env.state, &id).await, Some(3000));
    // Le reste de la route reste ouvert a Ops (Domains:Write).
    let r = send(
        &env.state,
        &ops,
        "PATCH",
        &format!("/api/domains/{id}"),
        Some(json!({ "path_prefix": "/app" })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    // Developer : Projects:Write, le port change.
    let r = send(
        &env.state,
        &dev,
        "PATCH",
        &format!("/api/domains/{id}"),
        Some(json!({ "internal_port": 4000 })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(service_port(&env.state, &id).await, Some(4000));
}

/// Prend la prochaine tache `acme_issue` (horloge `at`).
async fn claim_acme(state: &AppState, at: SystemTime) -> jobs::Job {
    let clock = Clock::from_fn(move || at);
    let job = jobs::claim(&state.db, &clock, "test-worker")
        .await
        .unwrap()
        .expect("tache acme_issue due");
    assert_eq!(job.kind, JobKind::AcmeIssue);
    job
}

/// Repond a la prochaine demande d'emission recue par l'agent.
async fn answer(env: &mut Env, outcome: CertificateOutcome) -> String {
    match respond(env, outcome).await {
        ControlToAgent::IssueCertificate { host } => host,
        other => panic!("emission attendue : {other:?}"),
    }
}

/// Repond au prochain ordre de certificat (emission ou verification) recu
/// par l'agent et le renvoie.
async fn respond(env: &mut Env, outcome: CertificateOutcome) -> ControlToAgent {
    let (msg, host) = loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), env.agent.recv())
            .await
            .expect("ordre de certificat")
            .unwrap();
        match &msg {
            ControlToAgent::IssueCertificate { host }
            | ControlToAgent::CheckCertificate { host } => {
                let host = host.clone();
                break (msg, host);
            }
            _ => {}
        }
    };
    env.state.agents.publish(
        ServerId(env.server),
        AgentToControl::Certificate { host, outcome },
    );
    msg
}

#[tokio::test]
async fn renews_30_days_before_expiry() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "renew").await;
    let id = add_ok(&env.state, &dev, p, "renew.example.com").await;
    env.dns.set("renew.example.com", &[NODE_IP]);
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{id}/verify"),
        None,
    )
    .await;
    assert_eq!(r.json()["dns_status"], "ok");
    assert_eq!(r.json()["cert_status"], "issuing");

    // Emission : l'agent recoit la demande, le proxy repond (90 jours).
    let job = claim_acme(&env.state, SystemTime::now()).await;
    let first = job.id;
    let not_after = unix(SystemTime::now()) + 90 * 86_400;
    let state = env.state.clone();
    let handler = tokio::spawn(async move { domains::acme::issue(&state, job).await });
    let host = answer(
        &mut env,
        CertificateOutcome::Issued {
            not_after,
            issuer: "R11".into(),
        },
    )
    .await;
    assert_eq!(host, "renew.example.com");
    handler.await.unwrap().expect("emission reussie");
    // Tache terminee, comme le ferait le worker.
    assert!(
        jobs::complete(&env.state.db, &Clock::system(), first, "test-worker")
            .await
            .unwrap()
    );

    let l = list(&env.state, &dev, p).await;
    let d = domain(&l, "renew.example.com");
    assert_eq!(d["cert_status"], "active");
    assert_eq!(d["cert_issuer"], "R11");
    assert!(d["cert_expires_at"].as_str().is_some());

    // Renouvellement planifie a J-30 (une seule tache par domaine).
    let (run_at, attempts): (i64, i32) = sqlx::query_as(
        "SELECT extract(epoch FROM run_at)::bigint, max_attempts FROM jobs \
         WHERE kind = 'acme_issue' AND status = 'queued' AND payload->>'domain_id' = $1",
    )
    .bind(&id)
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert_eq!(run_at, not_after - 30 * 86_400);
    assert!(attempts > 5, "reprises avec delai croissant : {attempts}");

    // Pas avant l'echeance.
    let early = SystemTime::now() + Duration::from_secs(59 * 86_400);
    assert!(
        jobs::claim(&env.state.db, &Clock::from_fn(move || early), "w")
            .await
            .unwrap()
            .is_none()
    );

    // A J-30 : nouvelle demande (DNS reverifie) ; un echec garde le
    // certificat actif (encore valide), note l'erreur et replanifie avec un
    // delai croissant borne, toujours une seule tache par domaine.
    let due = not_after - 30 * 86_400 + 1;
    let mut at = due;
    for (n, delay) in [(1, 300), (2, 600)] {
        let t = UNIX_EPOCH + Duration::from_secs(at as u64);
        let job = claim_acme(&env.state, t).await;
        let job_id = job.id;
        let mut state = env.state.clone();
        state.clock = Clock::from_fn(move || t);
        let handler = tokio::spawn(async move { domains::acme::issue(&state, job).await });
        answer(
            &mut env,
            CertificateOutcome::Failed {
                reason: "validation ACME refusee".into(),
            },
        )
        .await;
        handler
            .await
            .unwrap()
            .expect("echec note, reprise planifiee");
        assert!(
            jobs::complete(
                &env.state.db,
                &Clock::from_fn(move || t),
                job_id,
                "test-worker"
            )
            .await
            .unwrap()
        );
        let l = list(&env.state, &dev, p).await;
        let d = domain(&l, "renew.example.com");
        assert_eq!(d["cert_status"], "active");
        assert_eq!(d["cert_error"], "validation ACME refusee");
        assert_eq!(acme_jobs(&env.state, &id).await, 1);
        let (run_at, failures): (i64, Option<String>) = sqlx::query_as(
            "SELECT extract(epoch FROM run_at)::bigint, payload->>'failures' FROM jobs \
             WHERE kind = 'acme_issue' AND status = 'queued' AND payload->>'domain_id' = $1",
        )
        .bind(&id)
        .fetch_one(&env.state.db)
        .await
        .unwrap();
        assert!(
            (run_at - (at + delay)).abs() <= 2,
            "{run_at} vs {}",
            at + delay
        );
        assert_eq!(failures.as_deref(), Some(n.to_string().as_str()));
        at = run_at + 1;
    }
}

/// Taches d'emission en file ou en cours pour le domaine.
async fn acme_jobs(state: &AppState, id: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM jobs WHERE kind = 'acme_issue' \
         AND status IN ('queued', 'running') AND payload->>'domain_id' = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .unwrap()
}

async fn verify(state: &AppState, s: &Session, id: &str) -> Reply {
    send(state, s, "POST", &format!("/api/domains/{id}/verify"), None).await
}

/// Ordres recus par l'agent depuis le dernier appel.
fn drain(env: &mut Env) -> Vec<ControlToAgent> {
    let mut out = Vec::new();
    while let Ok(msg) = env.agent.try_recv() {
        out.push(msg);
    }
    out
}

/// Ajoute, verifie et emet un certificat de 90 jours pour `host` (tache
/// conclue comme par le worker).
async fn issue_ok(env: &mut Env, dev: &Session, p: Uuid, host: &str) -> String {
    let id = add_ok(&env.state, dev, p, host).await;
    env.dns.set(host, &[NODE_IP]);
    let r = verify(&env.state, dev, &id).await;
    assert_eq!(r.json()["dns_status"], "ok", "{}", r.raw);
    let job = claim_acme(&env.state, SystemTime::now()).await;
    let job_id = job.id;
    let state = env.state.clone();
    let handler = tokio::spawn(async move { domains::acme::issue(&state, job).await });
    answer(
        env,
        CertificateOutcome::Issued {
            not_after: unix(SystemTime::now()) + 90 * 86_400,
            issuer: "R11".into(),
        },
    )
    .await;
    handler.await.unwrap().expect("emission reussie");
    assert!(
        jobs::complete(&env.state.db, &Clock::system(), job_id, "test-worker")
            .await
            .unwrap()
    );
    id
}

#[tokio::test]
async fn valid_certificate_is_not_reissued_and_one_job_per_domain() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "once").await;
    let id = issue_ok(&mut env, &dev, p, "once.example.com").await;
    assert_eq!(
        acme_jobs(&env.state, &id).await,
        1,
        "renouvellement planifie"
    );
    drain(&mut env);

    // Verifications repetees d'un certificat valide : aucune emission.
    for _ in 0..3 {
        let r = verify(&env.state, &dev, &id).await;
        assert_eq!(r.status, 200, "{}", r.raw);
        assert_eq!(r.json()["cert_status"], "active");
    }
    // Renouvellement manuel refuse a plus de 30 jours de l'expiration.
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{id}/renew"),
        None,
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.json()["error"]["code"], "certificate_still_valid");
    assert!(
        r.json()["error"]["message"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("certificat encore valide")
    );
    assert_eq!(acme_jobs(&env.state, &id).await, 1, "une seule chaine");
    assert!(
        drain(&mut env)
            .iter()
            .all(|m| !matches!(m, ControlToAgent::IssueCertificate { .. }))
    );
}

/// Une execution de la tache `acme_issue` du domaine, a l'heure `at`, dont
/// la verification DNS est negative (aucun ordre n'est attendu) ; la
/// conclut comme le worker. Renvoie les ordres envoyes a l'agent.
async fn negative_round(env: &mut Env, at: SystemTime) -> Vec<ControlToAgent> {
    drain(env);
    let job = claim_acme(&env.state, at).await;
    let job_id = job.id;
    domains::acme::issue(&env.state, job)
        .await
        .expect("tache conclue sans erreur");
    assert!(
        jobs::complete(&env.state.db, &Clock::system(), job_id, "test-worker")
            .await
            .unwrap()
    );
    let sent = drain(env);
    assert!(
        sent.iter()
            .all(|m| !matches!(m, ControlToAgent::IssueCertificate { .. })),
        "{sent:?}"
    );
    sent
}

fn in_a_day(days: u64) -> SystemTime {
    SystemTime::now() + Duration::from_secs(days * 86_400)
}

#[tokio::test]
async fn three_negative_verdicts_move_project_domain() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "moved").await;
    let host = "moved.example.com";
    let id = add_ok(&env.state, &dev, p, host).await;
    env.dns.set(host, &[NODE_IP]);
    for _ in 0..2 {
        let r = verify(&env.state, &dev, &id).await;
        assert_eq!(r.json()["dns_status"], "ok", "{}", r.raw);
    }
    assert_eq!(acme_jobs(&env.state, &id).await, 1, "une seule demande");

    // Le DNS change avant l'emission : les deux premiers verdicts negatifs
    // sont retentes (delai croissant), le domaine reste en ligne.
    env.dns.set(host, &["198.51.100.9"]);
    for (round, failures) in [(1u64, "1"), (2, "2")] {
        let sent = negative_round(&mut env, in_a_day(round)).await;
        assert!(
            sent.iter()
                .all(|m| !matches!(m, ControlToAgent::ApplyRoutes(_))),
            "route gardee : {sent:?}"
        );
        let l = list(&env.state, &dev, p).await;
        assert_eq!(domain(&l, host)["dns_status"], "ok", "verdict {round}");
        let (_, queued, _) = queued_job(&env.state, &id).await;
        assert_eq!(queued.as_deref(), Some(failures), "reprise planifiee");
    }

    // Troisieme verdict negatif d'affilee : erreur DNS, route retiree, aucune
    // reprise planifiee.
    let sent = negative_round(&mut env, in_a_day(3)).await;
    let last = sent
        .iter()
        .rev()
        .find_map(|m| match m {
            ControlToAgent::ApplyRoutes(r) => Some(r.clone()),
            _ => None,
        })
        .expect("table repoussee");
    assert!(last.iter().all(|r| r.host != host));
    let l = list(&env.state, &dev, p).await;
    let d = domain(&l, host);
    assert_eq!(d["dns_status"], "error");
    assert!(d["dns_error"].as_str().unwrap().contains("198.51.100.9"));
    assert_eq!(d["cert_status"], "none");
    assert_eq!(acme_jobs(&env.state, &id).await, 0);
}

async fn dns_failures(state: &AppState, id: &str) -> i16 {
    sqlx::query_scalar("SELECT dns_failures FROM domains WHERE id = $1::uuid")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .unwrap()
}

/// Une mauvaise reponse passagere du DNS ne coupe pas le domaine : la tache
/// est replanifiee, le verdict suivant (correct) remet le compteur a zero et
/// le certificat est emis.
#[tokio::test]
async fn transient_dns_failure_is_retried() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "blip").await;
    let host = "blip.example.com";
    let id = add_ok(&env.state, &dev, p, host).await;
    env.dns.set(host, &[NODE_IP]);
    let r = verify(&env.state, &dev, &id).await;
    assert_eq!(r.json()["dns_status"], "ok", "{}", r.raw);

    // Premier verdict : mauvaise IP une seule fois.
    env.dns.once(host, &["198.51.100.9"]);
    negative_round(&mut env, in_a_day(1)).await;
    assert_eq!(dns_failures(&env.state, &id).await, 1);
    assert_eq!(acme_jobs(&env.state, &id).await, 1, "tache replanifiee");
    let l = list(&env.state, &dev, p).await;
    assert_eq!(domain(&l, host)["dns_status"], "ok");

    // Second passage : la bonne IP, l'emission aboutit.
    drain(&mut env);
    let job = claim_acme(&env.state, in_a_day(2)).await;
    let job_id = job.id;
    let state = env.state.clone();
    let handler = tokio::spawn(async move { domains::acme::issue(&state, job).await });
    answer(
        &mut env,
        CertificateOutcome::Issued {
            not_after: unix(SystemTime::now()) + 90 * 86_400,
            issuer: "R11".into(),
        },
    )
    .await;
    handler.await.unwrap().expect("emission reussie");
    assert!(
        jobs::complete(&env.state.db, &Clock::system(), job_id, "test-worker")
            .await
            .unwrap()
    );
    assert_eq!(dns_failures(&env.state, &id).await, 0);
    let l = list(&env.state, &dev, p).await;
    let d = domain(&l, host);
    assert_eq!(d["dns_status"], "ok");
    assert_eq!(d["cert_status"], "active");
    assert_eq!(
        acme_jobs(&env.state, &id).await,
        1,
        "renouvellement planifie"
    );
}

#[tokio::test]
async fn sslip_and_nip_hosts_are_refused_and_generated_names_skip_reserved_hosts() {
    let env = Env::with(&[("CIPHERA_PANEL_ALIASES", "panel.203.0.113.7.sslip.io")]).await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "squat").await;
    for host in [
        "other.203.0.113.7.sslip.io",
        "sslip.io",
        "A.Sslip.IO",
        "x.y.nip.io",
        "nip.io",
    ] {
        let r = add(
            &env.state,
            &dev,
            p,
            json!({ "host": host, "internal_port": 3000 }),
        )
        .await;
        assert_eq!(r.status, 422, "{host} : {}", r.raw);
    }
    // Projet nomme comme l'alias du panneau : pas de domaine genere.
    let q = project(&env.state, &dev, "panel").await;
    let l = list(&env.state, &dev, q).await;
    assert!(l["domains"].as_array().unwrap().is_empty(), "{l}");
}

#[tokio::test]
async fn generated_domain_cannot_be_primary_and_redirects_need_a_live_primary() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "live").await;
    let l = list(&env.state, &dev, p).await;
    let gid = domain(&l, "live.203.0.113.7.sslip.io")["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{gid}/primary"),
        None,
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);

    // Principal pas encore en ligne : la redirection n'est pas publiee.
    let main = add_ok(&env.state, &dev, p, "live.example").await;
    let old = add_ok(&env.state, &dev, p, "old.live.example").await;
    env.dns.set("old.live.example", &[NODE_IP]);
    verify(&env.state, &dev, &old).await;
    let r = send(
        &env.state,
        &dev,
        "POST",
        &format!("/api/domains/{old}/redirect"),
        Some(json!({ "enabled": true })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let routes = env.last_routes().await;
    assert!(
        routes.iter().all(|r| r.host != "old.live.example"),
        "{routes:?}"
    );
    // Principal en ligne : redirection publiee.
    env.dns.set("live.example", &[NODE_IP]);
    verify(&env.state, &dev, &main).await;
    let routes = env.last_routes().await;
    let route = routes
        .iter()
        .find(|r| r.host == "old.live.example")
        .expect("redirection publiee");
    assert_eq!(route.redirect_to.as_deref(), Some("https://live.example"));
}

#[tokio::test]
async fn aaaa_records_pass_when_the_node_ipv6_is_configured() {
    let env = Env::with(&[("CIPHERA_PUBLIC_IP6", "2001:db8::7")]).await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "dual").await;
    let id = add_ok(&env.state, &dev, p, "dual.example.com").await;
    env.dns.set("dual.example.com", &[NODE_IP, "2001:db8::7"]);
    let r = verify(&env.state, &dev, &id).await;
    assert_eq!(r.json()["dns_status"], "ok", "{}", r.raw);
    let l = list(&env.state, &dev, p).await;
    assert_eq!(l["node_ip6"], "2001:db8::7");
}

/// Tache d'emission en file du domaine : (echeance, echecs, tentatives).
async fn queued_job(state: &AppState, id: &str) -> (i64, Option<String>, i32) {
    sqlx::query_as(
        "SELECT extract(epoch FROM run_at)::bigint, payload->>'failures', attempts FROM jobs \
         WHERE kind = 'acme_issue' AND status = 'queued' AND payload->>'domain_id' = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await
    .unwrap()
}

/// Prend et execute la prochaine tache d'emission (horloge reelle) ; la
/// conclut comme le worker.
async fn run_acme(env: &mut Env, outcomes: Vec<CertificateOutcome>) -> Vec<ControlToAgent> {
    let job = claim_acme(&env.state, SystemTime::now()).await;
    let job_id = job.id;
    let state = env.state.clone();
    let handler = tokio::spawn(async move { domains::acme::issue(&state, job).await });
    let mut asked = Vec::new();
    for outcome in outcomes {
        asked.push(respond(env, outcome).await);
    }
    handler.await.unwrap().expect("tache conclue");
    assert!(
        jobs::complete(&env.state.db, &Clock::system(), job_id, "test-worker")
            .await
            .unwrap()
    );
    asked
}

fn has_route(routes: &[Route], host: &str) -> bool {
    routes
        .iter()
        .any(|r| r.host == host && r.tls == TlsMode::Acme)
}

#[tokio::test]
async fn a_failed_verify_keeps_the_key_and_https_returns_without_reissue() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "kept").await;
    let host = "kept.example.com";
    let id = issue_ok(&mut env, &dev, p, host).await;
    let (renewal, _, _) = queued_job(&env.state, &id).await;
    drain(&mut env);

    // Verification en echec : route retiree, certificat garde (aucun ordre
    // de suppression), toujours affiche actif.
    env.dns.set(host, &["198.51.100.9"]);
    let r = verify(&env.state, &dev, &id).await;
    assert_eq!(r.json()["dns_status"], "error", "{}", r.raw);
    let sent = drain(&mut env);
    assert!(
        sent.iter()
            .all(|m| !matches!(m, ControlToAgent::RemoveCertificate { .. })),
        "{sent:?}"
    );
    let routes = sent
        .iter()
        .rev()
        .find_map(|m| match m {
            ControlToAgent::ApplyRoutes(r) => Some(r.clone()),
            _ => None,
        })
        .expect("table repoussee");
    assert!(!has_route(&routes, host));

    // DNS corrige : la route revient, le proxy est seulement interroge
    // (il detient toujours le certificat), rien n'est emis.
    env.dns.set(host, &[NODE_IP]);
    let r = verify(&env.state, &dev, &id).await;
    assert_eq!(r.json()["dns_status"], "ok", "{}", r.raw);
    assert_eq!(r.json()["cert_status"], "active");
    assert!(has_route(&env.last_routes().await, host));
    assert_eq!(acme_jobs(&env.state, &id).await, 1, "une seule chaine");
    let held = renewal + 30 * 86_400;
    let asked = run_acme(
        &mut env,
        vec![CertificateOutcome::Issued {
            not_after: held,
            issuer: "R11".into(),
        }],
    )
    .await;
    assert_eq!(
        asked,
        vec![ControlToAgent::CheckCertificate { host: host.into() }]
    );
    assert!(
        drain(&mut env)
            .iter()
            .all(|m| !matches!(m, ControlToAgent::IssueCertificate { .. }))
    );
    let l = list(&env.state, &dev, p).await;
    let d = domain(&l, host);
    assert_eq!(d["cert_status"], "active");
    assert!(d["cert_error"].is_null(), "{d}");
    // Renouvellement toujours planifie a J-30.
    let (run_at, failures, _) = queued_job(&env.state, &id).await;
    assert_eq!(run_at, renewal);
    assert_eq!(failures.as_deref(), Some("0"));
}

#[tokio::test]
async fn a_certificate_missing_on_the_proxy_is_issued_again() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "lost").await;
    let host = "lost.example.com";
    let id = issue_ok(&mut env, &dev, p, host).await;
    drain(&mut env);

    // Le plan de controle le croit actif ; le proxy ne l'a plus.
    let r = verify(&env.state, &dev, &id).await;
    assert_eq!(r.json()["cert_status"], "active", "{}", r.raw);
    let job = claim_acme(&env.state, SystemTime::now()).await;
    let job_id = job.id;
    let state = env.state.clone();
    let handler = tokio::spawn(async move { domains::acme::issue(&state, job).await });
    let first = respond(&mut env, CertificateOutcome::Missing).await;
    assert_eq!(
        first,
        ControlToAgent::CheckCertificate { host: host.into() }
    );
    // Emission demandee ; d'ici la, plus affiche actif.
    let not_after = unix(SystemTime::now()) + 89 * 86_400;
    let second = loop {
        let msg = tokio::time::timeout(Duration::from_secs(10), env.agent.recv())
            .await
            .expect("emission")
            .unwrap();
        if matches!(msg, ControlToAgent::IssueCertificate { .. }) {
            break msg;
        }
    };
    assert_eq!(
        second,
        ControlToAgent::IssueCertificate { host: host.into() }
    );
    let d = domain(&list(&env.state, &dev, p).await, host).clone();
    assert_eq!(d["cert_status"], "issuing", "{d}");
    assert!(d["cert_expires_at"].is_null());
    env.state.agents.publish(
        ServerId(env.server),
        AgentToControl::Certificate {
            host: host.into(),
            outcome: CertificateOutcome::Issued {
                not_after,
                issuer: "R12".into(),
            },
        },
    );
    handler.await.unwrap().expect("re-emission reussie");
    assert!(
        jobs::complete(&env.state.db, &Clock::system(), job_id, "test-worker")
            .await
            .unwrap()
    );
    let d = domain(&list(&env.state, &dev, p).await, host).clone();
    assert_eq!(d["cert_status"], "active");
    assert_eq!(d["cert_issuer"], "R12");
    let (run_at, _, _) = queued_job(&env.state, &id).await;
    assert_eq!(run_at, not_after - 30 * 86_400);
}

#[tokio::test]
async fn deleting_a_domain_or_its_project_removes_the_certificate() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let admin = login(&env.state, "admin").await;
    let p = project(&env.state, &dev, "gone").await;
    let id = issue_ok(&mut env, &dev, p, "gone.example.com").await;
    let _other = issue_ok(&mut env, &dev, p, "other.example.com").await;
    drain(&mut env);

    let r = send(
        &env.state,
        &dev,
        "DELETE",
        &format!("/api/domains/{id}"),
        None,
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    let removed: Vec<String> = drain(&mut env)
        .into_iter()
        .filter_map(|m| match m {
            ControlToAgent::RemoveCertificate { host } => Some(host),
            _ => None,
        })
        .collect();
    assert_eq!(removed, vec!["gone.example.com".to_string()]);

    let r = send(
        &env.state,
        &admin,
        "DELETE",
        &format!("/api/projects/{p}"),
        None,
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    let mut removed: Vec<String> = drain(&mut env)
        .into_iter()
        .filter_map(|m| match m {
            ControlToAgent::RemoveCertificate { host } => Some(host),
            _ => None,
        })
        .collect();
    // Le domaine genere a lui aussi un certificat a retirer.
    removed.sort();
    assert_eq!(
        removed,
        vec![
            "gone.203.0.113.7.sslip.io".to_string(),
            "other.example.com".to_string()
        ]
    );
}

/// Domaine supprime pendant son emission : le certificat emis est retire
/// du proxy, sauf si le meme hote a ete ajoute de nouveau entre-temps (la
/// suppression, indexee par hote, effacerait le certificat du nouveau).
#[tokio::test]
async fn late_issuance_of_a_deleted_domain_spares_a_re_added_host() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "readd").await;
    for (host, re_add) in [("gone-late.example.com", false), ("back.example.com", true)] {
        let id = add_ok(&env.state, &dev, p, host).await;
        env.dns.set(host, &[NODE_IP]);
        let r = verify(&env.state, &dev, &id).await;
        assert_eq!(r.json()["dns_status"], "ok", "{}", r.raw);
        let job = claim_acme(&env.state, SystemTime::now()).await;
        let job_id = job.id;
        let state = env.state.clone();
        let handler = tokio::spawn(async move { domains::acme::issue(&state, job).await });
        // L'emission est demandee ; le domaine est supprime avant l'issue.
        let asked = loop {
            let msg = tokio::time::timeout(Duration::from_secs(10), env.agent.recv())
                .await
                .expect("emission")
                .unwrap();
            if matches!(msg, ControlToAgent::IssueCertificate { .. }) {
                break msg;
            }
        };
        assert_eq!(
            asked,
            ControlToAgent::IssueCertificate { host: host.into() }
        );
        let r = send(
            &env.state,
            &dev,
            "DELETE",
            &format!("/api/domains/{id}"),
            None,
        )
        .await;
        assert_eq!(r.status, 204, "{}", r.raw);
        if re_add {
            add_ok(&env.state, &dev, p, host).await;
        }
        drain(&mut env);
        env.state.agents.publish(
            ServerId(env.server),
            AgentToControl::Certificate {
                host: host.into(),
                outcome: CertificateOutcome::Issued {
                    not_after: unix(SystemTime::now()) + 90 * 86_400,
                    issuer: "R11".into(),
                },
            },
        );
        handler.await.unwrap().expect("tache conclue");
        jobs::complete(&env.state.db, &Clock::system(), job_id, "test-worker")
            .await
            .unwrap();
        let removed = drain(&mut env)
            .into_iter()
            .any(|m| m == ControlToAgent::RemoveCertificate { host: host.into() });
        assert_eq!(removed, !re_add, "{host}");
    }
}

#[tokio::test]
async fn transient_failures_postpone_issuance_and_a_dead_chain_is_visible() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "later").await;
    let host = "later.example.com";
    let id = add_ok(&env.state, &dev, p, host).await;
    env.dns.set(host, &[NODE_IP]);
    let r = verify(&env.state, &dev, &id).await;
    assert_eq!(r.json()["cert_status"], "issuing", "{}", r.raw);

    // Aucun serveur : nouvel essai dans une minute, sans echec compte ni
    // tentative consommee.
    sqlx::query("UPDATE servers SET revoked_at = now()")
        .execute(&env.state.db)
        .await
        .unwrap();
    let before = unix(SystemTime::now());
    run_acme(&mut env, vec![]).await;
    let (run_at, failures, attempts) = queued_job(&env.state, &id).await;
    assert!(
        (run_at - (before + 60)).abs() <= 2,
        "{run_at} vs {}",
        before + 60
    );
    assert_eq!(failures.as_deref(), Some("0"));
    assert_eq!(attempts, 0);
    let d = domain(&list(&env.state, &dev, p).await, host).clone();
    assert_eq!(d["cert_status"], "issuing");
    assert!(d["cert_error"].is_null(), "{d}");

    // La chaine s'arrete malgre tout (tentatives epuisees) : erreur visible.
    sqlx::query("UPDATE jobs SET status = 'dead' WHERE payload->>'domain_id' = $1")
        .bind(&id)
        .execute(&env.state.db)
        .await
        .unwrap();
    let d = domain(&list(&env.state, &dev, p).await, host).clone();
    assert!(
        d["cert_error"]
            .as_str()
            .is_some_and(|e| e.contains("interrompue") && e.contains("Vérifier")),
        "{d}"
    );

    // Une verification relance la chaine.
    sqlx::query("UPDATE servers SET revoked_at = NULL")
        .execute(&env.state.db)
        .await
        .unwrap();
    let r = verify(&env.state, &dev, &id).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(r.json()["cert_error"].is_null(), "{}", r.raw);
    assert_eq!(acme_jobs(&env.state, &id).await, 1);
}

#[tokio::test]
async fn a_dead_renewal_chain_is_visible_on_an_active_certificate() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "dead").await;
    let id = issue_ok(&mut env, &dev, p, "dead.example.com").await;
    sqlx::query("UPDATE jobs SET status = 'dead' WHERE payload->>'domain_id' = $1")
        .bind(&id)
        .execute(&env.state.db)
        .await
        .unwrap();
    let d = domain(&list(&env.state, &dev, p).await, "dead.example.com").clone();
    assert_eq!(d["cert_status"], "active");
    assert!(
        d["cert_error"]
            .as_str()
            .is_some_and(|e| e.contains("interrompue")),
        "{d}"
    );
    // Verifier relance la chaine (verification du certificat detenu).
    let r = verify(&env.state, &dev, &id).await;
    assert!(r.json()["cert_error"].is_null(), "{}", r.raw);
    assert_eq!(acme_jobs(&env.state, &id).await, 1);
}

/// Taches `notify` des certificats : (evenement, renouvellement, expiration).
async fn cert_notices(state: &AppState) -> Vec<(String, Option<bool>, Option<i64>)> {
    sqlx::query_as(
        "SELECT payload->>'event', (payload->>'renewal')::boolean, (payload->>'expires_at')::bigint \
         FROM jobs WHERE kind = 'notify' ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await
    .unwrap()
}

/// Execute la tache d'emission due a `at` ; le proxy repond `outcome`. Les
/// taches `notify` mises en file sont mises de cote (envoi teste dans
/// tests/notifications.rs).
async fn acme_at(env: &mut Env, at: i64, outcome: CertificateOutcome) {
    sqlx::query("UPDATE jobs SET status = 'done' WHERE kind = 'notify'")
        .execute(&env.state.db)
        .await
        .unwrap();
    let t = UNIX_EPOCH + Duration::from_secs(at as u64);
    let job = claim_acme(&env.state, t).await;
    let job_id = job.id;
    let mut state = env.state.clone();
    state.clock = Clock::from_fn(move || t);
    let handler = tokio::spawn(async move { domains::acme::issue(&state, job).await });
    respond(env, outcome).await;
    handler.await.unwrap().expect("tache conclue");
    assert!(
        jobs::complete(
            &env.state.db,
            &Clock::from_fn(move || t),
            job_id,
            "test-worker"
        )
        .await
        .unwrap()
    );
}

#[tokio::test]
async fn certificate_failures_queue_one_notification_per_failing_chain() {
    let mut env = Env::new().await;
    sqlx::query(
        "INSERT INTO smtp_settings (enabled, host, port, tls, from_address, from_name) \
         VALUES (true, 'smtp.example.com', 587, 'starttls', 'ciphera@example.com', 'Ciphera')",
    )
    .execute(&env.state.db)
    .await
    .unwrap();
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "notif").await;
    let host = "notif.example.com";
    let id = add_ok(&env.state, &dev, p, host).await;
    env.dns.set(host, &[NODE_IP]);
    assert_eq!(
        verify(&env.state, &dev, &id).await.json()["cert_status"],
        "issuing"
    );
    let refused = || CertificateOutcome::Failed {
        reason: "validation ACME refusee".into(),
    };

    // Premiere emission en echec : notifiee ; sa reprise en echec : non.
    let now = unix(SystemTime::now()) + 1;
    acme_at(&mut env, now, refused()).await;
    acme_at(&mut env, now + 301, refused()).await;
    assert_eq!(
        cert_notices(&env.state).await,
        vec![("cert_failed".into(), Some(false), None)]
    );

    // Emis, puis renouvellement en echec a J-30 : notifie une fois.
    let not_after = now + 90 * 86_400;
    acme_at(
        &mut env,
        now + 2000,
        CertificateOutcome::Issued {
            not_after,
            issuer: "R11".into(),
        },
    )
    .await;
    let due = not_after - 30 * 86_400 + 1;
    acme_at(&mut env, due, refused()).await;
    acme_at(&mut env, due + 301, refused()).await;
    // A moins de 7 jours de l'expiration, toujours en echec : expiration
    // proche signalee.
    acme_at(&mut env, not_after - 6 * 86_400, refused()).await;
    assert_eq!(
        cert_notices(&env.state).await,
        vec![
            ("cert_failed".into(), Some(false), None),
            ("cert_failed".into(), Some(true), None),
            ("cert_expiring".into(), None, Some(not_after)),
        ]
    );
}

#[tokio::test]
async fn domain_cap_per_project() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "cap").await;
    for i in 0..20 {
        add_ok(&env.state, &dev, p, &format!("d{i}.cap.example")).await;
    }
    // Le 21e est refuse ; un domaine avec `www` compte pour deux.
    let r = add(
        &env.state,
        &dev,
        p,
        json!({ "host": "d20.cap.example", "internal_port": 3000 }),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    assert!(r.raw.contains("20"), "{}", r.raw);
    // Le domaine genere ne compte pas dans le plafond : il est deja la.
    let l = list(&env.state, &dev, p).await;
    assert_eq!(l["domains"].as_array().unwrap().len(), 21, "{l}");
    // Un autre projet n'est pas touche.
    let q = project(&env.state, &dev, "other").await;
    add_ok(&env.state, &dev, q, "one.other.example").await;
}

#[tokio::test]
async fn www_option_counts_for_two_against_the_cap() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "capw").await;
    for i in 0..19 {
        add_ok(&env.state, &dev, p, &format!("d{i}.capw.example")).await;
    }
    let r = add(
        &env.state,
        &dev,
        p,
        json!({ "host": "last.capw.example", "internal_port": 3000, "www": true }),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM domains WHERE NOT generated")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(count, 19);
}

#[tokio::test]
async fn domain_cap_per_instance() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    // 100 domaines repartis sur 5 projets, inseres directement.
    for n in 0..5 {
        let p = project(&env.state, &dev, &format!("inst{n}")).await;
        let (environment, service): (Uuid, Uuid) = sqlx::query_as(
            "SELECT e.id, s.id FROM environments e JOIN services s ON s.environment_id = e.id \
             WHERE e.project_id = $1 AND e.name = 'production' LIMIT 1",
        )
        .bind(p)
        .fetch_one(&env.state.db)
        .await
        .unwrap();
        for i in 0..20 {
            sqlx::query(
                "INSERT INTO domains (id, project_id, environment_id, service_id, host, path_prefix, \
                 generated, is_primary, redirect, dns_status) \
                 VALUES ($1, $2, $3, $4, $5, '/', false, false, false, 'pending')",
            )
            .bind(Uuid::new_v4())
            .bind(p)
            .bind(environment)
            .bind(service)
            .bind(format!("h{i}.inst{n}.example"))
            .execute(&env.state.db)
            .await
            .unwrap();
        }
    }
    let p = project(&env.state, &dev, "inst-last").await;
    let r = add(
        &env.state,
        &dev,
        p,
        json!({ "host": "one-more.example", "internal_port": 3000 }),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    assert!(r.raw.contains("100"), "{}", r.raw);
}

#[tokio::test]
async fn concurrent_adds_at_the_cap_let_only_one_through() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "race").await;
    for i in 0..19 {
        add_ok(&env.state, &dev, p, &format!("d{i}.race.example")).await;
    }
    let (a, b) = tokio::join!(
        add(
            &env.state,
            &dev,
            p,
            json!({ "host": "a.race.example", "internal_port": 3000 })
        ),
        add(
            &env.state,
            &dev,
            p,
            json!({ "host": "b.race.example", "internal_port": 3000 })
        ),
    );
    let mut statuses = [a.status, b.status];
    statuses.sort_unstable();
    assert_eq!(statuses, [201, 422], "{} / {}", a.raw, b.raw);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM domains WHERE NOT generated")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(count, 20);
}

/// Domaine genere du projet (id) apres creation du projet.
async fn generated_id(env: &Env, dev: &Session, p: Uuid, host: &str) -> String {
    domain(&list(&env.state, dev, p).await, host)["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Port interne du service du projet : la route du domaine genere existe.
async fn open_port(state: &AppState) {
    sqlx::query("UPDATE services SET internal_port = 3000")
        .execute(&state.db)
        .await
        .unwrap();
}

#[tokio::test]
async fn generated_domain_requests_cert() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let host = "gen.203.0.113.7.sslip.io";
    let p = project_raw(&env.state, &dev, "gen").await;
    open_port(&env.state).await;
    let id = generated_id(&env, &dev, p, host).await;
    // Une tentative d'emission est en file des la creation.
    assert_eq!(acme_jobs(&env.state, &id).await, 1);
    let d = domain(&list(&env.state, &dev, p).await, host).clone();
    assert_eq!(d["cert_status"], "issuing", "{d}");

    // La route est HTTPS (ACME) ; le DNS joker n'est pas reverifie.
    let job = claim_acme(&env.state, SystemTime::now()).await;
    let job_id = job.id;
    let state = env.state.clone();
    let handler = tokio::spawn(async move { domains::acme::issue(&state, job).await });
    let routes = env.next_routes().await;
    let route = routes.iter().find(|r| r.host == host).expect("route");
    assert_eq!(route.tls, TlsMode::Acme);
    let ordered = answer(
        &mut env,
        CertificateOutcome::Issued {
            not_after: unix(SystemTime::now()) + 90 * 86_400,
            issuer: "R11".into(),
        },
    )
    .await;
    assert_eq!(ordered, host);
    handler.await.unwrap().expect("emission reussie");
    assert!(
        jobs::complete(&env.state.db, &Clock::system(), job_id, "test-worker")
            .await
            .unwrap()
    );
    let d = domain(&list(&env.state, &dev, p).await, host).clone();
    assert_eq!(d["cert_status"], "active", "{d}");
    assert_eq!(d["cert_issuer"], "R11");
    assert!(d["cert_error"].is_null(), "{d}");
    // Renouvellement planifie.
    assert_eq!(acme_jobs(&env.state, &id).await, 1);
}

#[tokio::test]
async fn rate_limited_generated_domain_stays_http_and_retries() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let host = "slow.203.0.113.7.sslip.io";
    let p = project_raw(&env.state, &dev, "slow").await;
    open_port(&env.state).await;
    let id = generated_id(&env, &dev, p, host).await;

    let now = unix(SystemTime::now()) + 1;
    acme_at(
        &mut env,
        now,
        CertificateOutcome::Failed {
            reason: "too many certificates already issued".into(),
        },
    )
    .await;
    // Pas de certificat : le domaine reste servi (route inchangee, en HTTP
    // tant que le proxy n'a pas de certificat), l'erreur est visible, aucun
    // courriel n'est envoye.
    let d = domain(&list(&env.state, &dev, p).await, host).clone();
    assert_eq!(d["cert_status"], "error", "{d}");
    assert!(d["cert_expires_at"].is_null(), "{d}");
    assert!(
        d["cert_error"]
            .as_str()
            .is_some_and(|e| e.contains("too many")),
        "{d}"
    );
    assert!(cert_notices(&env.state).await.is_empty());
    let routes = domains::route_table(
        &mut env.state.db.acquire().await.unwrap(),
        &env.state.cfg,
        env.server,
    )
    .await
    .unwrap();
    assert!(
        routes
            .iter()
            .any(|r| r.host == host && r.tls == TlsMode::Acme)
    );

    // Une nouvelle tentative par jour au plus, puis le certificat arrive.
    let (run_at, failures, _) = queued_job(&env.state, &id).await;
    assert!((run_at - (now + 86_400)).abs() <= 2, "{run_at}");
    assert_eq!(failures.as_deref(), Some("1"));
    let early = UNIX_EPOCH + Duration::from_secs((now + 86_400 - 3600) as u64);
    let clock = Clock::from_fn(move || early);
    assert!(
        jobs::claim(&env.state.db, &clock, "test-worker")
            .await
            .unwrap()
            .is_none(),
        "pas de nouvelle tentative avant un jour"
    );
    acme_at(
        &mut env,
        now + 86_401,
        CertificateOutcome::Issued {
            not_after: now + 90 * 86_400,
            issuer: "R11".into(),
        },
    )
    .await;
    let d = domain(&list(&env.state, &dev, p).await, host).clone();
    assert_eq!(d["cert_status"], "active", "{d}");
    assert!(d["cert_error"].is_null(), "{d}");
}

#[tokio::test]
async fn generated_domain_without_a_route_waits_without_asking_the_proxy() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let host = "wait.203.0.113.7.sslip.io";
    let p = project_raw(&env.state, &dev, "wait").await;
    let id = generated_id(&env, &dev, p, host).await;
    // Sans port interne, aucune route : rien n'est demande au proxy et la
    // tache est replanifiee, sans echec ni tentative comptes.
    let before = unix(SystemTime::now());
    let asked = run_acme(&mut env, vec![]).await;
    assert!(asked.is_empty());
    assert!(drain(&mut env).iter().all(|m| !matches!(
        m,
        ControlToAgent::IssueCertificate { .. } | ControlToAgent::CheckCertificate { .. }
    )));
    let (run_at, failures, attempts) = queued_job(&env.state, &id).await;
    assert!(run_at > before + 60, "{run_at}");
    assert_eq!(failures.as_deref(), Some("0"));
    assert_eq!(attempts, 0);
    let d = domain(&list(&env.state, &dev, p).await, host).clone();
    assert!(d["cert_error"].is_null(), "{d}");
}

#[tokio::test]
async fn disabling_the_generated_domain_removes_its_certificate() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let host = "off.203.0.113.7.sslip.io";
    let p = project(&env.state, &dev, "off").await;
    drain(&mut env);
    let r = send(
        &env.state,
        &dev,
        "PATCH",
        &format!("/api/domains?project={p}"),
        Some(json!({ "generated_domain": false })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let orders = drain(&mut env);
    assert!(
        orders
            .iter()
            .any(|m| matches!(m, ControlToAgent::RemoveCertificate { host: h } if h == host)),
        "{orders:?}"
    );
}

#[tokio::test]
async fn recreating_the_generated_domain_postpones_its_first_attempt() {
    let mut env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let host = "again.203.0.113.7.sslip.io";
    let p = project_raw(&env.state, &dev, "again").await;
    let first = generated_id(&env, &dev, p, host).await;
    // Creation du projet : tentative immediate.
    let (run_at, _, _) = queued_job(&env.state, &first).await;
    assert!(run_at <= unix(SystemTime::now()) + 2, "{run_at}");

    // Desactiver puis reactiver ne relance pas l'emission avant un jour.
    for enabled in [false, true] {
        let r = send(
            &env.state,
            &dev,
            "PATCH",
            &format!("/api/domains?project={p}"),
            Some(json!({ "generated_domain": enabled })),
        )
        .await;
        assert_eq!(r.status, 200, "{}", r.raw);
    }
    let second = generated_id(&env, &dev, p, host).await;
    assert_ne!(first, second);
    let (run_at, _, _) = queued_job(&env.state, &second).await;
    let wanted = unix(SystemTime::now()) + 86_400;
    assert!((run_at - wanted).abs() <= 5, "{run_at} vs {wanted}");
    let d = domain(&list(&env.state, &dev, p).await, host).clone();
    assert_eq!(d["cert_status"], "none", "{d}");
    drain(&mut env);
}

#[tokio::test]
async fn regenerating_the_same_host_keeps_its_certificate() {
    let env = Env::new().await;
    let dev = login(&env.state, "developer").await;
    let p = project(&env.state, &dev, "same").await;
    let mut conn = env.state.db.acquire().await.unwrap();
    // Nom inchange : meme hote, rien a retirer du proxy.
    let removed = domains::regenerate(&mut conn, &env.state.cfg, p)
        .await
        .unwrap();
    assert!(removed.is_empty(), "{removed:?}");
}
