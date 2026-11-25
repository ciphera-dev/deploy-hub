mod common;

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use control_plane::app::build_router;
use control_plane::auth::password;
use control_plane::http::client_ip::ProxyListener;
use control_plane::state::AppState;
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const EMAIL: &str = "Alice@Example.com";
const PASSWORD: &str = "correct horse battery staple";
const DIRECT: &str = "203.0.113.7:50000";

struct Env {
    state: AppState,
    user_id: Uuid,
    _pg: common::TestDb,
}

async fn env() -> Env {
    let (state, pg) = common::state().await;
    let user_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, 'admin', 'Alice Martin', now())",
    )
    .bind(user_id)
    .bind(EMAIL)
    .bind(password::hash(PASSWORD).unwrap())
    .execute(&state.db)
    .await
    .unwrap();
    Env {
        state,
        user_id,
        _pg: pg,
    }
}

#[derive(Default)]
struct Req<'a> {
    cookie: Option<&'a str>,
    csrf: Option<&'a str>,
    peer: Option<&'a str>,
    /// Requete recue par l'ecoute locale du proxy.
    proxy: bool,
    xff: Option<&'a str>,
    body: Option<Value>,
}

struct Reply {
    status: u16,
    set_cookie: Option<String>,
    retry_after: Option<String>,
    raw: String,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.raw).unwrap_or(Value::Null)
    }

    /// Valeur du cookie de session pose par la reponse.
    fn token(&self) -> String {
        let c = self.set_cookie.as_deref().expect("Set-Cookie");
        let v = c
            .strip_prefix("__Host-ciphera_session=")
            .expect("nom du cookie");
        v.split(';').next().unwrap().to_string()
    }
}

async fn call(state: &AppState, method: &str, path: &str, r: Req<'_>) -> Reply {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(c) = r.cookie {
        req = req.header(
            header::COOKIE,
            format!("theme=dark; __Host-ciphera_session={c}"),
        );
    }
    if let Some(t) = r.csrf {
        req = req.header("x-csrf-token", t);
    }
    if let Some(x) = r.xff {
        req = req.header("x-forwarded-for", x);
    }
    let body = match r.body {
        Some(b) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let mut req = req.body(body).unwrap();
    let peer: SocketAddr = r.peer.unwrap_or(DIRECT).parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    if r.proxy {
        req.extensions_mut().insert(ProxyListener);
    }
    let res = build_router(state.clone()).oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    // Bloc : l'emprunt de la reponse se termine avant l'attente (futur Send).
    let (set_cookie, retry_after) = {
        let get = |name| {
            res.headers()
                .get(name)
                .map(|v: &header::HeaderValue| v.to_str().unwrap().to_string())
        };
        (get(header::SET_COOKIE), get(header::RETRY_AFTER))
    };
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    Reply {
        status,
        set_cookie,
        retry_after,
        raw: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

async fn login_as(state: &AppState, email: &str, pw: &str, r: Req<'_>) -> Reply {
    let body = json!({ "email": email, "password": pw, "remember": false });
    call(
        state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(body),
            ..r
        },
    )
    .await
}

async fn login(state: &AppState) -> Reply {
    login_as(state, "alice@example.com", PASSWORD, Req::default()).await
}

async fn me(state: &AppState, token: &str) -> Reply {
    call(
        state,
        "GET",
        "/api/me",
        Req {
            cookie: Some(token),
            ..Req::default()
        },
    )
    .await
}

async fn audit_actions(state: &AppState) -> Vec<(String, Value)> {
    sqlx::query_as("SELECT action, detail FROM audit_events ORDER BY at, action")
        .fetch_all(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn login_ok_sets_secure_cookie() {
    let env = env().await;
    let r = login(&env.state).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.json()["mfa_required"], false);
    let csrf = r.json()["csrf_token"].as_str().unwrap().to_string();
    assert!(csrf.len() >= 32);

    let cookie = r.set_cookie.clone().unwrap();
    for attr in [
        "__Host-ciphera_session=",
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/",
    ] {
        assert!(cookie.contains(attr), "{attr} absent de {cookie}");
    }
    assert!(!cookie.contains("Domain"));
    assert!(
        !cookie.contains("Max-Age"),
        "sans « remember » : cookie de session du navigateur"
    );
    let hours: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM expires_at - now())::float8 / 3600 FROM sessions",
    )
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert!((11.9..=12.0).contains(&hours), "{hours}");

    // La session donne acces a /api/me, avec le meme jeton CSRF.
    let m = me(&env.state, &r.token()).await;
    assert_eq!(m.status, 200, "{}", m.raw);
    let body = m.json();
    assert_eq!(body["id"], env.user_id.to_string());
    assert_eq!(body["email"], EMAIL);
    assert_eq!(body["name"], "Alice Martin");
    assert_eq!(body["role"], "admin");
    assert_eq!(body["csrf_token"], csrf);

    let audit = audit_actions(&env.state).await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].0, "auth.login.success");

    // « remember » : 30 jours, cookie persistant.
    let r = call(
        &env.state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": EMAIL, "password": PASSWORD, "remember": true })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(r.set_cookie.unwrap().contains("Max-Age=2592000"));
}

