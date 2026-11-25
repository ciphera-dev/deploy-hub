//! Logs en direct : WebSocket `GET /api/ws/logs` (session, origine, droits),
//! masquage des secrets, tampon borne par client, suivi des agents compte
//! par abonnes, et telechargement `GET /api/deployments/{id}/logs.txt`.

mod common;

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::api::{Req, call};
use control_plane::app::build_router;
use control_plane::auth::password;
use control_plane::domain::deployments::retention;
use control_plane::domain::logs::hub::{self, LogHub, Source};
use control_plane::domain::logs::redact::{SecretValues, SecretsResult, SecretsUnavailable};
use control_plane::domain::logs::store::{self, NewLine};
use control_plane::state::AppState;
use futures_util::{SinkExt, StreamExt};
use proto::{
    AgentState, AgentToControl, ControlToAgent, LogLine, LogStream, LogTarget, Stage, StageStatus,
};
use serde_json::{Value, json};
use shared::config::Config;
use shared::ids::{DeploymentId, ServerId, ServiceId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message};
use uuid::Uuid;
use zeroize::Zeroizing;

const PASSWORD: &str = "correct horse battery staple";

/// Secrets fixes par projet (fixtures de test) ; `1` : source en panne ;
/// `2` : projets dont la lecture ne repond pas (base bloquee).
#[derive(Default)]
struct FixtureSecrets(
    Mutex<HashMap<Uuid, Vec<String>>>,
    AtomicBool,
    Mutex<HashSet<Uuid>>,
);

impl SecretValues for FixtureSecrets {
    fn values(&self, project: Uuid) -> Pin<Box<dyn Future<Output = SecretsResult> + Send + '_>> {
        if self.1.load(Ordering::SeqCst) {
            return Box::pin(async { Err(SecretsUnavailable) });
        }
        if self.2.lock().unwrap().contains(&project) {
            return Box::pin(std::future::pending());
        }
        let values = self
            .0
            .lock()
            .unwrap()
            .get(&project)
            .map(|v| v.iter().cloned().map(Zeroizing::new).collect())
            .unwrap_or_default();
        Box::pin(async move { Ok(values) })
    }
}

struct Env {
    state: AppState,
    _pg: common::TestDb,
    addr: SocketAddr,
    server: Uuid,
    agent: mpsc::Receiver<ControlToAgent>,
    project: Uuid,
    environment: Uuid,
    service: Uuid,
    deployment: Uuid,
    _stop: watch::Sender<bool>,
}

impl Env {
    async fn new() -> Self {
        Self::with(|_| {}).await
    }

    async fn with(configure: impl FnOnce(&mut AppState)) -> Self {
        let (mut state, pg) = common::state().await;
        configure(&mut state);
        let server = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO servers (id, name, kind, cert_fingerprint) VALUES ($1, 'local', 'local', 'AA:BB')",
        )
        .bind(server)
        .execute(&state.db)
        .await
        .unwrap();
        let agent = state.agents.attach_channel(ServerId(server));
        let (project, environment, service) = image_project(&state, "web").await;
        let deployment = seed_deployment(&state, project, environment, Some(server)).await;
        let (stop, stop_rx) = watch::channel(false);
        tokio::spawn(hub::run(
            state.logs.clone(),
            state.agents.subscribe(),
            state.agents.subscribe_logs(),
            stop_rx.clone(),
        ));
        tokio::spawn(store::run(
            state.clone(),
            state.agents.subscribe_logs(),
            stop_rx,
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = build_router(state.clone());
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        Self {
            state,
            _pg: pg,
            addr,
            server,
            agent,
            project,
            environment,
            service,
            deployment,
            _stop: stop,
        }
    }

    fn origin(&self) -> String {
        format!("https://{}", self.addr)
    }

    fn orders(&mut self) -> Vec<ControlToAgent> {
        let mut out = Vec::new();
        while let Ok(m) = self.agent.try_recv() {
            out.push(m);
        }
        out
    }

    /// Ordres de l'agent apparus dans les 5 s (au moins un attendu).
    async fn wait_orders(&mut self) -> Vec<ControlToAgent> {
        for _ in 0..100 {
            let orders = self.orders();
            if !orders.is_empty() {
                return orders;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Vec::new()
    }

    fn agent_says(&self, msg: AgentToControl) {
        self.state.agents.publish(ServerId(self.server), msg);
    }

    fn line(&self, target: LogTarget, stream: LogStream, text: &str) {
        self.agent_says(AgentToControl::LogLine(LogLine {
            target,
            stream,
            timestamp_ms: 1_763_800_000_123,
            line: text.to_string(),
        }));
    }

    async fn ws(&self, query: &str, cookie: Option<&str>, origin: Option<&str>) -> Result<Ws, u16> {
        let mut req = format!("ws://{}/api/ws/logs?{query}", self.addr)
            .into_client_request()
            .unwrap();
        if let Some(c) = cookie {
            req.headers_mut().insert(
                "cookie",
                HeaderValue::from_str(&format!("__Host-ciphera_session={c}")).unwrap(),
            );
        }
        if let Some(o) = origin {
            req.headers_mut()
                .insert("origin", HeaderValue::from_str(o).unwrap());
        }
        let tcp = TcpStream::connect(self.addr).await.unwrap();
        match tokio_tungstenite::client_async(req, tcp).await {
            Ok((ws, _)) => Ok(Ws(ws)),
            Err(tungstenite::Error::Http(res)) => Err(res.status().as_u16()),
            Err(e) => panic!("poignee de main WebSocket : {e}"),
        }
    }

    /// GET brut (corps de plus de 1 Mio possible) : statut, en-tetes, corps.
    async fn raw_get(&self, path: &str, cookie: &str) -> (u16, String, Vec<u8>) {
        let mut tcp = TcpStream::connect(self.addr).await.unwrap();
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {}\r\nCookie: __Host-ciphera_session={cookie}\r\nConnection: close\r\n\r\n",
            self.addr
        );
        tcp.write_all(req.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        tcp.read_to_end(&mut raw).await.unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = dechunk(
            &raw[split + 4..],
            head.to_ascii_lowercase()
                .contains("transfer-encoding: chunked"),
        );
        (status, head, body)
    }
}

fn dechunk(mut raw: &[u8], chunked: bool) -> Vec<u8> {
    if !chunked {
        return raw.to_vec();
    }
    let mut out = Vec::new();
    loop {
        let end = raw.windows(2).position(|w| w == b"\r\n").unwrap();
        let size =
            usize::from_str_radix(std::str::from_utf8(&raw[..end]).unwrap().trim(), 16).unwrap();
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&raw[end + 2..end + 2 + size]);
        raw = &raw[end + 2 + size + 2..];
    }
}

struct Ws(tokio_tungstenite::WebSocketStream<TcpStream>);

impl Ws {
    async fn next_json(&mut self) -> Value {
        loop {
            let msg = tokio::time::timeout(Duration::from_secs(5), self.0.next())
                .await
                .expect("message attendu")
                .expect("flux ouvert")
                .expect("message lisible");
            if let Message::Text(t) = msg {
                return serde_json::from_str(&t).unwrap();
            }
        }
    }

    async fn close(mut self) {
        let _ = self.0.close(None).await;
        let _ = self.0.send(Message::Close(None)).await;
    }
}

async fn login(state: &AppState, role: &str) -> String {
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
    r.token()
}

/// Projet image cree par l'API : (projet, environnement, service).
async fn image_project(state: &AppState, name: &str) -> (Uuid, Uuid, Uuid) {
    let email = format!("dev-{}@example.com", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, 'developer', 'Dev', now())",
    )
    .bind(Uuid::new_v4())
    .bind(&email)
    .bind(password::hash(PASSWORD).unwrap())
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
    let (token, csrf) = (r.token(), r.csrf());
    let r = call(
        state,
        "POST",
        "/api/projects",
        Req {
            cookie: Some(&token),
            csrf: Some(&csrf),
            body: Some(
                json!({ "name": name, "source": { "type": "image", "image": "ghcr.io/acme/app:1" } }),
            ),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let p = r.json();
    let id = |v: &Value| Uuid::parse_str(v.as_str().unwrap()).unwrap();
    (
        id(&p["id"]),
        id(&p["environments"][0]["id"]),
        id(&p["services"][0]["id"]),
    )
}

/// Deploiement reussi insere directement (sert d'actuel a son environnement).
async fn seed_deployment(state: &AppState, project: Uuid, env: Uuid, server: Option<Uuid>) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, environment_id, server_id, status, trigger, \
         created_at, started_at, finished_at, build_sent_at) \
         VALUES ($1, $2, $3, $4, 'success', 'manual', now(), now(), clock_timestamp(), now())",
    )
    .bind(id)
    .bind(project)
    .bind(env)
    .bind(server)
    .execute(&state.db)
    .await
    .unwrap();
    id
}

