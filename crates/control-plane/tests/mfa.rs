mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::mail::{Delivery, OutgoingMail};
use control_plane::state::AppState;
use serde_json::{Value, json};
use shared::config::Config;
use totp_rs::{Builder, Secret, Totp};
use uuid::Uuid;

const EMAIL: &str = "alice@example.com";
const PASSWORD: &str = "correct horse battery staple";

struct Env {
    state: AppState,
    user_id: Uuid,
    _pg: common::TestDb,
}

async fn env() -> Env {
    let (state, pg) = common::state().await;
    let user_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name) VALUES ($1, $2, $3, 'admin', 'Alice Martin')",
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

/// Session ouverte par le mot de passe : (jeton, jeton CSRF, mfa_required).
async fn login(state: &AppState) -> (String, String, bool) {
    let r = call(
        state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": EMAIL, "password": PASSWORD })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let mfa = r.json()["mfa_required"].as_bool().unwrap();
    (r.token(), r.csrf(), mfa)
}

async fn post(state: &AppState, path: &str, token: &str, csrf: Option<&str>, body: Value) -> Reply {
    call(
        state,
        "POST",
        path,
        Req {
            cookie: Some(token),
            csrf,
            body: Some(body),
            ..Req::default()
        },
    )
    .await
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

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Evite de calculer un code juste avant un changement de fenetre de 30 s.
async fn away_from_step_boundary() {
    let into = now() % 30;
    if into >= 26 {
        tokio::time::sleep(Duration::from_secs(31 - into)).await;
    }
}

fn totp_from_uri(uri: &str) -> Totp {
    let secret = uri
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("secret="))
        .expect("secret dans l'URI otpauth");
    Builder::new()
        .with_secret(Secret::try_from_base32(secret).unwrap())
        .with_skew(0)
        .build()
        .unwrap()
}

/// Code de la fenetre `offset` (0 = courante, -1 = precedente...).
fn code(totp: &Totp, offset: i64) -> String {
    let t = (now() as i64 + offset * 30) as u64;
    totp.generate(t).to_string()
}

struct Enrolled {
    totp: Totp,
    recovery_codes: Vec<String>,
    token: String,
    csrf: String,
}

/// Enrole la TOTP depuis une session complete ; renvoie la session tournee.
async fn enrol(state: &AppState, token: &str, csrf: &str) -> Enrolled {
    let start = post(
        state,
        "/api/me/mfa/totp/start",
        token,
        Some(csrf),
        json!({ "current_password": PASSWORD }),
    )
    .await;
    assert_eq!(start.status, 200, "{}", start.raw);
    let totp = totp_from_uri(start.json()["otpauth_uri"].as_str().unwrap());
    away_from_step_boundary().await;
    let confirm = post(
        state,
        "/api/me/mfa/totp/confirm",
        token,
        Some(csrf),
        json!({ "code": code(&totp, 0) }),
    )
    .await;
    assert_eq!(confirm.status, 200, "{}", confirm.raw);
    let recovery_codes = confirm.json()["recovery_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    Enrolled {
        totp,
        recovery_codes,
        token: confirm.token(),
        csrf: confirm.csrf(),
    }
}

/// Oublie la derniere fenetre utilisee (comme si le code d'enrolement etait ancien).
async fn forget_last_step(state: &AppState) {
    sqlx::query("UPDATE user_totp SET last_used_step = NULL")
        .execute(&state.db)
        .await
        .unwrap();
}

async fn enrolled_env() -> (Env, Enrolled) {
    let env = env().await;
    let (token, csrf, mfa) = login(&env.state).await;
    assert!(!mfa);
    let e = enrol(&env.state, &token, &csrf).await;
    forget_last_step(&env.state).await;
    (env, e)
}

async fn verify(state: &AppState, token: &str, code: &str) -> Reply {
    post(
        state,
        "/api/auth/mfa/verify",
        token,
        None,
        json!({ "code": code }),
    )
    .await
}

async fn recovery(state: &AppState, token: &str, code: &str) -> Reply {
    post(
        state,
        "/api/auth/recovery-code",
        token,
        None,
        json!({ "code": code }),
    )
    .await
}

async fn audit(state: &AppState) -> Vec<(String, Value)> {
    sqlx::query_as("SELECT action, detail FROM audit_events ORDER BY at, action")
        .fetch_all(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn enrolment_returns_ten_recovery_codes_once() {
    let env = env().await;
    let (token, csrf, _) = login(&env.state).await;

    // Ecriture protegee par CSRF.
    let r = post(
        &env.state,
        "/api/me/mfa/totp/start",
        &token,
        None,
        json!({}),
    )
    .await;
    assert_eq!(r.status, 403);

    let start = post(
        &env.state,
        "/api/me/mfa/totp/start",
        &token,
        Some(&csrf),
        json!({ "current_password": PASSWORD }),
    )
    .await;
    assert_eq!(start.status, 200, "{}", start.raw);
    assert_eq!(start.cache_control.as_deref(), Some("no-store"));
    let uri = start.json()["otpauth_uri"].as_str().unwrap().to_string();
    assert!(
        uri.starts_with("otpauth://totp/Ciphera:alice%40example.com?"),
        "{uri}"
    );
    assert!(uri.contains("issuer=Ciphera"), "{uri}");
    let svg = start.json()["qr_svg"].as_str().unwrap().to_string();
    assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
    assert!(!svg.contains("<script") && !svg.contains("href"));

    // Graine scellee (age) : ni le base32 ni la valeur brute ne sont en base.
    let secret_b32 = uri
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("secret="))
        .unwrap()
        .to_string();
    let (sealed, confirmed): (Vec<u8>, bool) = sqlx::query_as(
        "SELECT secret_sealed, confirmed_at IS NOT NULL FROM user_totp WHERE user_id = $1",
    )
    .bind(env.user_id)
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert!(!confirmed);
    let raw = Secret::try_from_base32(&secret_b32).unwrap();
    assert!(!sealed.windows(raw.len()).any(|w| w == raw.as_ref()));
    assert!(!String::from_utf8_lossy(&sealed).contains(&secret_b32));
    // Tant que la TOTP n'est pas confirmee, la connexion ne la demande pas.
    assert!(!login(&env.state).await.2);

    // Mauvais code : refuse, rien n'est active.
    let totp = totp_from_uri(&uri);
    away_from_step_boundary().await;
    let wrong = format!(
        "{:06}",
        (code(&totp, 0).parse::<u32>().unwrap() + 1) % 1_000_000
    );
    let r = post(
        &env.state,
        "/api/me/mfa/totp/confirm",
        &token,
        Some(&csrf),
        json!({ "code": wrong }),
    )
    .await;
    assert_eq!(r.status, 422, "{}", r.raw);
    assert_eq!(r.code(), "invalid_code");

    let r = post(
        &env.state,
        "/api/me/mfa/totp/confirm",
        &token,
        Some(&csrf),
        json!({ "code": code(&totp, 0) }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(r.cache_control.as_deref(), Some("no-store"));
    let codes: Vec<String> = r.json()["recovery_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    assert_eq!(codes.len(), 10);
    for c in &codes {
        assert_eq!(c.len(), 11, "{c}");
        assert_eq!(&c[5..6], "-");
        assert!(
            c.chars()
                .all(|x| x == '-' || x.is_ascii_uppercase() || ('2'..='7').contains(&x))
        );
    }
    let mut unique = codes.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 10);

    // La session a tourne (nouveau jeton, nouveau CSRF).
    let (new_token, new_csrf) = (r.token(), r.csrf());
    assert_ne!(new_token, token);
    assert_eq!(me(&env.state, &token).await.status, 401);
    let m = me(&env.state, &new_token).await;
    assert_eq!(m.status, 200);
    assert_eq!(m.json()["totp_enabled"], true);
    assert_eq!(m.json()["mfa_enrollment_required"], false);

    // Deja active : un nouvel enrolement exige de la desactiver d'abord.
    let r = post(
        &env.state,
        "/api/me/mfa/totp/start",
        &new_token,
        Some(&new_csrf),
        json!({ "current_password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, 409);
    assert_eq!(r.code(), "totp_already_enabled");

    let actions: Vec<String> = audit(&env.state).await.into_iter().map(|a| a.0).collect();
    assert!(
        actions.contains(&"mfa.totp.enabled".to_string()),
        "{actions:?}"
    );
}

#[tokio::test]
async fn new_start_replaces_pending_secret() {
    let env = env().await;
    let (token, csrf, _) = login(&env.state).await;
    let first = post(
        &env.state,
        "/api/me/mfa/totp/start",
        &token,
        Some(&csrf),
        json!({ "current_password": PASSWORD }),
    )
    .await;
    let second = post(
        &env.state,
        "/api/me/mfa/totp/start",
        &token,
        Some(&csrf),
        json!({ "current_password": PASSWORD }),
    )
    .await;
    assert_eq!(second.status, 200);
    let (a, b) = (
        first.json()["otpauth_uri"].as_str().unwrap().to_string(),
        second.json()["otpauth_uri"].as_str().unwrap().to_string(),
    );
    assert_ne!(a, b);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM user_totp")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(rows, 1);
    away_from_step_boundary().await;
    let old = code(&totp_from_uri(&a), 0);
    let new = code(&totp_from_uri(&b), 0);
    if old != new {
        let r = post(
            &env.state,
            "/api/me/mfa/totp/confirm",
            &token,
            Some(&csrf),
            json!({ "code": old }),
        )
        .await;
        assert_eq!(r.status, 422, "le premier secret est remplace");
    }
    let r = post(
        &env.state,
        "/api/me/mfa/totp/confirm",
        &token,
        Some(&csrf),
        json!({ "code": new }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test]
async fn totp_accepts_current_and_previous_window_only() {
    let (env, e) = enrolled_env().await;

    let (pending, _, mfa) = login(&env.state).await;
    assert!(mfa, "TOTP confirmee : la connexion exige le second facteur");
    let m = me(&env.state, &pending).await;
    assert_eq!(m.status, 401);
    assert_eq!(m.code(), "mfa_required");

    away_from_step_boundary().await;
    for offset in [-2, 1, 2] {
        let r = verify(&env.state, &pending, &code(&e.totp, offset)).await;
        assert_eq!(r.status, 401, "fenetre {offset} : {}", r.raw);
        assert_eq!(r.code(), "invalid_code");
    }
    // Format invalide : refuse et compte comme un echec (4 echecs, sous la limite de 5).
    let r = verify(&env.state, &pending, "abcdef").await;
    assert_eq!(r.status, 401);
    // Fenetre precedente : acceptee.
    let r = verify(&env.state, &pending, &code(&e.totp, -1)).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let full = r.token();
    assert_eq!(me(&env.state, &full).await.status, 200);

    // Fenetre courante : acceptee (plus recente que la derniere utilisee).
    let (pending, _, _) = login(&env.state).await;
    away_from_step_boundary().await;
    let r = verify(&env.state, &pending, &code(&e.totp, 0)).await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn totp_code_not_reusable_within_window() {
    let (env, e) = enrolled_env().await;
    away_from_step_boundary().await;
    let c = code(&e.totp, 0);

    let (a, _, _) = login(&env.state).await;
    assert_eq!(verify(&env.state, &a, &c).await.status, 200);
    let (b, _, _) = login(&env.state).await;
    let r = verify(&env.state, &b, &c).await;
    assert_eq!(r.status, 401, "meme code, meme fenetre : refuse");
    assert_eq!(r.code(), "invalid_code");
    // La fenetre precedente est anterieure a la derniere utilisee : refusee aussi.
    assert_eq!(verify(&env.state, &b, &code(&e.totp, -1)).await.status, 401);

    // Soumissions simultanees du meme code : une seule passe. (Compteur
    // d'echecs remis a zero : seul le rejeu est mesure ici.)
    forget_last_step(&env.state).await;
    sqlx::query("DELETE FROM mfa_attempts")
        .execute(&env.state.db)
        .await
        .unwrap();
    let mut sessions = Vec::new();
    for _ in 0..4 {
        sessions.push(login(&env.state).await.0);
    }
    let tasks: Vec<_> = sessions
        .into_iter()
        .map(|s| {
            let state = env.state.clone();
            let c = c.clone();
            tokio::spawn(async move { verify(&state, &s, &c).await.status })
        })
        .collect();
    let mut statuses = Vec::new();
    for t in tasks {
        statuses.push(t.await.unwrap());
    }
    statuses.sort();
    assert_eq!(statuses, vec![200, 401, 401, 401]);
}

#[tokio::test]
async fn recovery_code_single_use() {
    let (env, e) = enrolled_env().await;
    let first = e.recovery_codes[0].clone();

    let (pending, _, _) = login(&env.state).await;
    // Saisie tolerante : minuscules, sans tiret, espaces.
    let typed = format!(" {} ", first.replace('-', "").to_lowercase());
    let r = recovery(&env.state, &pending, &typed).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let full = r.token();
    assert_ne!(full, pending);
    assert_eq!(me(&env.state, &pending).await.status, 401);
    assert_eq!(me(&env.state, &full).await.status, 200);

    let (pending, _, _) = login(&env.state).await;
    let r = recovery(&env.state, &pending, &first).await;
    assert_eq!(r.status, 401, "deja utilise : {}", r.raw);
    assert_eq!(r.code(), "invalid_code");
    assert_eq!(
        recovery(&env.state, &pending, "AAAAA-AAAAA").await.status,
        401
    );
    assert_eq!(
        recovery(&env.state, &pending, &e.recovery_codes[1])
            .await
            .status,
        200
    );

    let used: i64 =
        sqlx::query_scalar("SELECT count(*) FROM recovery_codes WHERE used_at IS NOT NULL")
            .fetch_one(&env.state.db)
            .await
            .unwrap();
    assert_eq!(used, 2);

    // L'audit trace succes et echecs, jamais le code.
    let events = audit(&env.state).await;
    let actions: Vec<&str> = events.iter().map(|(a, _)| a.as_str()).collect();
    assert!(
        actions.contains(&"auth.recovery_code.success"),
        "{actions:?}"
    );
    assert!(
        actions.contains(&"auth.recovery_code.failure"),
        "{actions:?}"
    );
    for (_, detail) in &events {
        let d = detail.to_string().to_uppercase().replace('-', "");
        for c in &e.recovery_codes {
            assert!(!d.contains(&c.replace('-', "")), "code dans l'audit");
        }
    }
}

#[tokio::test]
async fn recovery_codes_stored_hashed() {
    let (env, e) = enrolled_env().await;
    let rows: Vec<(String, bool)> =
        sqlx::query_as("SELECT code_hash, used_at IS NULL FROM recovery_codes WHERE user_id = $1")
            .bind(env.user_id)
            .fetch_all(&env.state.db)
            .await
            .unwrap();
    assert_eq!(rows.len(), 10);
    for (hash, unused) in &rows {
        assert!(hash.starts_with("$argon2id$"), "{hash}");
        assert!(unused);
        for c in &e.recovery_codes {
            assert!(!hash.contains(c) && !hash.contains(&c.replace('-', "")));
        }
    }
}

#[tokio::test]
async fn mfa_verify_rate_limited() {
    let (env, e) = enrolled_env().await;
    let (pending, _, _) = login(&env.state).await;
    for _ in 0..5 {
        let r = verify(&env.state, &pending, "000000").await;
        // (000000 pourrait etre le bon code : une chance sur un million.)
        assert_eq!(r.status, 401, "{}", r.raw);
    }
    // 6e tentative : bloquee, meme avec le bon code ou un code de secours.
    away_from_step_boundary().await;
    let r = verify(&env.state, &pending, &code(&e.totp, 0)).await;
    assert_eq!(r.status, 429, "{}", r.raw);
    assert_eq!(r.code(), "rate_limited");
    let secs: u64 = r
        .retry_after
        .as_deref()
        .expect("Retry-After")
        .parse()
        .unwrap();
    assert!((1..=900).contains(&secs), "{secs}");
    // Les echecs TOTP ne bloquent pas les codes de secours (limites a part).
    // Une autre session du meme compte ne remet pas le compteur a zero.
    let (other, _, _) = login(&env.state).await;
    assert_eq!(
        verify(&env.state, &other, &code(&e.totp, 0)).await.status,
        429
    );

    let actions: Vec<String> = audit(&env.state).await.into_iter().map(|a| a.0).collect();
    assert_eq!(
        actions.iter().filter(|a| *a == "auth.mfa.failure").count(),
        5
    );

    // Fenetre de 15 minutes echue : le bon code passe.
    sqlx::query("UPDATE mfa_attempts SET at = at - interval '16 minutes'")
        .execute(&env.state.db)
        .await
        .unwrap();
    away_from_step_boundary().await;
    assert_eq!(
        verify(&env.state, &pending, &code(&e.totp, 0)).await.status,
        200
    );
}

/// Verrouillage progressif (ASVS V2.2.1) : au-dela de 20 echecs en 24 h, le
/// compte reste bloque jusqu'a ce que le plus ancien sorte de la journee,
/// meme quand chaque fenetre de 15 minutes est respectee. Sans ce plafond,
/// qui detient le mot de passe essaie 480 codes TOTP par jour.
#[tokio::test]
async fn mfa_failures_are_capped_per_day() {
    let (env, e) = enrolled_env().await;
    let (pending, _, _) = login(&env.state).await;
    // 4 series de 5 echecs, chacune apres expiration de la fenetre de 15 min.
    for _ in 0..4 {
        for _ in 0..5 {
            let r = verify(&env.state, &pending, "000000").await;
            assert_eq!(r.status, 401, "{}", r.raw);
        }
        sqlx::query("UPDATE mfa_attempts SET at = at - interval '16 minutes'")
            .execute(&env.state.db)
            .await
            .unwrap();
    }
    // 20 echecs dans la journee : bloque, meme avec le bon code.
    away_from_step_boundary().await;
    let r = verify(&env.state, &pending, &code(&e.totp, 0)).await;
    assert_eq!(r.status, 429, "{}", r.raw);
    let secs: u64 = r
        .retry_after
        .as_deref()
        .expect("Retry-After")
        .parse()
        .unwrap();
    assert!((901..=86_400).contains(&secs), "{secs}");
    // Le blocage vaut pour le compte, pas pour la seule session.
    let (other, _, _) = login(&env.state).await;
    assert_eq!(
        verify(&env.state, &other, &code(&e.totp, 0)).await.status,
        429
    );

    // Journee echue : le bon code passe.
    sqlx::query("UPDATE mfa_attempts SET at = at - interval '24 hours'")
        .execute(&env.state.db)
        .await
        .unwrap();
    away_from_step_boundary().await;
    assert_eq!(
        verify(&env.state, &pending, &code(&e.totp, 0)).await.status,
        200
    );
}

/// 20 codes TOTP faux, chacun hors de la fenetre de 15 minutes du precedent
/// lot de 5 : atteint le plafond journalier sans la limite courte.
async fn reach_daily_totp_cap(state: &AppState, pending: &str) {
    for _ in 0..4 {
        for _ in 0..5 {
            let r = verify(state, pending, "000000").await;
            assert_eq!(r.status, 401, "{}", r.raw);
        }
        sqlx::query("UPDATE mfa_attempts SET at = at - interval '16 minutes'")
            .execute(&state.db)
            .await
            .unwrap();
    }
}

/// Le plafond journalier ne vise que la TOTP (10^6 codes) : un code de secours
/// (50 bits) reste utilisable par le titulaire du compte.
#[tokio::test]
async fn daily_cap_blocks_totp_but_not_recovery_codes() {
    let (env, e) = enrolled_env().await;
    let (pending, _, _) = login(&env.state).await;
    reach_daily_totp_cap(&env.state, &pending).await;
    away_from_step_boundary().await;
    assert_eq!(
        verify(&env.state, &pending, &code(&e.totp, 0)).await.status,
        429
    );
    let r = recovery(&env.state, &pending, &e.recovery_codes[0]).await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

/// Une reinitialisation du mot de passe met l'ancien detenteur dehors : les
/// echecs de second facteur du compte sont oublies.
#[tokio::test]
async fn password_reset_clears_second_factor_failures() {
    let (env, e) = enrolled_env().await;
    let (pending, _, _) = login(&env.state).await;
    reach_daily_totp_cap(&env.state, &pending).await;

    let mut tx = env.state.db.begin().await.unwrap();
    let issued = control_plane::http::password_reset::issue(&mut tx, EMAIL, now() as f64, 0.0)
        .await
        .unwrap()
        .expect("lien cree");
    tx.commit().await.unwrap();
    const NEW: &str = "un tout autre mot de passe";
    let r = call(
        &env.state,
        "POST",
        "/api/auth/password/reset",
        Req {
            body: Some(json!({ "token": issued.token.expose(), "password": NEW })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);

    let r = call(
        &env.state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": EMAIL, "password": NEW })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    away_from_step_boundary().await;
    let r = verify(&env.state, &r.token(), &code(&e.totp, 0)).await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

/// Un changement de mot de passe (re-authentification reussie) remet aussi
/// les compteurs du compte a zero.
#[tokio::test]
async fn password_change_clears_second_factor_failures() {
    let (env, e) = enrolled_env().await;
    let (pending, _, _) = login(&env.state).await;
    reach_daily_totp_cap(&env.state, &pending).await;
    const NEW: &str = "un tout autre mot de passe";
    let r = post(
        &env.state,
        "/api/me/password",
        &e.token,
        Some(&e.csrf),
        json!({ "current_password": PASSWORD, "new_password": NEW }),
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    let r = call(
        &env.state,
        "POST",
        "/api/auth/login",
        Req {
            body: Some(json!({ "email": EMAIL, "password": NEW })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    away_from_step_boundary().await;
    let r = verify(&env.state, &r.token(), &code(&e.totp, 0)).await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

/// Un 503 « serveur occupe » (calculs Argon2 satures) n'est pas un echec :
/// la tentative enregistree avant le calcul est retiree, pour la connexion
/// comme pour le second facteur. Une autre erreur la laisse compter.
#[tokio::test]
async fn busy_hashing_does_not_count_as_a_failure() {
    use control_plane::auth::password::OffloadError;
    use control_plane::auth::rate_limit::{self, Admission, Attempt, MfaFactor};

    let env = env().await;
    let db = &env.state.db;
    let ip = rate_limit::IpKey::login("203.0.113.9".parse().unwrap());
    let key = rate_limit::email_key(EMAIL);
    let count = |table: &'static str| async move {
        sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
            .fetch_one(db)
            .await
            .unwrap()
    };

    let Admission::Allowed(id) = rate_limit::admit(db, &ip, &key).await.unwrap() else {
        panic!("admis")
    };
    let err = rate_limit::offload_error(db, Attempt::Login(id), OffloadError::Busy, "test").await;
    assert_eq!(err.status, 503);
    assert_eq!(count("login_attempts").await, 0);
    let Admission::Allowed(id) = rate_limit::admit(db, &ip, &key).await.unwrap() else {
        panic!("admis")
    };
    let err = rate_limit::offload_error(
        db,
        Attempt::Login(id),
        OffloadError::Failed("x".into()),
        "test",
    )
    .await;
    assert_eq!(err.status, 500);
    assert_eq!(count("login_attempts").await, 1);

    let session = Uuid::new_v4();
    let Admission::Allowed(id) =
        rate_limit::admit_mfa(db, env.user_id, session, MfaFactor::Reauth, None)
            .await
            .unwrap()
    else {
        panic!("admis")
    };
    let err = rate_limit::offload_error(db, Attempt::Mfa(id), OffloadError::Busy, "test").await;
    assert_eq!(err.status, 503);
    assert_eq!(count("mfa_attempts").await, 0);
}

#[tokio::test]
async fn verify_requires_pending_session() {
    let (env, e) = enrolled_env().await;
    // Sans session.
    let r = call(
        &env.state,
        "POST",
        "/api/auth/mfa/verify",
        Req {
            body: Some(json!({ "code": "123456" })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 401);
    assert_eq!(r.code(), "unauthenticated");
    // Session deja complete.
    let r = verify(&env.state, &e.token, &code(&e.totp, 0)).await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "mfa_not_pending");
    // Session en attente, avec son vrai jeton CSRF : l'enrolement et la
    // desactivation (session complete) lui sont fermes.
    let (pending, csrf, _) = login(&env.state).await;
    let r = post(
        &env.state,
        "/api/me/mfa/totp/start",
        &pending,
        Some(&csrf),
        json!({ "current_password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "mfa_required");
    let r = call(
        &env.state,
        "DELETE",
        "/api/me/mfa/totp",
        Req {
            cookie: Some(&pending),
            csrf: Some(&csrf),
            body: Some(json!({ "password": PASSWORD })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "mfa_required");
}

async fn clear_attempts(state: &AppState) {
    sqlx::query("DELETE FROM mfa_attempts")
        .execute(&state.db)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_second_factor_on_one_session_passes_once() {
    let (env, e) = enrolled_env().await;

    // Meme code de secours, meme session, 4 fois en parallele : un seul 200,
    // les autres voient une session deja complete ; un seul code consomme.
    let (pending, _, _) = login(&env.state).await;
    let tasks: Vec<_> = (0..4)
        .map(|_| {
            let (state, s, c) = (
                env.state.clone(),
                pending.clone(),
                e.recovery_codes[0].clone(),
            );
            tokio::spawn(async move { recovery(&state, &s, &c).await.status })
        })
        .collect();
    let mut statuses = Vec::new();
    for t in tasks {
        statuses.push(t.await.unwrap());
    }
    statuses.sort();
    // Un perdant qui lit la session apres sa rotation la trouve absente (401) ;
    // s'il l'a lue avant, il la voit deja complete (409). Les deux sont sains.
    assert_eq!(statuses.iter().filter(|s| **s == 200).count(), 1);
    assert!(
        statuses.iter().all(|s| matches!(*s, 200 | 401 | 409)),
        "{statuses:?}"
    );

    // Meme code de secours depuis 4 sessions : un seul 200.
    clear_attempts(&env.state).await;
    let mut sessions = Vec::new();
    for _ in 0..4 {
        sessions.push(login(&env.state).await.0);
    }
    let tasks: Vec<_> = sessions
        .into_iter()
        .map(|s| {
            let (state, c) = (env.state.clone(), e.recovery_codes[1].clone());
            tokio::spawn(async move { recovery(&state, &s, &c).await.status })
        })
        .collect();
    let mut statuses = Vec::new();
    for t in tasks {
        statuses.push(t.await.unwrap());
    }
    statuses.sort();
    assert_eq!(statuses, vec![200, 401, 401, 401]);

    // TOTP et code de secours en meme temps sur une session : un seul passe,
    // jamais de 500, et le code de secours n'est pas brule par le perdant.
    clear_attempts(&env.state).await;
    let (pending, _, _) = login(&env.state).await;
    away_from_step_boundary().await;
    let c = code(&e.totp, 0);
    let a = {
        let (state, s) = (env.state.clone(), pending.clone());
        tokio::spawn(async move { verify(&state, &s, &c).await.status })
    };
    let b = {
        let (state, s, rc) = (
            env.state.clone(),
            pending.clone(),
            e.recovery_codes[2].clone(),
        );
        tokio::spawn(async move { recovery(&state, &s, &rc).await.status })
    };
    let (sa, sb) = (a.await.unwrap(), b.await.unwrap());
    let statuses = vec![sa, sb];
    // Perdant : 409 (session lue avant la rotation) ou 401 (apres, session remplacee).
    assert_eq!(statuses.iter().filter(|s| **s == 200).count(), 1);
    assert!(
        statuses.iter().all(|s| matches!(*s, 200 | 401 | 409)),
        "{statuses:?}"
    );
    let used: i64 =
        sqlx::query_scalar("SELECT count(*) FROM recovery_codes WHERE used_at IS NOT NULL")
            .fetch_one(&env.state.db)
            .await
            .unwrap();
    // Codes 0 et 1 deja consommes ; le code 2 seulement si le code de secours a gagne.
    assert_eq!(used, if sb == 200 { 3 } else { 2 });
}

#[tokio::test]
async fn disable_refused_when_organization_requires_mfa() {
    let (env, e) = enrolled_env().await;
    sqlx::query("INSERT INTO instance (id, name, mfa_required) VALUES ($1, 'Acme', true)")
        .bind(Uuid::new_v4())
        .execute(&env.state.db)
        .await
        .unwrap();
    let r = call(
        &env.state,
        "DELETE",
        "/api/me/mfa/totp",
        Req {
            cookie: Some(&e.token),
            csrf: Some(&e.csrf),
            body: Some(json!({ "password": PASSWORD })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "mfa_required_by_org");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM user_totp")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(n, 1, "TOTP conservee");
}

#[tokio::test]
async fn disable_totp_requires_password() {
    let (env, e) = enrolled_env().await;
    let del = |pw: &'static str| {
        let state = env.state.clone();
        let (token, csrf) = (e.token.clone(), e.csrf.clone());
        async move {
            call(
                &state,
                "DELETE",
                "/api/me/mfa/totp",
                Req {
                    cookie: Some(&token),
                    csrf: Some(&csrf),
                    body: Some(json!({ "password": pw })),
                    ..Req::default()
                },
            )
            .await
        }
    };
    let r = del("mauvais mot de passe").await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "invalid_credentials");
    assert_eq!(
        me(&env.state, &e.token).await.status,
        200,
        "session intacte"
    );

    let r = del(PASSWORD).await;
    assert_eq!(r.status, 204, "{}", r.raw);
    for table in ["user_totp", "recovery_codes"] {
        let n: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&env.state.db)
                .await
                .unwrap();
        assert_eq!(n, 0, "{table}");
    }
    assert!(!login(&env.state).await.2, "plus de second facteur");
    let actions: Vec<String> = audit(&env.state).await.into_iter().map(|a| a.0).collect();
    assert!(actions.contains(&"mfa.totp.disabled".to_string()));
}

#[tokio::test]
async fn org_mfa_required_forces_enrollment() {
    let env = env().await;
    sqlx::query("INSERT INTO instance (id, name, mfa_required) VALUES ($1, 'Acme', true)")
        .bind(Uuid::new_v4())
        .execute(&env.state.db)
        .await
        .unwrap();

    let (token, csrf, mfa) = login(&env.state).await;
    assert!(!mfa, "pas encore de TOTP a verifier");
    // GET /api/me reste ouvert et signale l'enrolement obligatoire.
    let m = me(&env.state, &token).await;
    assert_eq!(m.status, 200, "{}", m.raw);
    assert_eq!(m.json()["mfa_enrollment_required"], true);
    assert_eq!(m.json()["totp_enabled"], false);
    // Tout le reste repond 403 mfa_enrollment_required.
    let r = call(
        &env.state,
        "DELETE",
        "/api/me/mfa/totp",
        Req {
            cookie: Some(&token),
            csrf: Some(&csrf),
            body: Some(json!({ "password": PASSWORD })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(r.code(), "mfa_enrollment_required");

    // L'enrolement est permis, puis le drapeau tombe avec la rotation.
    let e = enrol(&env.state, &token, &csrf).await;
    assert_eq!(
        me(&env.state, &token).await.status,
        401,
        "ancienne session supprimee"
    );
    let m = me(&env.state, &e.token).await;
    assert_eq!(m.status, 200);
    assert_eq!(m.json()["mfa_enrollment_required"], false);
    assert_eq!(m.json()["totp_enabled"], true);

    // Connexion suivante : second facteur exige.
    assert!(login(&env.state).await.2);
}

#[tokio::test]
async fn totp_start_requires_password() {
    let env = env().await;
    let (token, csrf, _) = login(&env.state).await;
    let start = |body| {
        post(
            &env.state,
            "/api/me/mfa/totp/start",
            &token,
            Some(&csrf),
            body,
        )
    };

    let r = start(json!({})).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    let r = start(json!({ "current_password": "" })).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    let r = start(json!({ "current_password": "mauvais mot de passe" })).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "invalid_credentials");
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM user_totp")
        .fetch_one(&env.state.db)
        .await
        .unwrap();
    assert_eq!(pending, 0, "aucune graine creee sans mot de passe");

    let r = start(json!({ "current_password": PASSWORD })).await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test]
async fn enrollment_session_can_start_totp_without_password() {
    let env = env().await;
    sqlx::query("INSERT INTO instance (id, name, mfa_required) VALUES ($1, 'Acme', true)")
        .bind(Uuid::new_v4())
        .execute(&env.state.db)
        .await
        .unwrap();
    let (token, csrf, mfa) = login(&env.state).await;
    assert!(!mfa);
    let m = me(&env.state, &token).await;
    assert_eq!(m.json()["mfa_enrollment_required"], true);
    let r = post(
        &env.state,
        "/api/me/mfa/totp/start",
        &token,
        Some(&csrf),
        json!({}),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test]
async fn factor_added_mail_sent() {
    let (mut state, _pg) = common::state().await;
    let outbox: Arc<Mutex<Vec<OutgoingMail>>> = Arc::default();
    let sink = outbox.clone();
    state.mail = Delivery::from_fn(move |_mailer, mail| {
        sink.lock().unwrap().push(mail);
        Box::pin(async { Ok(()) })
    });
    state.cfg = Arc::new(
        Config::from_lookup(|name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
            "CIPHERA_PUBLIC_URL" => Some("https://panel.example.com".to_string()),
            _ => None,
        })
        .unwrap(),
    );
    sqlx::query(
        "INSERT INTO smtp_settings (enabled, host, port, tls, from_address, from_name) \
         VALUES (true, 'smtp.example.com', 587, 'starttls', 'ciphera@example.com', 'Ciphera')",
    )
    .execute(&state.db)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name) VALUES ($1, $2, $3, 'admin', 'Alice Martin')",
    )
    .bind(Uuid::new_v4())
    .bind(EMAIL)
    .bind(password::hash(PASSWORD).unwrap())
    .execute(&state.db)
    .await
    .unwrap();

    let (token, csrf, _) = login(&state).await;
    enrol(&state, &token, &csrf).await;
    for _ in 0..100 {
        if !outbox.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mails = outbox.lock().unwrap().clone();
    assert_eq!(mails.len(), 1, "un seul avis");
    assert_eq!(mails[0].to, EMAIL);
    assert!(mails[0].subject.contains("Nouveau facteur ajouté"));
    assert!(
        !mails[0].text.contains("https://"),
        "aucun lien dans l'avis"
    );
}

#[tokio::test]
async fn totp_start_is_rate_limited_after_five_wrong_passwords() {
    let env = env().await;
    let (token, csrf, _) = login(&env.state).await;
    for _ in 0..5 {
        let r = post(
            &env.state,
            "/api/me/mfa/totp/start",
            &token,
            Some(&csrf),
            json!({ "current_password": "mauvais mot de passe" }),
        )
        .await;
        assert_eq!(r.status, 401, "{}", r.raw);
    }
    let r = post(
        &env.state,
        "/api/me/mfa/totp/start",
        &token,
        Some(&csrf),
        json!({ "current_password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, 429, "{}", r.raw);
}

/// Cinq echecs TOTP depuis une session en attente bloquent la TOTP du compte,
/// mais pas la re-authentification d'une autre session, complete.
#[tokio::test]
async fn pending_session_failures_do_not_block_full_session_reauth() {
    let (env, e) = enrolled_env().await;
    let (pending, _, _) = login(&env.state).await;
    for _ in 0..5 {
        let r = verify(&env.state, &pending, "000000").await;
        assert_eq!(r.status, 401, "{}", r.raw);
    }
    away_from_step_boundary().await;
    assert_eq!(
        verify(&env.state, &pending, &code(&e.totp, 0)).await.status,
        429
    );
    let r = post(
        &env.state,
        "/api/me/password",
        &e.token,
        Some(&e.csrf),
        json!({ "current_password": PASSWORD, "new_password": "un tout autre mot de passe" }),
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
}

/// Un code de secours n'est bloque ni par les echecs TOTP du compte, ni par
/// ceux d'une autre session : seuls comptent les siens (session et IP).
#[tokio::test]
async fn recovery_codes_are_limited_per_session_and_ip_not_by_totp_failures() {
    use control_plane::auth::rate_limit::{self, Admission, IpKey, MfaFactor};
    let (env, e) = enrolled_env().await;
    let (pending, _, _) = login(&env.state).await;
    for _ in 0..5 {
        assert_eq!(verify(&env.state, &pending, "000000").await.status, 401);
    }
    let (other, _, _) = login(&env.state).await;
    let r = recovery(&env.state, &other, &e.recovery_codes[0]).await;
    assert_eq!(r.status, 200, "{}", r.raw);

    // Par session : 5 echecs, puis refus ; une autre session repart a zero.
    let db = &env.state.db;
    let ip = IpKey::login("203.0.113.50".parse().unwrap());
    let admit = |session: Uuid, factor: MfaFactor| {
        let ip = ip.clone();
        async move {
            rate_limit::admit_mfa(db, env.user_id, session, factor, Some(&ip))
                .await
                .unwrap()
        }
    };
    let s1 = Uuid::new_v4();
    for _ in 0..5 {
        assert!(matches!(
            admit(s1, MfaFactor::Recovery).await,
            Admission::Allowed(_)
        ));
    }
    assert!(matches!(
        admit(s1, MfaFactor::Recovery).await,
        Admission::Limited(_)
    ));
    assert!(matches!(
        admit(s1, MfaFactor::Passkey).await,
        Admission::Limited(_)
    ));
    // Par IP : 20 echecs au total (5 de s1 compris), puis refus meme pour une
    // session neuve.
    for _ in 0..3 {
        let s = Uuid::new_v4();
        for _ in 0..5 {
            assert!(matches!(
                admit(s, MfaFactor::Passkey).await,
                Admission::Allowed(_)
            ));
        }
    }
    assert!(matches!(
        admit(Uuid::new_v4(), MfaFactor::Recovery).await,
        Admission::Limited(_)
    ));
}

/// Le plafond journalier TOTP n'est pas remis a zero par une verification
/// reussie : seule une reinitialisation du mot de passe l'efface.
#[tokio::test]
async fn daily_totp_cap_survives_login() {
    let (env, e) = enrolled_env().await;
    let (pending, _, _) = login(&env.state).await;
    // 15 echecs dans la journee, chaque lot hors de la fenetre de 15 minutes.
    for _ in 0..3 {
        for _ in 0..5 {
            assert_eq!(verify(&env.state, &pending, "000000").await.status, 401);
        }
        sqlx::query("UPDATE mfa_attempts SET at = at - interval '16 minutes'")
            .execute(&env.state.db)
            .await
            .unwrap();
    }
    // Le titulaire se connecte (mot de passe puis TOTP).
    away_from_step_boundary().await;
    let r = verify(&env.state, &pending, &code(&e.totp, 0)).await;
    assert_eq!(r.status, 200, "{}", r.raw);
    // 5 echecs de plus : 20 dans la journee, la TOTP est bloquee. Ils sortent
    // de la fenetre de 15 minutes : seul le plafond journalier peut bloquer.
    let (pending, _, _) = login(&env.state).await;
    for _ in 0..5 {
        assert_eq!(verify(&env.state, &pending, "000000").await.status, 401);
    }
    sqlx::query("UPDATE mfa_attempts SET at = at - interval '16 minutes'")
        .execute(&env.state.db)
        .await
        .unwrap();
    away_from_step_boundary().await;
    forget_last_step(&env.state).await;
    let r = verify(&env.state, &pending, &code(&e.totp, 0)).await;
    assert_eq!(r.status, 429, "{}", r.raw);
    let secs: u64 = r
        .retry_after
        .as_deref()
        .expect("Retry-After")
        .parse()
        .unwrap();
    assert!(secs > 900, "bloque par le plafond de 24 h : {secs}");
}

/// Une session en attente du second facteur expire apres 10 minutes
/// (l'horloge est celle de la base : l'age de la session est avance en SQL).
#[tokio::test]
async fn mfa_pending_expires_after_10_min() {
    let (env, e) = enrolled_env().await;
    let (pending, _, _) = login(&env.state).await;
    let ttl: i64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM expires_at - created_at)::bigint FROM sessions WHERE mfa_pending",
    )
    .fetch_one(&env.state.db)
    .await
    .unwrap();
    assert_eq!(ttl, 600);
    let methods = |token: String| {
        let state = env.state.clone();
        async move {
            call(
                &state,
                "GET",
                "/api/auth/mfa/methods",
                Req {
                    cookie: Some(&token),
                    ..Req::default()
                },
            )
            .await
        }
    };
    sqlx::query("UPDATE sessions SET created_at = now() - interval '9 minutes' WHERE mfa_pending")
        .execute(&env.state.db)
        .await
        .unwrap();
    assert_eq!(methods(pending.clone()).await.status, 200);
    sqlx::query("UPDATE sessions SET created_at = now() - interval '11 minutes' WHERE mfa_pending")
        .execute(&env.state.db)
        .await
        .unwrap();
    assert_eq!(methods(pending.clone()).await.status, 401);
    away_from_step_boundary().await;
    let r = verify(&env.state, &pending, &code(&e.totp, 0)).await;
    assert_eq!(r.status, 401, "{}", r.raw);
}