#[tokio::test]
async fn me_without_session_401() {
    let env = env().await;
    let r = call(&env.state, "GET", "/api/me", Req::default()).await;
    assert_eq!(r.status, 401);
    assert_eq!(r.json()["error"]["code"], "unauthenticated");
    let r = me(&env.state, "jeton-inconnu").await;
    assert_eq!(r.status, 401);
    assert_eq!(r.json()["error"]["code"], "unauthenticated");
}

#[tokio::test]
async fn login_wrong_password_401_generic() {
    let env = env().await;
    let wrong = login_as(
        &env.state,
        "alice@example.com",
        "mauvais mot de passe",
        Req::default(),
    )
    .await;
    let unknown = login_as(&env.state, "bob@example.com", PASSWORD, Req::default()).await;
    for r in [&wrong, &unknown] {
        assert_eq!(r.status, 401, "{}", r.raw);
        assert!(r.set_cookie.is_none());
        assert_eq!(r.json()["error"]["code"], "invalid_credentials");
    }
    assert_eq!(wrong.raw, unknown.raw, "meme reponse : pas d'enumeration");

    let audit = audit_actions(&env.state).await;
    assert_eq!(audit.len(), 2);
    for (action, detail) in &audit {
        assert_eq!(action, "auth.login.failure");
        let d = detail.to_string();
        assert!(!d.contains("mauvais") && !d.contains(PASSWORD));
        assert!(!d.to_lowercase().contains("example.com"));
    }
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
}

async fn fail(state: &AppState, email: &str, r: Req<'_>) -> Reply {
    login_as(state, email, "mauvais mot de passe", r).await
}

fn assert_limited(r: &Reply) {
    assert_eq!(r.status, 429, "{}", r.raw);
    assert_eq!(r.json()["error"]["code"], "rate_limited");
    let secs: u64 = r
        .retry_after
        .as_deref()
        .expect("Retry-After")
        .parse()
        .unwrap();
    assert!((1..=900).contains(&secs), "{secs}");
}