fn logs_orders(orders: &[ControlToAgent]) -> Vec<(LogTarget, u32, bool)> {
    orders
        .iter()
        .filter_map(|o| match o {
            ControlToAgent::Logs {
                target,
                tail,
                follow,
            } => Some((*target, *tail, *follow)),
            _ => None,
        })
        .collect()
}

fn stop_orders(orders: &[ControlToAgent]) -> Vec<LogTarget> {
    orders
        .iter()
        .filter_map(|o| match o {
            ControlToAgent::StopLogs { target } => Some(*target),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn ninth_ws_stream_refused_for_same_account() {
    let env = Env::new().await;
    let viewer = login(&env.state, "viewer").await;
    let origin = env.origin();
    let q = format!("deployment={}", env.deployment);
    let mut open = Vec::new();
    for _ in 0..hub::MAX_STREAMS_PER_USER {
        open.push(env.ws(&q, Some(&viewer), Some(&origin)).await.unwrap());
    }
    assert_eq!(
        env.ws(&q, Some(&viewer), Some(&origin)).await.err(),
        Some(429)
    );
    // Un autre compte passe ; un flux ferme libere une place.
    let other = login(&env.state, "admin").await;
    assert!(env.ws(&q, Some(&other), Some(&origin)).await.is_ok());
    open.pop().unwrap().close().await;
    let mut again = None;
    for _ in 0..40 {
        if let Ok(ws) = env.ws(&q, Some(&viewer), Some(&origin)).await {
            again = Some(ws);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(again.is_some(), "place non rendue apres fermeture");
}

#[tokio::test]
async fn ws_requires_session_and_same_origin() {
    let mut env = Env::new().await;
    let viewer = login(&env.state, "viewer").await;
    let origin = env.origin();
    let q = format!("deployment={}", env.deployment);

    // Sans session : 401, meme avec la bonne origine.
    assert_eq!(env.ws(&q, None, Some(&origin)).await.err(), Some(401));
    // Origine absente, d'un autre site ou d'un autre schema : 403 avant
    // la mise a niveau.
    assert_eq!(env.ws(&q, Some(&viewer), None).await.err(), Some(403));
    assert_eq!(
        env.ws(&q, Some(&viewer), Some("https://evil.example"))
            .await
            .err(),
        Some(403)
    );
    let http = format!("http://{}", env.addr);
    assert_eq!(
        env.ws(&q, Some(&viewer), Some(&http)).await.err(),
        Some(403)
    );
    let suffixed = format!("{origin}.evil.example");
    assert_eq!(
        env.ws(&q, Some(&viewer), Some(&suffixed)).await.err(),
        Some(403)
    );
    assert!(env.orders().is_empty());
    assert_eq!(env.state.logs.streams(), 0);

    // Session et meme origine : flux ouvert.
    let ws = env.ws(&q, Some(&viewer), Some(&origin)).await;
    assert!(ws.is_ok(), "{:?}", ws.err());
    ws.unwrap().close().await;
}

#[tokio::test]
async fn ws_origin_is_the_public_url_when_configured() {
    let env = Env::with(|state| {
        state.cfg = Arc::new(
            Config::from_lookup(|name| match name {
                "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
                "CIPHERA_PUBLIC_URL" => Some("https://panel.example.com".to_string()),
                _ => None,
            })
            .unwrap(),
        );
    })
    .await;
    let viewer = login(&env.state, "viewer").await;
    let q = format!("deployment={}", env.deployment);
    // L'hote de la requete ne compte plus : seule l'adresse publique est admise.
    assert_eq!(
        env.ws(&q, Some(&viewer), Some(&env.origin())).await.err(),
        Some(403)
    );
    let ws = env
        .ws(&q, Some(&viewer), Some("https://panel.example.com"))
        .await;
    assert!(ws.is_ok(), "{:?}", ws.err());
}

#[tokio::test]
async fn ws_forbidden_for_other_project_scope() {
    let mut env = Env::new().await;
    let origin = env.origin();
    let viewer = login(&env.state, "viewer").await;

    // Identifiant inconnu (deploiement ou service) ou invalide : 404.
    for q in [
        format!("deployment={}", Uuid::new_v4()),
        format!("service={}", Uuid::new_v4()),
        "deployment=pas-un-uuid".to_string(),
    ] {
        assert_eq!(
            env.ws(&q, Some(&viewer), Some(&origin)).await.err(),
            Some(404),
            "{q}"
        );
    }
    // Requete invalide : 422.
    for q in [
        String::new(),
        format!("deployment={}&service={}", env.deployment, env.service),
        format!("service={}&tail=5000", env.service),
    ] {
        assert_eq!(
            env.ws(&q, Some(&viewer), Some(&origin)).await.err(),
            Some(422),
            "{q}"
        );
    }

    // Role sans droit de lecture sur les logs : 403.
    sqlx::query("ALTER TABLE users DROP CONSTRAINT users_role_check")
        .execute(&env.state.db)
        .await
        .unwrap();
    let outsider = login(&env.state, "superuser").await;
    for q in [
        format!("deployment={}", env.deployment),
        format!("service={}", env.service),
    ] {
        assert_eq!(
            env.ws(&q, Some(&outsider), Some(&origin)).await.err(),
            Some(403),
            "{q}"
        );
    }
    let (status, _, _) = env
        .raw_get(
            &format!("/api/deployments/{}/logs.txt", env.deployment),
            &outsider,
        )
        .await;
    assert_eq!(status, 403);

    // Dans aucun cas un flux n'a ete ouvert ni un suivi demande a l'agent.
    assert!(env.orders().is_empty());
    assert_eq!(env.state.logs.streams(), 0);
}

#[tokio::test]
async fn secret_values_redacted_in_log_lines() {
    // Secrets du projet (fixtures) : le plus long est masque en premier ; une
    // valeur de moins de 4 caracteres n'est jamais masquee.
    let secrets = Arc::new(FixtureSecrets::default());
    let fixture = secrets.clone();
    let mut env = Env::with(move |state| {
        state.logs = LogHub::new(state.agents.clone(), fixture);
    })
    .await;
    secrets.0.lock().unwrap().insert(
        env.project,
        vec![
            "hunter2".to_string(),
            "hunter2-long-secret".to_string(),
            "abc".to_string(),
        ],
    );
    let viewer = login(&env.state, "viewer").await;
    let origin = env.origin();

    let raw = "token=hunter2-long-secret pw=hunter2 short=abc";
    let masked = "token=•••• pw=•••• short=abc";

    // Journal d'un deploiement : flux en direct et fichier telecharge.
    let target = LogTarget::Deployment(DeploymentId(env.deployment));
    let mut ws = env
        .ws(
            &format!("deployment={}", env.deployment),
            Some(&viewer),
            Some(&origin),
        )
        .await
        .unwrap();
    env.line(target, LogStream::Stderr, raw);
    let msg = ws.next_json().await;
    assert_eq!(msg["message"], masked);
    assert_eq!(msg["level"], "error");
    assert_eq!(msg["container"], "build");
    assert_eq!(msg["ts"], "2025-11-22T08:26:40.123Z");

    // Ligne enregistree masquee, puis fichier telecharge.
    let mut stored = Vec::new();
    for _ in 0..100 {
        stored = sqlx::query_scalar::<_, String>(
            "SELECT line FROM deployment_logs WHERE deployment_id = $1",
        )
        .bind(env.deployment)
        .fetch_all(&env.state.db)
        .await
        .unwrap();
        if !stored.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(stored, vec![masked.to_string()]);
    let (status, _, body) = env
        .raw_get(
            &format!("/api/deployments/{}/logs.txt", env.deployment),
            &viewer,
        )
        .await;
    assert_eq!(status, 200);
    let text = String::from_utf8(body).unwrap();
    assert!(text.contains(masked), "{text}");
    assert!(!text.contains("hunter2"), "{text}");

    // Ligne enregistree avant l'ajout d'un secret : masquee a la lecture.
    store::append(
        &env.state.db,
        env.deployment,
        &[NewLine {
            timestamp_ms: 1_763_800_000_000,
            stream: LogStream::Stdout,
            line: "ancien hunter2".to_string(),
        }],
    )
    .await
    .unwrap();
    let (_, _, body) = env
        .raw_get(
            &format!("/api/deployments/{}/logs.txt", env.deployment),
            &viewer,
        )
        .await;
    assert!(!String::from_utf8(body).unwrap().contains("hunter2"));
    // Historique envoye a l'ouverture d'un flux : masque aussi.
    let mut late = env
        .ws(
            &format!("deployment={}", env.deployment),
            Some(&viewer),
            Some(&origin),
        )
        .await
        .unwrap();
    let first = late.next_json().await;
    let second = late.next_json().await;
    for m in [&first, &second] {
        assert!(!m["message"].as_str().unwrap().contains("hunter2"), "{m}");
    }

    // Journal d'un service : meme masquage.
    let mut svc = env
        .ws(
            &format!("service={}", env.service),
            Some(&viewer),
            Some(&origin),
        )
        .await
        .unwrap();
    assert_eq!(
        logs_orders(&env.wait_orders().await),
        vec![(LogTarget::Service(ServiceId(env.service)), 500, true)]
    );
    env.line(
        LogTarget::Service(ServiceId(env.service)),
        LogStream::Stdout,
        raw,
    );
    let msg = svc.next_json().await;
    assert_eq!(msg["message"], masked);
    assert_eq!(msg["level"], "info");
    assert_eq!(msg["container"], "web");
    ws.close().await;
}

#[tokio::test]
async fn slow_client_does_not_block_others() {
    let mut env = Env::new().await;
    let target = LogTarget::Service(ServiceId(env.service));
    let source = || Source {
        target,
        project: env.project,
        container: "web".to_string(),
        follow: Some((ServerId(env.server), 500)),
        origin: None,
    };
    let mut fast = env.state.logs.subscribe(source(), 0);
    let mut slow = env.state.logs.subscribe(source(), 0);
    // Un seul suivi demande a l'agent pour deux clients.
    assert_eq!(logs_orders(&env.orders()), vec![(target, 500, true)]);

    let total = hub::CLIENT_BUFFER + 500;
    for i in 0..total {
        // Diffusion sans attente : le client lent ne retient pas l'agent.
        env.state
            .logs
            .dispatch(
                ServerId(env.server),
                &AgentToControl::LogLine(LogLine {
                    target,
                    stream: LogStream::Stdout,
                    timestamp_ms: 1_763_800_000_000 + i as i64,
                    line: format!("ligne {i}"),
                }),
            )
            .await;
        let got = fast.next().await.expect("ligne");
        assert_eq!(got.message, format!("ligne {i}"));
    }

    // Le client lent a perdu les plus anciennes lignes, signalees une fois.
    let marker = slow.next().await.unwrap();
    assert_eq!(marker.message, "500 lignes perdues");
    assert_eq!(marker.level, "warn");
    let first = slow.next().await.unwrap();
    assert_eq!(first.message, "ligne 500");
    let mut last = first;
    for _ in 1..hub::CLIENT_BUFFER {
        last = slow.next().await.unwrap();
    }
    assert_eq!(last.message, format!("ligne {}", total - 1));
    drop(fast);
    drop(slow);
    assert_eq!(stop_orders(&env.orders()), vec![target]);
}

#[tokio::test]
async fn last_unsubscribe_sends_stop_logs() {
    let mut env = Env::new().await;
    let viewer = login(&env.state, "viewer").await;
    let origin = env.origin();
    let target = LogTarget::Service(ServiceId(env.service));
    let q = format!("service={}&tail=100", env.service);

    let first = env.ws(&q, Some(&viewer), Some(&origin)).await.unwrap();
    assert_eq!(
        logs_orders(&env.wait_orders().await),
        vec![(target, 100, true)]
    );
    let mut second = env.ws(&q, Some(&viewer), Some(&origin)).await.unwrap();
    // Le second client recoit les lignes recentes gardees par le plan de
    // controle, sans nouveau suivi chez l'agent.
    env.line(target, LogStream::Stdout, "avant");
    assert_eq!(second.next_json().await["message"], "avant");
    let mut third = env.ws(&q, Some(&viewer), Some(&origin)).await.unwrap();
    assert_eq!(third.next_json().await["message"], "avant");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(env.orders().is_empty(), "un seul suivi par cible");

    first.close().await;
    third.close().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(stop_orders(&env.orders()).is_empty(), "un abonne reste");
    assert_eq!(env.state.logs.streams(), 1);

    second.close().await;
    assert_eq!(stop_orders(&env.wait_orders().await), vec![target]);
    assert_eq!(env.state.logs.streams(), 0);

    // Reconnexion de l'agent pendant un suivi : la demande est renvoyee.
    let again = env.ws(&q, Some(&viewer), Some(&origin)).await.unwrap();
    assert_eq!(
        logs_orders(&env.wait_orders().await),
        vec![(target, 100, true)]
    );
    env.agent_says(AgentToControl::State(AgentState::default()));
    assert_eq!(
        logs_orders(&env.wait_orders().await),
        vec![(target, 0, true)]
    );
    again.close().await;
    assert_eq!(stop_orders(&env.wait_orders().await), vec![target]);

    // Lignes en vol apres l'arret : rien de plus.
    env.line(target, LogStream::Stdout, "en vol");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(env.orders().is_empty());

    // Lignes d'un suivi que personne n'a demande a ce plan de controle (il a
    // redemarre) : l'agent est prie d'arreter, une seule fois.
    let orphan = LogTarget::Service(ServiceId::new());
    env.line(orphan, LogStream::Stdout, "orpheline");
    env.line(orphan, LogStream::Stdout, "orpheline");
    assert_eq!(stop_orders(&env.wait_orders().await), vec![orphan]);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(env.orders().is_empty());
}

#[tokio::test]
async fn service_without_serving_deployment_opens_without_follow() {
    let mut env = Env::new().await;
    let viewer = login(&env.state, "viewer").await;
    sqlx::query("UPDATE deployments SET status = 'failed' WHERE id = $1")
        .bind(env.deployment)
        .execute(&env.state.db)
        .await
        .unwrap();
    let ws = env
        .ws(
            &format!("service={}", env.service),
            Some(&viewer),
            Some(&env.origin()),
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        env.orders().is_empty(),
        "aucun conteneur servi : rien a suivre"
    );
    ws.close().await;
}

#[tokio::test]
async fn logs_txt_is_an_attachment_with_a_size_cap() {
    let env = Env::new().await;
    let viewer = login(&env.state, "viewer").await;
    let lines: Vec<NewLine> = (0..3)
        .map(|i| NewLine {
            timestamp_ms: 1_763_800_000_000 + i,
            stream: if i == 1 {
                LogStream::Stderr
            } else {
                LogStream::Stdout
            },
            line: format!("etape {i}"),
        })
        .collect();
    store::append(&env.state.db, env.deployment, &lines)
        .await
        .unwrap();
    let path = format!("/api/deployments/{}/logs.txt", env.deployment);
    let (status, head, body) = env.raw_get(&path, &viewer).await;
    assert_eq!(status, 200, "{head}");
    let head = head.to_ascii_lowercase();
    assert!(
        head.contains("content-type: text/plain; charset=utf-8"),
        "{head}"
    );
    assert!(head.contains("x-content-type-options: nosniff"), "{head}");
    assert!(
        head.contains("content-disposition: attachment; filename=\"deployment-"),
        "{head}"
    );
    assert!(head.contains("cache-control: no-store"), "{head}");
    assert_eq!(
        String::from_utf8(body).unwrap(),
        "2025-11-22T08:26:40.000Z [INFO] build | etape 0\n\
         2025-11-22T08:26:40.001Z [ERR] build | etape 1\n\
         2025-11-22T08:26:40.002Z [INFO] build | etape 2\n"
    );

    // Sans session : 401 ; deploiement inconnu : 404.
    let r = call(&env.state, "GET", &path, Req::default()).await;
    assert_eq!(r.status, 401);
    let (status, _, _) = env
        .raw_get(
            &format!("/api/deployments/{}/logs.txt", Uuid::new_v4()),
            &viewer,
        )
        .await;
    assert_eq!(status, 404);

    // Plafond : au-dela de MAX_DOWNLOAD_BYTES, le fichier est tronque.
    let big = "x".repeat(store::MAX_LINE_BYTES);
    let n = store::MAX_DOWNLOAD_BYTES / store::MAX_LINE_BYTES + 50;
    let lines: Vec<NewLine> = (0..n)
        .map(|i| NewLine {
            timestamp_ms: 1_763_800_001_000 + i as i64,
            stream: LogStream::Stdout,
            line: big.clone(),
        })
        .collect();
    store::append(&env.state.db, env.deployment, &lines)
        .await
        .unwrap();
    let (status, _, body) = env.raw_get(&path, &viewer).await;
    assert_eq!(status, 200);
    assert!(
        body.len() <= store::MAX_DOWNLOAD_BYTES + 200,
        "{}",
        body.len()
    );
    let text = String::from_utf8(body).unwrap();
    assert!(
        text.ends_with("[journal tronqué]\n"),
        "{}",
        &text[text.len() - 80..]
    );
}

#[tokio::test]
async fn stored_lines_are_capped_truncated_and_removed_with_the_deployment() {
    let env = Env::new().await;
    // Plafond par deploiement : les plus anciennes lignes sont supprimees,
    // les dernieres gardees.
    let batch = |from: usize, n: usize| -> Vec<NewLine> {
        (from..from + n)
            .map(|i| NewLine {
                timestamp_ms: i as i64,
                stream: LogStream::Stdout,
                line: format!("l{i}"),
            })
            .collect()
    };
    let kept = store::append(&env.state.db, env.deployment, &batch(0, 6_000))
        .await
        .unwrap();
    assert_eq!(kept, 6_000);
    let kept = store::append(&env.state.db, env.deployment, &batch(6_000, 6_000))
        .await
        .unwrap();
    assert_eq!(kept, 6_000);
    let count = |id: Uuid| {
        let db = env.state.db.clone();
        async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM deployment_logs WHERE deployment_id = $1",
            )
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap()
        }
    };
    assert_eq!(count(env.deployment).await, store::MAX_LINES_PER_DEPLOYMENT);
    let lines: Vec<String> = sqlx::query_scalar(
        "SELECT line FROM deployment_logs WHERE deployment_id = $1 ORDER BY seq",
    )
    .bind(env.deployment)
    .fetch_all(&env.state.db)
    .await
    .unwrap();
    assert_eq!(lines.first().map(String::as_str), Some("l2000"));
    assert_eq!(lines.last().map(String::as_str), Some("l11999"));
    let counter: i32 = sqlx::query_scalar("SELECT log_lines FROM deployments WHERE id = $1")
        .bind(env.deployment)
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(i64::from(counter), store::MAX_LINES_PER_DEPLOYMENT);
    // Un lot plus grand que le plafond : ses dernieres lignes seulement.
    store::append(&env.state.db, env.deployment, &batch(20_000, 12_000))
        .await
        .unwrap();
    let tail = store::tail(&env.state.db, env.deployment, 1).await.unwrap();
    assert_eq!(tail[0].line, "l31999");
    assert_eq!(count(env.deployment).await, store::MAX_LINES_PER_DEPLOYMENT);
    let first: String = sqlx::query_scalar(
        "SELECT line FROM deployment_logs WHERE deployment_id = $1 ORDER BY seq LIMIT 1",
    )
    .bind(env.deployment)
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert_eq!(first, "l22000");

    // Ligne trop longue : tronquee a MAX_LINE_BYTES (sur une frontiere de
    // caractere).
    let other = seed_deployment(&env.state, env.project, env.environment, None).await;
    let long = "é".repeat(store::MAX_LINE_BYTES);
    store::append(
        &env.state.db,
        other,
        &[NewLine {
            timestamp_ms: 0,
            stream: LogStream::Stdout,
            line: long,
        }],
    )
    .await
    .unwrap();
    let line: String =
        sqlx::query_scalar("SELECT line FROM deployment_logs WHERE deployment_id = $1")
            .bind(other)
            .fetch_one(&env.state.db)
            .await
            .unwrap();
    assert!(line.len() <= store::MAX_LINE_BYTES && line.len() >= store::MAX_LINE_BYTES - 1);

    // Retention : les deploiements supprimes par le nettoyage perdent leurs
    // lignes ; ceux conserves les gardent.
    let mut newer = Vec::new();
    for _ in 0..5 {
        newer.push(seed_deployment(&env.state, env.project, env.environment, None).await);
    }
    for id in &newer {
        store::append(&env.state.db, *id, &batch(0, 3))
            .await
            .unwrap();
    }
    assert_eq!(
        retention::cleanup(&env.state, env.environment)
            .await
            .unwrap(),
        0
    );
    // Envoye a l'agent : lignes gardees jusqu'a son accuse.
    assert!(count(env.deployment).await > 0, "en attente de l'accuse");
    retention::removed(&env.state, env.server, env.deployment)
        .await
        .unwrap();
    assert_eq!(count(env.deployment).await, 0, "plus ancien : nettoye");
    assert_eq!(count(other).await, 0, "au-dela des 5 conserves : nettoye");
    for id in &newer {
        assert_eq!(count(*id).await, 3, "conserve");
    }

    // Projet supprime : lignes supprimees avec ses deploiements.
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(env.project)
        .execute(&env.state.db)
        .await
        .unwrap();
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM deployment_logs")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

/// Lignes enregistrees du deploiement (au moins `n` attendues, 5 s au plus).
async fn stored_lines(env: &Env, n: usize) -> Vec<String> {
    let mut stored = Vec::new();
    for _ in 0..100 {
        stored = sqlx::query_scalar::<_, String>(
            "SELECT line FROM deployment_logs WHERE deployment_id = $1 ORDER BY seq",
        )
        .bind(env.deployment)
        .fetch_all(&env.state.db)
        .await
        .unwrap();
        if stored.len() >= n {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    stored
}

#[tokio::test]
async fn multi_line_secret_masked_live_stored_and_downloaded() {
    let secrets = Arc::new(FixtureSecrets::default());
    let fixture = secrets.clone();
    let env = Env::with(move |state| {
        state.logs = LogHub::new(state.agents.clone(), fixture);
    })
    .await;
    // Cle PEM (fixture) : l'agent envoie une ligne de journal par ligne. En-tete
    // coupe par concat! : aucune ligne de la source n'imite une vraie cle.
    let pem = concat!(
        "-----BEGIN ",
        "PRIVATE KEY-----\n",
        "MIIEvQIBADANBgkqhkiG9w0B\nAQEFAASCBKcwggSjAgEAAoIB\n-----END PRIVATE KEY-----"
    );
    secrets
        .0
        .lock()
        .unwrap()
        .insert(env.project, vec![pem.to_string()]);
    let viewer = login(&env.state, "viewer").await;
    let origin = env.origin();
    let target = LogTarget::Deployment(DeploymentId(env.deployment));
    let mut ws = env
        .ws(
            &format!("deployment={}", env.deployment),
            Some(&viewer),
            Some(&origin),
        )
        .await
        .unwrap();
    let mut sent: Vec<String> = pem.lines().map(|l| format!("  {l}")).collect();
    // Valeur entiere dans une seule ligne de plusieurs lignes.
    sent.push(format!("dump: {pem}"));
    for line in &sent {
        env.line(target, LogStream::Stdout, line);
    }
    let body = ["MIIEvQIBADANBgkqhkiG9w0B", "AQEFAASCBKcwggSjAgEAAoIB"];
    for _ in &sent {
        let msg = ws.next_json().await;
        let text = msg["message"].as_str().unwrap();
        assert!(body.iter().all(|b| !text.contains(b)), "{text}");
        assert!(!text.contains("PRIVATE KEY"), "{text}");
    }

    let stored = stored_lines(&env, sent.len()).await;
    assert_eq!(stored.len(), sent.len());
    for line in &stored {
        assert!(body.iter().all(|b| !line.contains(b)), "{line}");
    }
    let (status, _, file) = env
        .raw_get(
            &format!("/api/deployments/{}/logs.txt", env.deployment),
            &viewer,
        )
        .await;
    assert_eq!(status, 200);
    let text = String::from_utf8(file).unwrap();
    assert!(body.iter().all(|b| !text.contains(b)), "{text}");
    ws.close().await;
}

#[tokio::test]
async fn secrets_changed_while_streaming_are_masked() {
    let secrets = Arc::new(FixtureSecrets::default());
    let fixture = secrets.clone();
    let mut env = Env::with(move |state| {
        state.logs = LogHub::new(state.agents.clone(), fixture);
    })
    .await;
    let viewer = login(&env.state, "viewer").await;
    let origin = env.origin();
    let service = LogTarget::Service(ServiceId(env.service));
    let deployment = LogTarget::Deployment(DeploymentId(env.deployment));
    let mut svc = env
        .ws(
            &format!("service={}", env.service),
            Some(&viewer),
            Some(&origin),
        )
        .await
        .unwrap();
    let mut dep = env
        .ws(
            &format!("deployment={}", env.deployment),
            Some(&viewer),
            Some(&origin),
        )
        .await
        .unwrap();
    env.wait_orders().await;
    env.line(service, LogStream::Stdout, "avant new-secret-value");
    assert_eq!(svc.next_json().await["message"], "avant new-secret-value");
    env.line(deployment, LogStream::Stdout, "avant new-secret-value");
    assert_eq!(dep.next_json().await["message"], "avant new-secret-value");
    assert_eq!(stored_lines(&env, 1).await.len(), 1);

    // Secret ajoute au projet pendant que les flux sont ouverts.
    secrets
        .0
        .lock()
        .unwrap()
        .insert(env.project, vec!["new-secret-value".to_string()]);
    env.state.logs.invalidate(env.project);

    env.line(service, LogStream::Stdout, "apres new-secret-value");
    assert_eq!(svc.next_json().await["message"], "apres ••••");
    env.line(deployment, LogStream::Stdout, "apres new-secret-value");
    assert_eq!(dep.next_json().await["message"], "apres ••••");
    let stored = stored_lines(&env, 2).await;
    assert_eq!(stored.get(1).map(String::as_str), Some("apres ••••"));
    svc.close().await;
    dep.close().await;
}

#[tokio::test]
async fn log_flood_never_drops_agent_events() {
    let env = Env::new().await;
    // Abonne du suivi des deploiements (orchestrateur).
    let mut events = env.state.agents.subscribe();
    let target = LogTarget::Service(ServiceId(env.service));
    for i in 0..5_000 {
        env.line(target, LogStream::Stdout, &format!("ligne {i}"));
    }
    env.agent_says(AgentToControl::DeployProgress {
        deployment_id: DeploymentId(env.deployment),
        stage: Stage::Image,
        status: StageStatus::Failed {
            message: "x".into(),
        },
    });
    let got = events.recv().await;
    assert!(
        matches!(got, Ok((_, AgentToControl::DeployProgress { .. }))),
        "{got:?}"
    );
}

#[tokio::test]
async fn unreadable_secrets_hold_lines_instead_of_leaking() {
    let secrets = Arc::new(FixtureSecrets::default());
    let fixture = secrets.clone();
    let mut env = Env::with(move |state| {
        state.logs = LogHub::new(state.agents.clone(), fixture);
    })
    .await;
    secrets
        .0
        .lock()
        .unwrap()
        .insert(env.project, vec!["held-secret-value".to_string()]);
    let viewer = login(&env.state, "viewer").await;
    let origin = env.origin();
    let service = LogTarget::Service(ServiceId(env.service));
    let deployment = LogTarget::Deployment(DeploymentId(env.deployment));
    let mut svc = env
        .ws(
            &format!("service={}", env.service),
            Some(&viewer),
            Some(&origin),
        )
        .await
        .unwrap();
    let mut dep = env
        .ws(
            &format!("deployment={}", env.deployment),
            Some(&viewer),
            Some(&origin),
        )
        .await
        .unwrap();
    env.wait_orders().await;

    // Source des secrets en panne (base indisponible) : rien n'est garde en
    // cache, les lignes sont retenues derriere un seul marqueur.
    secrets.1.store(true, Ordering::SeqCst);
    env.state.logs.invalidate(env.project);
    env.line(service, LogStream::Stdout, "un held-secret-value");
    env.line(service, LogStream::Stdout, "deux held-secret-value");
    let marker = svc.next_json().await;
    assert_eq!(marker["message"], hub::MASKING_DOWN);
    assert_eq!(marker["level"], "warn");
    env.line(deployment, LogStream::Stdout, "build held-secret-value");
    assert_eq!(dep.next_json().await["message"], hub::MASKING_DOWN);
    let stored = stored_lines(&env, 1).await;
    assert_eq!(stored, vec![hub::MASKING_DOWN.to_string()]);
    // Historique et telechargement : refuses plutot que servis en clair.
    let (status, _, body) = env
        .raw_get(
            &format!("/api/deployments/{}/logs.txt", env.deployment),
            &viewer,
        )
        .await;
    assert_eq!(status, 503);
    assert!(!String::from_utf8_lossy(&body).contains("held-secret-value"));
    assert_eq!(
        env.ws(
            &format!("deployment={}", env.deployment),
            Some(&viewer),
            Some(&origin)
        )
        .await
        .err(),
        Some(503)
    );

    // Source revenue : la ligne suivante refait l'essai et libere les
    // lignes retenues, masquees et dans l'ordre.
    secrets.1.store(false, Ordering::SeqCst);
    env.line(service, LogStream::Stdout, "trois held-secret-value");
    assert_eq!(svc.next_json().await["message"], "un ••••");
    assert_eq!(svc.next_json().await["message"], "deux ••••");
    assert_eq!(svc.next_json().await["message"], "trois ••••");
    env.line(deployment, LogStream::Stdout, "fin held-secret-value");
    assert_eq!(dep.next_json().await["message"], "build ••••");
    assert_eq!(dep.next_json().await["message"], "fin ••••");
    let stored = stored_lines(&env, 3).await;
    assert_eq!(
        stored,
        vec![
            hub::MASKING_DOWN.to_string(),
            "build ••••".to_string(),
            "fin ••••".to_string()
        ]
    );
    let all: String = stored.concat();
    assert!(!all.contains("held-secret-value"));
    svc.close().await;
    dep.close().await;
}

async fn next(sub: &mut hub::Subscription) -> String {
    tokio::time::timeout(Duration::from_secs(8), sub.next())
        .await
        .expect("ligne attendue")
        .expect("flux ouvert")
        .message
}

#[tokio::test]
async fn hung_secret_source_never_delays_other_projects() {
    let secrets = Arc::new(FixtureSecrets::default());
    let fixture = secrets.clone();
    let env = Env::with(move |state| {
        state.logs = LogHub::new(state.agents.clone(), fixture);
    })
    .await;
    let hub = env.state.logs.clone();
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    {
        let mut values = secrets.0.lock().unwrap();
        values.insert(a, vec!["secret-of-a".to_string()]);
        values.insert(b, vec!["secret-of-b".to_string()]);
    }
    secrets.2.lock().unwrap().insert(a);
    let server = ServerId(Uuid::new_v4());
    let source = |project: Uuid| Source {
        target: LogTarget::Deployment(DeploymentId::new()),
        project,
        container: "build".into(),
        follow: None,
        origin: Some(server),
    };
    let (src_a, src_b) = (source(a), source(b));
    let (target_a, target_b) = (src_a.target, src_b.target);
    let mut sub_a = hub.subscribe(src_a, 0);
    let mut sub_b = hub.subscribe(src_b, 0);
    let line = |target: LogTarget, text: &str| {
        AgentToControl::LogLine(LogLine {
            target,
            stream: LogStream::Stdout,
            timestamp_ms: 1_763_800_000_123,
            line: text.to_string(),
        })
    };

    // Source de A bloquee : la diffusion n'attend qu'une fois `INLINE_WAIT`,
    // jamais une lecture par ligne.
    let started = std::time::Instant::now();
    for n in 0..20 {
        hub.dispatch(server, &line(target_a, &format!("a{n} secret-of-a")))
            .await;
    }
    hub.dispatch(server, &line(target_b, "b secret-of-b")).await;
    assert!(
        started.elapsed() < hub::LOAD_TIMEOUT,
        "diffusion bloquee : {:?}",
        started.elapsed()
    );
    // Les lignes de B passent, masquees.
    assert_eq!(next(&mut sub_b).await, "b ••••");

    // A : un seul marqueur quand la lecture echoue, puis ses lignes,
    // masquees et dans l'ordre, des que la source repond (sans nouvelle
    // ligne).
    assert_eq!(next(&mut sub_a).await, hub::MASKING_DOWN);
    secrets.2.lock().unwrap().clear();
    for n in 0..20 {
        assert_eq!(next(&mut sub_a).await, format!("a{n} ••••"));
    }
}

/// Prochaine trame de fermeture du serveur (5 s au plus) : son code.
async fn closed_with(ws: &mut Ws) -> Option<u16> {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.0.next())
            .await
            .expect("fermeture attendue")?;
        match msg {
            Ok(Message::Close(frame)) => return frame.map(|f| u16::from(f.code)),
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}

#[tokio::test]
async fn revoked_session_and_allowlist_close_open_streams() {
    let env = Env::new().await;
    let origin = env.origin();
    let q = format!("deployment={}", env.deployment);

    // Deconnexion : le flux de cette session se ferme aussitot (1008),
    // celui d'une autre session reste ouvert.
    let gone = login(&env.state, "viewer").await;
    let kept = login(&env.state, "viewer").await;
    let mut gone_ws = env.ws(&q, Some(&gone), Some(&origin)).await.unwrap();
    let mut kept_ws = env.ws(&q, Some(&kept), Some(&origin)).await.unwrap();
    let me = call(
        &env.state,
        "GET",
        "/api/me",
        Req {
            cookie: Some(&gone),
            ..Req::default()
        },
    )
    .await;
    let csrf = me.json()["csrf_token"].as_str().unwrap().to_string();
    let r = call(
        &env.state,
        "POST",
        "/api/auth/logout",
        Req {
            cookie: Some(&gone),
            csrf: Some(&csrf),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    assert_eq!(closed_with(&mut gone_ws).await, Some(1008));

    // Toujours ouvert : une ligne arrive encore.
    env.line(
        LogTarget::Deployment(DeploymentId(env.deployment)),
        LogStream::Stdout,
        "encore ouvert",
    );
    assert_eq!(kept_ws.next_json().await["message"], "encore ouvert");

    // Liste d'IP qui exclut l'adresse du flux (127.0.0.1) : ferme aussi.
    sqlx::query(
        "INSERT INTO instance (id, name, slug, timezone, language) \
         VALUES ($1, 'Acme', 'acme', 'Europe/Zurich', 'fr')",
    )
    .bind(Uuid::new_v4())
    .execute(&env.state.db)
    .await
    .unwrap();
    let owner = login(&env.state, "owner").await;
    let me = call(
        &env.state,
        "GET",
        "/api/me",
        Req {
            cookie: Some(&owner),
            ..Req::default()
        },
    )
    .await;
    let csrf = me.json()["csrf_token"].as_str().unwrap().to_string();
    let r = call(
        &env.state,
        "PATCH",
        "/api/settings/security",
        Req {
            cookie: Some(&owner),
            csrf: Some(&csrf),
            body: Some(json!({ "ip_allowlist": ["203.0.113.0/24"], "current_password": PASSWORD })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(closed_with(&mut kept_ws).await, Some(1008));
}

/// 2FA imposee : le flux ouvert par une session sans second facteur, basculee
/// en enrolement obligatoire, se ferme (1008).
#[tokio::test]
async fn enforced_two_factor_closes_open_streams_of_switched_sessions() {
    let env = Env::new().await;
    let origin = env.origin();
    let q = format!("deployment={}", env.deployment);
    sqlx::query(
        "INSERT INTO instance (id, name, slug, timezone, language)          VALUES ($1, 'Acme', 'acme', 'Europe/Zurich', 'fr')",
    )
    .bind(Uuid::new_v4())
    .execute(&env.state.db)
    .await
    .unwrap();
    let viewer = login(&env.state, "viewer").await;
    let mut ws = env.ws(&q, Some(&viewer), Some(&origin)).await.unwrap();
    let owner = login(&env.state, "owner").await;
    let me = call(
        &env.state,
        "GET",
        "/api/me",
        Req {
            cookie: Some(&owner),
            ..Req::default()
        },
    )
    .await;
    let csrf = me.json()["csrf_token"].as_str().unwrap().to_string();
    let r = call(
        &env.state,
        "PATCH",
        "/api/settings/security",
        Req {
            cookie: Some(&owner),
            csrf: Some(&csrf),
            body: Some(json!({ "mfa_required": true })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(closed_with(&mut ws).await, Some(1008));
}

/// Une ligne n'est acceptee que de l'agent du serveur de sa cible : ni
/// diffusee, ni enregistree si un autre agent l'envoie.
#[tokio::test]
async fn log_lines_from_another_agent_are_ignored() {
    let env = Env::new().await;
    let hub = env.state.logs.clone();
    let target = LogTarget::Deployment(DeploymentId(env.deployment));
    let mut sub = hub.subscribe(
        Source {
            target,
            project: env.project,
            container: "build".into(),
            follow: None,
            origin: Some(ServerId(env.server)),
        },
        0,
    );
    let intruder = ServerId(Uuid::new_v4());
    let line = |text: &str| {
        AgentToControl::LogLine(LogLine {
            target,
            stream: LogStream::Stdout,
            timestamp_ms: 1_763_800_000_123,
            line: text.to_string(),
        })
    };
    // Diffusion directe (hub) puis par le registre (enregistrement).
    hub.dispatch(intruder, &line("fausse ligne")).await;
    env.state.agents.publish(intruder, line("fausse ligne"));
    hub.dispatch(ServerId(env.server), &line("vraie ligne"))
        .await;
    env.line(target, LogStream::Stdout, "vraie ligne");
    assert_eq!(next(&mut sub).await, "vraie ligne");
    let stored = stored_lines(&env, 1).await;
    // Laisse au besoin le temps a une ligne en trop d'arriver.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let stored_after = stored_lines(&env, 1).await;
    assert_eq!(stored, vec!["vraie ligne".to_string()]);
    assert_eq!(
        stored_after, stored,
        "ligne d'un autre agent jamais enregistree"
    );
}

/// Telechargements de `logs.txt` simultanes bornes : au-dela, 429.
#[tokio::test]
async fn logs_txt_downloads_are_throttled() {
    let env = Env::new().await;
    let viewer = login(&env.state, "viewer").await;
    let path = format!("/api/deployments/{}/logs.txt", env.deployment);
    let held: Vec<_> = (0..hub::MAX_DOWNLOADS)
        .map(|_| env.state.logs.try_download().expect("place libre"))
        .collect();
    let (status, head, _) = env.raw_get(&path, &viewer).await;
    assert_eq!(status, 429, "{head}");
    assert!(head.to_ascii_lowercase().contains("retry-after:"), "{head}");
    drop(held);
    let (status, head, _) = env.raw_get(&path, &viewer).await;
    assert_eq!(status, 200, "{head}");
}

/// Base indisponible pendant qu'un autre agent envoie des lignes : elles
/// sont retenues avec leur serveur, puis reverifiees au retour de la base
/// (jamais enregistrees sans le controle du serveur).
#[tokio::test]
async fn foreign_lines_held_during_an_outage_are_rechecked() {
    let env = Env::new().await;
    let target = LogTarget::Deployment(DeploymentId(env.deployment));
    let intruder = ServerId(Uuid::new_v4());
    // Lecture des deploiements en echec (table renommee).
    sqlx::query("ALTER TABLE deployments RENAME TO deployments_hors_service")
        .execute(&env.state.db)
        .await
        .unwrap();
    env.state.agents.publish(
        intruder,
        AgentToControl::LogLine(LogLine {
            target,
            stream: LogStream::Stdout,
            timestamp_ms: 1_763_800_000_100,
            line: "fausse ligne".into(),
        }),
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    sqlx::query("ALTER TABLE deployments_hors_service RENAME TO deployments")
        .execute(&env.state.db)
        .await
        .unwrap();
    env.line(target, LogStream::Stdout, "vraie ligne");
    let stored = stored_lines(&env, 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let stored_after = stored_lines(&env, 1).await;
    assert_eq!(stored, vec!["vraie ligne".to_string()]);
    assert_eq!(stored_after, stored, "ligne d'un autre agent enregistree");
}

/// Retrait d'un membre (API Equipe) : son flux de logs ouvert se ferme
/// (1008), celui des autres reste ouvert, et son cookie ne sert plus.
#[tokio::test]
async fn removed_member_sessions_and_ws_closed() {
    let env = Env::new().await;
    let origin = env.origin();
    let q = format!("deployment={}", env.deployment);
    let member = login(&env.state, "viewer").await;
    let owner = login(&env.state, "owner").await;
    let mut member_ws = env.ws(&q, Some(&member), Some(&origin)).await.unwrap();
    let mut owner_ws = env.ws(&q, Some(&owner), Some(&origin)).await.unwrap();

    let member_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE role = 'viewer'")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    let me = call(
        &env.state,
        "GET",
        "/api/me",
        Req {
            cookie: Some(&owner),
            ..Req::default()
        },
    )
    .await;
    let csrf = me.json()["csrf_token"].as_str().unwrap().to_string();
    let r = call(
        &env.state,
        "DELETE",
        &format!("/api/team/members/{member_id}"),
        Req {
            cookie: Some(&owner),
            csrf: Some(&csrf),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(closed_with(&mut member_ws).await, Some(1008));

    // Le flux du proprietaire reste ouvert ; le cookie du membre est mort.
    env.line(
        LogTarget::Deployment(DeploymentId(env.deployment)),
        LogStream::Stdout,
        "encore ouvert",
    );
    assert_eq!(owner_ws.next_json().await["message"], "encore ouvert");
    assert_eq!(
        env.ws(&q, Some(&member), Some(&origin)).await.err(),
        Some(401)
    );
}