#[tokio::test]
async fn login_rate_limited_after_5_failures_per_15min_per_ip_and_email() {
    let env = env().await;
    for _ in 0..5 {
        assert_eq!(fail(&env.state, EMAIL, Req::default()).await.status, 401);
    }
    // 6e tentative : bloquee, meme avec le bon mot de passe (casse differente de l'e-mail).
    assert_limited(&login(&env.state).await);
    // Autre e-mail depuis la meme IP : pas concerne par ce compteur.
    assert_eq!(
        fail(&env.state, "bob@example.com", Req::default())
            .await
            .status,
        401
    );
    // Meme e-mail depuis une autre IP : pas concerne non plus.
    let other = Req {
        peer: Some("198.51.100.9:4000"),
        ..Req::default()
    };
    assert_eq!(
        login_as(&env.state, EMAIL, PASSWORD, other).await.status,
        200
    );

    // Une fenetre echue ne compte plus.
    sqlx::query("UPDATE login_attempts SET at = at - interval '16 minutes'")
        .execute(&env.state.db)
        .await
        .unwrap();
    assert_eq!(login(&env.state).await.status, 200);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_burst_cannot_exceed_the_limit() {
    let env = env().await;
    // 10 mauvais mots de passe en parallele pour le meme couple (IP, e-mail) :
    // une seule connexion par compte est en vol, les autres sont refusees.
    let tasks: Vec<_> = (0..10)
        .map(|_| {
            let state = env.state.clone();
            tokio::spawn(async move { fail(&state, EMAIL, Req::default()).await.status })
        })
        .collect();
    let mut statuses = Vec::new();
    for t in tasks {
        statuses.push(t.await.unwrap());
    }
    let reached = statuses.iter().filter(|s| **s == 401).count();
    let limited = statuses.iter().filter(|s| **s == 429).count();
    assert!((1..=5).contains(&reached), "{statuses:?}");
    assert_eq!(reached + limited, 10, "{statuses:?}");
    let recorded: i64 = sqlx::query_scalar("SELECT count(*) FROM login_attempts")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(recorded, reached as i64);
}

/// L'atomicite de `admit` par couple (IP, e-mail) : 10 admissions simultanees,
/// 5 seulement sont acceptees.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn admit_is_atomic_per_ip_and_email_pair() {
    use control_plane::auth::rate_limit::{Admission, IpKey, admit, email_key};
    let env = env().await;
    let ip = IpKey::login("203.0.113.7".parse().unwrap());
    let key = email_key("alice@example.com");
    let tasks: Vec<_> = (0..10)
        .map(|_| {
            let (db, ip, key) = (env.state.db.clone(), ip.clone(), key.clone());
            tokio::spawn(async move { admit(&db, &ip, &key).await.unwrap() })
        })
        .collect();
    let mut allowed = 0;
    for t in tasks {
        if matches!(t.await.unwrap(), Admission::Allowed(_)) {
            allowed += 1;
        }
    }
    assert_eq!(allowed, 5);
}

/// Le delai croissant ne tient pas la place de calcul : pendant qu'une requete
/// fautive dort, le titulaire n'est pas refuse (`busy`) et se connecte.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failure_delay_does_not_lock_the_owner_out() {
    let env = env().await;
    sqlx::query(
        "INSERT INTO login_attempts (ip, email_hash, succeeded)          SELECT '198.51.100.' || g, $1, false FROM generate_series(1, 15) g",
    )
    .bind(control_plane::auth::rate_limit::email_key("alice@example.com"))
    .execute(&env.state.db)
    .await
    .unwrap();
    // 15 echecs recents : delai de 6 s pour chaque tentative.
    let state = env.state.clone();
    let attacker = tokio::spawn(async move {
        let other = Req {
            peer: Some("198.51.100.9:4000"),
            ..Req::default()
        };
        fail(&state, EMAIL, other).await.status
    });
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    // La requete fautive dort : sous l'ancien schema elle tenait la place et le
    // titulaire recevait 429 `busy` pendant tout ce temps (plus de 4 s ici,
    // bien au-dela des reessais). Une collision brève avec le calcul Argon2 de
    // l'autre requete reste admise : on reessaie sur `busy`.
    let mut r = login(&env.state).await;
    for _ in 0..15 {
        if r.status != 429 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        r = login(&env.state).await;
    }
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(attacker.await.unwrap(), 401);
}

/// Par compte, un seul calcul Argon2 de connexion en vol : les connexions
/// simultanees sur le meme e-mail recoivent 429 `busy` sans etre comptees
/// comme des echecs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn per_account_argon2_single_flight() {
    let env = env().await;
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let state = env.state.clone();
            tokio::spawn(async move {
                let r = fail(&state, EMAIL, Req::default()).await;
                (r.status, r.json()["error"]["code"].clone())
            })
        })
        .collect();
    let mut replies = Vec::new();
    for t in tasks {
        replies.push(t.await.unwrap());
    }
    let passed = replies.iter().filter(|(s, _)| *s == 401).count();
    let busy = replies
        .iter()
        .filter(|(s, code)| *s == 429 && code == "busy")
        .count();
    assert!(passed >= 1, "{replies:?}");
    assert!(busy >= 1, "{replies:?}");
    assert_eq!(passed + busy, 8, "{replies:?}");
    let recorded: i64 = sqlx::query_scalar("SELECT count(*) FROM login_attempts")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(recorded, passed as i64);

    // Un autre compte n'est pas concerne : sa connexion passe pendant ce temps.
    assert_eq!(
        fail(&env.state, "bob@example.com", Req::default())
            .await
            .status,
        401
    );
}

/// Au-dela de 10 echecs de mot de passe en 15 minutes sur le compte (toutes
/// IP), chaque tentative est ralentie d'une seconde de plus, sans blocage :
/// le bon mot de passe passe encore.
#[tokio::test]
async fn login_slows_down_after_ten_failures_without_blocking() {
    let env = env().await;
    sqlx::query(
        "INSERT INTO login_attempts (ip, email_hash, succeeded)          SELECT '198.51.100.' || g, $1, false FROM generate_series(1, 10) g",
    )
    .bind(control_plane::auth::rate_limit::email_key("alice@example.com"))
    .execute(&env.state.db)
    .await
    .unwrap();
    let started = std::time::Instant::now();
    let r = login(&env.state).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert!(
        started.elapsed() >= std::time::Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    // Le succes du titulaire remet le compteur du compte a zero.
    let failures = control_plane::auth::rate_limit::recent_login_failures(
        &env.state.db,
        &control_plane::auth::rate_limit::email_key("alice@example.com"),
    )
    .await
    .unwrap();
    assert_eq!(failures, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_burst_cannot_exceed_the_ip_cap() {
    let env = env().await;
    // 48 echecs deja enregistres depuis l'IP : il reste 2 places sous le plafond de 50.
    sqlx::query(
        "INSERT INTO login_attempts (ip, email_hash, succeeded)          SELECT '203.0.113.7', sha256(('x' || g)::bytea), false FROM generate_series(1, 48) g",
    )
    .execute(&env.state.db)
    .await
    .unwrap();
    let tasks: Vec<_> = (0..6)
        .map(|i| {
            let state = env.state.clone();
            tokio::spawn(async move {
                let email = format!("autre{i}@example.com");
                fail(&state, &email, Req::default()).await.status
            })
        })
        .collect();
    let mut statuses = Vec::new();
    for t in tasks {
        statuses.push(t.await.unwrap());
    }
    assert_eq!(
        statuses.iter().filter(|s| **s == 401).count(),
        2,
        "{statuses:?}"
    );
    assert_eq!(
        statuses.iter().filter(|s| **s == 429).count(),
        4,
        "{statuses:?}"
    );
}

#[tokio::test]
async fn login_success_resets_ip_email_counter() {
    let env = env().await;
    for _ in 0..4 {
        assert_eq!(fail(&env.state, EMAIL, Req::default()).await.status, 401);
    }
    assert_eq!(login(&env.state).await.status, 200);
    for _ in 0..4 {
        assert_eq!(fail(&env.state, EMAIL, Req::default()).await.status, 401);
    }
    assert_eq!(login(&env.state).await.status, 200);
}

#[tokio::test]
async fn login_rate_limited_per_ip_overall() {
    let env = env().await;
    // 50 echecs recents depuis l'IP, chacun sur un e-mail different.
    sqlx::query(
        "INSERT INTO login_attempts (ip, email_hash, succeeded) \
         SELECT '203.0.113.7', sha256(('x' || g)::bytea), false FROM generate_series(1, 50) g",
    )
    .execute(&env.state.db)
    .await
    .unwrap();
    assert_limited(&login(&env.state).await);
}

#[tokio::test]
async fn ipv6_clients_are_rate_limited_per_slash_64() {
    let env = env().await;
    // Cinq echecs depuis cinq adresses du meme /64.
    for i in 1..=5 {
        let peer = format!("[2001:db8:1:2::{i:x}]:4000");
        let r = fail(
            &env.state,
            EMAIL,
            Req {
                peer: Some(&peer),
                ..Req::default()
            },
        )
        .await;
        assert_eq!(r.status, 401, "{}", r.raw);
    }
    let same_64 = Req {
        peer: Some("[2001:db8:1:2:abcd:ef01:2345:6789]:4000"),
        ..Req::default()
    };
    assert_limited(&login_as(&env.state, EMAIL, PASSWORD, same_64).await);
    let other_64 = Req {
        peer: Some("[2001:db8:1:3::1]:4000"),
        ..Req::default()
    };
    assert_eq!(
        login_as(&env.state, EMAIL, PASSWORD, other_64).await.status,
        200
    );
}

#[tokio::test]
async fn forgot_password_spam_does_not_block_logins_from_the_same_ip() {
    let env = env().await;
    // Demandes « mot de passe oublie » jusqu'au plafond par IP (jamais
    // marquees reussies), chacune pour un autre e-mail.
    let mut limited = false;
    for i in 0..60 {
        let r = call(
            &env.state,
            "POST",
            "/api/auth/password/forgot",
            Req {
                body: Some(json!({ "email": format!("spam{i}@example.com") })),
                ..Req::default()
            },
        )
        .await;
        match r.status {
            202 => {}
            429 => {
                limited = true;
                break;
            }
            other => panic!("statut inattendu {other} : {}", r.raw),
        }
    }
    assert!(limited, "plafond par IP des demandes atteint");
    // La connexion depuis la meme IP n'est pas concernee.
    assert_eq!(login(&env.state).await.status, 200);
}

#[tokio::test]
async fn xff_ignored_from_non_proxy_peer() {
    let env = env().await;
    // Client direct : chaque requete annonce une autre IP dans X-Forwarded-For.
    for i in 0..5 {
        let xff = format!("192.0.2.{i}");
        let r = fail(
            &env.state,
            EMAIL,
            Req {
                xff: Some(&xff),
                ..Req::default()
            },
        )
        .await;
        assert_eq!(r.status, 401);
    }
    let r = login_as(
        &env.state,
        EMAIL,
        PASSWORD,
        Req {
            xff: Some("192.0.2.200"),
            ..Req::default()
        },
    )
    .await;
    assert_limited(&r);

    // Pair en boucle locale sur l'acces direct (8443) : l'en-tete n'est pas
    // cru non plus, toutes ces requetes comptent pour 127.0.0.1.
    for i in 0..5 {
        let xff = format!("198.51.100.{}", 100 + i);
        let r = fail(
            &env.state,
            EMAIL,
            Req {
                peer: Some("127.0.0.1:41000"),
                xff: Some(&xff),
                ..Req::default()
            },
        )
        .await;
        assert_eq!(r.status, 401);
    }
    let r = login_as(
        &env.state,
        EMAIL,
        PASSWORD,
        Req {
            peer: Some("127.0.0.1:41000"),
            xff: Some("198.51.100.200"),
            ..Req::default()
        },
    )
    .await;
    assert_limited(&r);

    // Depuis l'ecoute locale du proxy, l'en-tete fait foi.
    for _ in 0..5 {
        let r = fail(
            &env.state,
            EMAIL,
            Req {
                peer: Some("127.0.0.1:41000"),
                proxy: true,
                xff: Some("198.51.100.1"),
                ..Req::default()
            },
        )
        .await;
        assert_eq!(r.status, 401);
    }
    let via_proxy = |xff| Req {
        peer: Some("127.0.0.1:41000"),
        proxy: true,
        xff: Some(xff),
        ..Req::default()
    };
    assert_limited(&login_as(&env.state, EMAIL, PASSWORD, via_proxy("198.51.100.1")).await);
    assert_eq!(
        login_as(&env.state, EMAIL, PASSWORD, via_proxy("198.51.100.2"))
            .await
            .status,
        200
    );
}

#[tokio::test]
async fn post_without_csrf_403() {
    let env = env().await;
    let a = login(&env.state).await;
    let b = login_as(
        &env.state,
        EMAIL,
        PASSWORD,
        Req {
            peer: Some("198.51.100.9:4000"),
            ..Req::default()
        },
    )
    .await;
    let (token, csrf) = (
        a.token(),
        a.json()["csrf_token"].as_str().unwrap().to_string(),
    );
    let other_csrf = b.json()["csrf_token"].as_str().unwrap().to_string();
    assert_ne!(csrf, other_csrf, "jeton lie a la session");

    for bad in [None, Some(""), Some("x"), Some(other_csrf.as_str())] {
        let r = call(
            &env.state,
            "POST",
            "/api/auth/logout",
            Req {
                cookie: Some(&token),
                csrf: bad,
                ..Req::default()
            },
        )
        .await;
        assert_eq!(r.status, 403, "{bad:?}");
        assert_eq!(r.json()["error"]["code"], "csrf");
    }
    // Aucune ecriture sous /api sans jeton, meme sur une route inconnue.
    let r = call(&env.state, "DELETE", "/api/projects/x", Req::default()).await;
    assert_eq!(r.status, 403);
    // La session est toujours valide, puis le bon jeton passe.
    assert_eq!(me(&env.state, &token).await.status, 200);
    let r = call(
        &env.state,
        "POST",
        "/api/auth/logout",
        Req {
            cookie: Some(&token),
            csrf: Some(&csrf),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
}

#[tokio::test]
async fn logout_invalidates_session() {
    let env = env().await;
    let r = login(&env.state).await;
    let (token, csrf) = (
        r.token(),
        r.json()["csrf_token"].as_str().unwrap().to_string(),
    );
    let out = call(
        &env.state,
        "POST",
        "/api/auth/logout",
        Req {
            cookie: Some(&token),
            csrf: Some(&csrf),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(out.status, 204, "{}", out.raw);
    let cleared = out.set_cookie.expect("cookie efface");
    assert!(cleared.starts_with("__Host-ciphera_session=;"));
    assert!(cleared.contains("Max-Age=0"));

    let m = me(&env.state, &token).await;
    assert_eq!(m.status, 401);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(left, 0);
    let audit = audit_actions(&env.state).await;
    assert_eq!(audit.last().unwrap().0, "auth.logout");
}

#[tokio::test]
async fn session_id_rotated_after_login_and_mfa() {
    let env = env().await;
    // Login : un cookie deja present (session anterieure) est remplace et supprime.
    let first = login(&env.state).await;
    let old = first.token();
    let second = login_as(
        &env.state,
        EMAIL,
        PASSWORD,
        Req {
            cookie: Some(&old),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(second.status, 200);
    let new = second.token();
    assert_ne!(old, new);
    assert_eq!(me(&env.state, &old).await.status, 401);
    assert_eq!(me(&env.state, &new).await.status, 200);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(count, 1);

    // MFA : enrolement TOTP depuis la session complete, puis connexion
    // (session en attente de MFA) et verification : un nouveau jeton a chaque
    // changement de niveau.
    let csrf = second.json()["csrf_token"].as_str().unwrap().to_string();
    let with_session = |body| Req {
        cookie: Some(&new),
        csrf: Some(&csrf),
        body: Some(body),
        ..Req::default()
    };
    let start = call(
        &env.state,
        "POST",
        "/api/me/mfa/totp/start",
        with_session(json!({ "current_password": PASSWORD })),
    )
    .await;
    assert_eq!(start.status, 200, "{}", start.raw);
    let uri = start.json()["otpauth_uri"].as_str().unwrap().to_string();
    let secret = uri
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("secret="))
        .unwrap();
    let totp = totp_rs::Builder::new()
        .with_secret(totp_rs::Secret::try_from_base32(secret).unwrap())
        .build()
        .unwrap();
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    };
    if now() % 30 >= 26 {
        tokio::time::sleep(std::time::Duration::from_secs(31 - now() % 30)).await;
    }
    let code = totp.generate(now()).to_string();
    let confirmed = call(
        &env.state,
        "POST",
        "/api/me/mfa/totp/confirm",
        with_session(json!({ "code": code })),
    )
    .await;
    assert_eq!(confirmed.status, 200, "{}", confirmed.raw);
    let enrolled = confirmed.token();
    assert_ne!(enrolled, new, "rotation apres l'enrolement");
    assert_eq!(me(&env.state, &new).await.status, 401);
    assert_eq!(me(&env.state, &enrolled).await.status, 200);
    // Comme si le code d'enrolement datait d'une fenetre echue.
    sqlx::query("UPDATE user_totp SET last_used_step = NULL")
        .execute(&env.state.db)
        .await
        .unwrap();

    let login_pending = call(
        &env.state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": EMAIL, "password": PASSWORD, "remember": true })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(login_pending.json()["mfa_required"], true);
    let pending = login_pending.token();
    let m = me(&env.state, &pending).await;
    assert_eq!(m.status, 401);
    assert_eq!(m.json()["error"]["code"], "mfa_required");

    let verified = call(
        &env.state,
        "POST",
        "/api/auth/mfa/verify",
        Req {
            cookie: Some(&pending),
            body: Some(json!({ "code": code })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(verified.status, 200, "{}", verified.raw);
    let rotated = verified.token();
    assert_ne!(rotated, pending);
    assert_ne!(
        verified.json()["csrf_token"],
        login_pending.json()["csrf_token"],
        "nouveau jeton CSRF"
    );
    assert_eq!(me(&env.state, &pending).await.status, 401);
    assert_eq!(me(&env.state, &rotated).await.status, 200);
    assert!(
        verified.set_cookie.unwrap().contains("Max-Age=2592000"),
        "la rotation garde la duree"
    );
}

/// Sortie de tous les journaux `tracing` du binaire de test.
fn captured_logs() -> Arc<Mutex<Vec<u8>>> {
    static LOGS: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    LOGS.get_or_init(|| {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(move || Sink(sink.clone()))
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect("abonne tracing");
        buf
    })
    .clone()
}

struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn password_never_logged() {
    let logs = captured_logs();
    let env = env().await;
    tracing::info!("debut de la capture");
    assert_eq!(login(&env.state).await.status, 200);
    let wrong = "un-autre-mot-de-passe-secret";
    assert_eq!(
        login_as(&env.state, EMAIL, wrong, Req::default())
            .await
            .status,
        401
    );
    assert_eq!(
        login_as(&env.state, "inconnu@example.com", wrong, Req::default())
            .await
            .status,
        401
    );
    let out = String::from_utf8_lossy(&logs.lock().unwrap()).into_owned();
    assert!(out.contains("debut de la capture"), "capture active");
    assert!(!out.contains(PASSWORD));
    assert!(!out.contains(wrong));
    assert!(!out.to_lowercase().contains("example.com"), "aucun e-mail");
}

#[tokio::test]
async fn failed_login_audit_purged() {
    let (state, _pg) = common::state().await;
    let insert = |action: &'static str, days: i32| {
        let db = state.db.clone();
        async move {
            sqlx::query(
                "INSERT INTO audit_events (id, at, action, detail) \
                 VALUES ($1, now() - make_interval(days => $2), $3, '{}'::jsonb)",
            )
            .bind(Uuid::new_v4())
            .bind(days)
            .bind(action)
            .execute(&db)
            .await
            .unwrap();
        }
    };
    for action in control_plane::auth::audit_purge::FAILED_LOGIN_ACTIONS {
        insert(action, 91).await;
        insert(action, 89).await;
    }
    // Les autres evenements anciens restent.
    insert("organization.delete", 400).await;
    insert("auth.login.success", 400).await;

    let removed = control_plane::auth::audit_purge::purge(&state.db)
        .await
        .unwrap();
    assert_eq!(removed, 4);
    let left: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM audit_events WHERE at < now() - interval '90 days' ORDER BY action",
    )
    .fetch_all(&state.db)
    .await
    .unwrap();
    assert_eq!(left, vec!["auth.login.success", "organization.delete"]);
    let recent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE at > now() - interval '90 days'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap();
    assert_eq!(recent, 4);
}
