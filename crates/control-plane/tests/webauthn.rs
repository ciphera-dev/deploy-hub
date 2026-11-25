//! Passkeys (WebAuthn) : enregistrement, second facteur, connexion sans mot
//! de passe, suppression. Authentificateur logiciel `webauthn-authenticator-rs`
//! (SoftPasskey : verification de l'utilisateur simulee).

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::domain::domains::panel::Panel;
use control_plane::mail::{Delivery, OutgoingMail};
use control_plane::state::AppState;
use serde_json::{Value, json};
use shared::config::Config;
use uuid::Uuid;
use webauthn_authenticator_rs::WebauthnAuthenticator;
use webauthn_authenticator_rs::softpasskey::SoftPasskey;
use webauthn_rs::prelude::{CreationChallengeResponse, RequestChallengeResponse, Url};

const EMAIL: &str = "alice@example.com";
const PASSWORD: &str = "correct horse battery staple";
const RP_ID: &str = "localhost";
const ORIGIN: &str = "https://localhost:8443";

struct Env {
    state: AppState,
    user_id: Uuid,
    _pg: common::TestDb,
}

fn config(pairs: &'static [(&'static str, &'static str)]) -> Arc<Config> {
    Arc::new(
        Config::from_lookup(|name| {
            if name == "CIPHERA_DATABASE_URL" {
                return Some("postgres://u:p@127.0.0.1/c".to_string());
            }
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        })
        .unwrap(),
    )
}

async fn env_with(pairs: &'static [(&'static str, &'static str)]) -> Env {
    let (mut state, pg) = common::state().await;
    state.cfg = config(pairs);
    let user_id = add_user(&state, EMAIL).await;
    Env {
        state,
        user_id,
        _pg: pg,
    }
}

async fn env() -> Env {
    env_with(&[
        ("CIPHERA_WEBAUTHN_RP_ID", RP_ID),
        ("CIPHERA_WEBAUTHN_RP_ORIGIN", ORIGIN),
    ])
    .await
}

async fn add_user(state: &AppState, email: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, 'admin', 'Alice Martin', now())",
    )
    .bind(id)
    .bind(email)
    .bind(password::hash(PASSWORD).unwrap())
    .execute(&state.db)
    .await
    .unwrap();
    id
}

fn authenticator() -> SoftPasskey {
    // `true` : la verification de l'utilisateur (UV) est annoncee comme faite.
    SoftPasskey::new(true)
}

fn origin(o: &str) -> Url {
    Url::parse(o).unwrap()
}

/// Session ouverte par le mot de passe : (jeton, jeton CSRF, mfa_required).
async fn login_as(state: &AppState, email: &str) -> (String, String, bool) {
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
    let mfa = r.json()["mfa_required"].as_bool().unwrap();
    (r.token(), r.csrf(), mfa)
}

async fn login(state: &AppState) -> (String, String, bool) {
    login_as(state, EMAIL).await
}

async fn send(
    state: &AppState,
    method: &str,
    path: &str,
    token: Option<&str>,
    csrf: Option<&str>,
    body: Option<Value>,
) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: token,
            csrf,
            body,
            ..Req::default()
        },
    )
    .await
}

struct Registered {
    /// Identifiant de la ligne (`/api/me/webauthn/{id}`).
    id: String,
    /// Identifiant du credential (rawId, base64url).
    raw_id: String,
    token: String,
    csrf: String,
    /// Codes de secours emis a l'enregistrement (vide si le compte en avait).
    recovery_codes: Vec<String>,
}

/// Enregistre une passkey depuis une session complete ; renvoie la session tournee.
async fn register(state: &AppState, auth: &mut SoftPasskey, token: &str, csrf: &str) -> Registered {
    register_at(state, auth, token, csrf, None, ORIGIN).await
}

/// Appel sous l'hote `host` (aucun en-tete `Host` si `None`).
async fn send_at(
    state: &AppState,
    host: Option<&str>,
    method: &str,
    path: &str,
    token: Option<&str>,
    csrf: Option<&str>,
    body: Option<Value>,
) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: token,
            csrf,
            host,
            body,
            ..Req::default()
        },
    )
    .await
}

/// Enregistrement sous l'hote `host`, le navigateur etant sur `from`.
async fn register_at(
    state: &AppState,
    auth: &mut SoftPasskey,
    token: &str,
    csrf: &str,
    host: Option<&str>,
    from: &str,
) -> Registered {
    let r = send_at(
        state,
        host,
        "POST",
        "/api/me/webauthn/register/options",
        Some(token),
        Some(csrf),
        Some(json!({ "current_password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let ceremony = r.json()["ceremony_id"].as_str().unwrap().to_string();
    let ccr: CreationChallengeResponse =
        serde_json::from_value(r.json()["options"].clone()).expect("options de creation WebAuthn");
    let cred = auth
        .do_registration(origin(from), ccr)
        .expect("enregistrement par l'authentificateur logiciel");
    let cred = serde_json::to_value(&cred).unwrap();
    let raw_id = cred["rawId"].as_str().unwrap().to_string();
    let v = send_at(
        state,
        host,
        "POST",
        "/api/me/webauthn/register/verify",
        Some(token),
        Some(csrf),
        Some(json!({ "ceremony_id": ceremony, "name": "YubiKey 5C", "credential": cred })),
    )
    .await;
    assert_eq!(v.status, 200, "{}", v.raw);
    Registered {
        id: v.json()["id"].as_str().unwrap().to_string(),
        raw_id,
        token: v.token(),
        csrf: v.csrf(),
        recovery_codes: serde_json::from_value(v.json()["recovery_codes"].clone())
            .expect("recovery_codes"),
    }
}

async fn unused_codes(state: &AppState) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM recovery_codes WHERE used_at IS NULL")
        .fetch_one(&state.db)
        .await
        .unwrap()
}

async fn require_mfa(state: &AppState) {
    sqlx::query("INSERT INTO instance (id, name, mfa_required) VALUES ($1, 'Acme', true)")
        .bind(Uuid::new_v4())
        .execute(&state.db)
        .await
        .unwrap();
}

/// Attend que `n` requetes soient bloquees sur un verrou de la base (5 s au plus).
async fn wait_for_lock_waiters(state: &AppState, n: i64) {
    for _ in 0..200 {
        let waiting: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_stat_activity \
             WHERE wait_event_type = 'Lock' AND datname = current_database()",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        if waiting >= n {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

/// Tient le verrou des seconds facteurs du compte (meme cle que le serveur)
/// jusqu'au commit de la transaction renvoyee : les requetes concurrentes
/// s'alignent derriere, ce qui rend la course reproductible.
async fn hold_factors_lock(
    state: &AppState,
    user_id: Uuid,
) -> sqlx::Transaction<'static, sqlx::Postgres> {
    let mut tx = state.db.begin().await.unwrap();
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('ciphera-factors:' || $1::text, 0))",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx
}

/// Les deux reponses d'une course : une gagne (`ok`), l'autre est refusee (`refused`).
fn one_each(a: &Reply, b: &Reply, ok: u16, refused: u16) {
    let mut statuses = [a.status, b.status];
    statuses.sort_unstable();
    let mut expected = [ok, refused];
    expected.sort_unstable();
    assert_eq!(statuses, expected, "{} / {}", a.raw, b.raw);
}

/// Options d'assertion : (ceremony_id, options).
async fn assert_options(state: &AppState, token: Option<&str>, mode: &str) -> (String, Value) {
    let r = send(
        state,
        "POST",
        "/api/auth/webauthn/assert/options",
        token,
        None,
        Some(json!({ "mode": mode })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    (
        r.json()["ceremony_id"].as_str().unwrap().to_string(),
        r.json()["options"].clone(),
    )
}

/// Assertion signee par l'authentificateur pour ces options (JSON du navigateur).
fn sign(auth: &mut SoftPasskey, options: Value, from: &str) -> Value {
    let rcr: RequestChallengeResponse =
        serde_json::from_value(options).expect("options d'assertion WebAuthn");
    let cred = auth
        .do_authentication(origin(from), rcr)
        .expect("assertion par l'authentificateur logiciel");
    serde_json::to_value(&cred).unwrap()
}

/// Connexion sans mot de passe : l'authentificateur logiciel ne garde pas de
/// cle residente, on simule donc ce que ferait une passkey decouvrable (le
/// navigateur choisit le credential et renvoie l'identifiant du compte).
fn sign_discoverable(
    auth: &mut SoftPasskey,
    mut options: Value,
    raw_id: &str,
    user_id: Uuid,
    from: &str,
) -> Value {
    assert_eq!(
        options["publicKey"]["allowCredentials"],
        json!([]),
        "connexion sans mot de passe : aucun credential impose"
    );
    options["publicKey"]["allowCredentials"] = json!([{ "type": "public-key", "id": raw_id }]);
    let mut cred = sign(auth, options, from);
    cred["response"]["userHandle"] = json!(URL_SAFE_NO_PAD.encode(user_id.as_bytes()));
    cred
}

async fn assert_verify(state: &AppState, token: Option<&str>, body: Value) -> Reply {
    send(
        state,
        "POST",
        "/api/auth/webauthn/assert/verify",
        token,
        None,
        Some(body),
    )
    .await
}

async fn me(state: &AppState, token: &str) -> Reply {
    send(state, "GET", "/api/me", Some(token), None, None).await
}

async fn stored_counter(state: &AppState) -> i64 {
    sqlx::query_scalar("SELECT (passkey -> 'cred' ->> 'counter')::bigint FROM webauthn_credentials")
        .fetch_one(&state.db)
        .await
        .unwrap()
}

async fn audit_actions(state: &AppState) -> Vec<String> {
    sqlx::query_scalar("SELECT action FROM audit_events ORDER BY at, action")
        .fetch_all(&state.db)
        .await
        .unwrap()
}

/// Raisons des refus d'assertion ecrites dans l'audit.
async fn failure_reasons(state: &AppState) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT detail ->> 'reason' FROM audit_events WHERE action = 'auth.webauthn.failure' ORDER BY at",
    )
    .fetch_all(&state.db)
    .await
    .unwrap()
}

/// Compte avec une passkey, deconnecte : (authentificateur, enregistrement).
async fn registered_env() -> (Env, SoftPasskey, Registered) {
    let env = env().await;
    let mut auth = authenticator();
    let (token, csrf, mfa) = login(&env.state).await;
    assert!(!mfa);
    let reg = register(&env.state, &mut auth, &token, &csrf).await;
    (env, auth, reg)
}

#[tokio::test]
async fn register_then_assert_ok() {
    let (env, mut auth, reg) = registered_env().await;
    let s = &env.state;

    // La session a tourne a l'enregistrement et reste complete.
    assert_eq!(me(s, &reg.token).await.status, 200);
    let list = send(s, "GET", "/api/me/webauthn", Some(&reg.token), None, None).await;
    assert_eq!(list.status, 200, "{}", list.raw);
    let creds = list.json()["credentials"].as_array().unwrap().clone();
    assert_eq!(creds.len(), 1);
    assert_eq!(creds[0]["name"], "YubiKey 5C");
    assert_eq!(creds[0]["id"], reg.id.as_str());
    assert!(creds[0]["last_used_at"].is_null());
    // Ni cle publique ni identifiant du credential dans la liste.
    assert!(!list.raw.contains(&reg.raw_id), "{}", list.raw);
    assert!(!list.raw.contains("public"), "{}", list.raw);

    // Le compte a une passkey : la connexion par mot de passe attend le second facteur.
    let (pending, _, mfa) = login(s).await;
    assert!(mfa);
    let methods = send(
        s,
        "GET",
        "/api/auth/mfa/methods",
        Some(&pending),
        None,
        None,
    )
    .await;
    assert_eq!(methods.status, 200, "{}", methods.raw);
    assert_eq!(
        methods.json(),
        json!({ "totp": false, "webauthn": true, "recovery": true, "public_url": null })
    );
    let (ceremony, options) = assert_options(s, Some(&pending), "second_factor").await;
    assert_eq!(
        options["publicKey"]["allowCredentials"][0]["id"],
        reg.raw_id.as_str()
    );
    // Credentials enregistres avec UV requise : webauthn-rs l'exige aussi a l'assertion.
    assert_eq!(options["publicKey"]["userVerification"], "required");
    let cred = sign(&mut auth, options, ORIGIN);
    let ok = assert_verify(
        s,
        Some(&pending),
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await;
    assert_eq!(ok.status, 200, "{}", ok.raw);
    let full = ok.token();
    assert_ne!(full, pending);
    assert_eq!(me(s, &full).await.status, 200);
    assert_eq!(me(s, &pending).await.status, 401, "ancien jeton invalide");
    assert_eq!(stored_counter(s).await, 1);

    // Connexion sans mot de passe : nouvelle session complete, sans cookie prealable.
    let (ceremony, options) = assert_options(s, None, "login").await;
    assert_eq!(options["publicKey"]["userVerification"], "required");
    let cred = sign_discoverable(&mut auth, options, &reg.raw_id, env.user_id, ORIGIN);
    let ok = assert_verify(
        s,
        None,
        json!({ "ceremony_id": ceremony, "credential": cred, "remember": true }),
    )
    .await;
    assert_eq!(ok.status, 200, "{}", ok.raw);
    assert!(
        ok.set_cookie
            .as_deref()
            .unwrap()
            .contains("Max-Age=2592000")
    );
    let me_reply = me(s, &ok.token()).await;
    assert_eq!(me_reply.status, 200, "{}", me_reply.raw);
    assert_eq!(me_reply.json()["webauthn_enabled"], true);
    assert_eq!(stored_counter(s).await, 2);
    let last_used: Option<i64> = sqlx::query_scalar(
        "SELECT extract(epoch FROM last_used_at)::bigint FROM webauthn_credentials",
    )
    .fetch_one(&s.db)
    .await
    .unwrap();
    assert!(last_used.is_some());

    let actions = audit_actions(s).await;
    for a in [
        "mfa.webauthn.registered",
        "auth.webauthn.success",
        "auth.webauthn.login",
    ] {
        assert!(actions.iter().any(|x| x == a), "{a} : {actions:?}");
    }
}

#[tokio::test]
async fn assert_with_wrong_origin_fails() {
    let (env, mut auth, reg) = registered_env().await;
    let s = &env.state;

    // Meme RP ID, autre origine (port) : refusee par le serveur.
    let (ceremony, options) = assert_options(s, None, "login").await;
    let cred = sign_discoverable(
        &mut auth,
        options,
        &reg.raw_id,
        env.user_id,
        "https://localhost:9999",
    );
    let r = assert_verify(
        s,
        None,
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "webauthn_failed");
    assert!(r.set_cookie.is_none());

    // Second facteur : meme refus, la session reste en attente.
    let (pending, _, _) = login(s).await;
    let (ceremony, options) = assert_options(s, Some(&pending), "second_factor").await;
    let cred = sign(&mut auth, options, "https://localhost:9999");
    let r = assert_verify(
        s,
        Some(&pending),
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(me(s, &pending).await.code(), "mfa_required");
    assert_eq!(stored_counter(s).await, 0, "aucune mise a jour du compteur");
    // Refus par la verification cryptographique (origine), pas faute de cle.
    assert_eq!(
        failure_reasons(s).await,
        ["invalid_assertion", "invalid_assertion"]
    );
}

#[tokio::test]
async fn counter_regression_rejected() {
    let (env, mut auth, reg) = registered_env().await;
    let s = &env.state;

    // Compteur stocke en avance sur l'authentificateur (credential clone).
    sqlx::query(
        "UPDATE webauthn_credentials SET passkey = jsonb_set(passkey, '{cred,counter}', '1000')",
    )
    .execute(&s.db)
    .await
    .unwrap();
    let (ceremony, options) = assert_options(s, None, "login").await;
    let cred = sign_discoverable(&mut auth, options, &reg.raw_id, env.user_id, ORIGIN);
    let r = assert_verify(
        s,
        None,
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "webauthn_failed");
    assert_eq!(stored_counter(s).await, 1000, "compteur inchange");
    assert_eq!(failure_reasons(s).await, ["invalid_assertion"]);

    // Second facteur : le compteur avance pendant la ceremonie (options
    // emises avant) ; la valeur stockee au moment de la verification fait foi.
    sqlx::query(
        "UPDATE webauthn_credentials SET passkey = jsonb_set(passkey, '{cred,counter}', '0')",
    )
    .execute(&s.db)
    .await
    .unwrap();
    let (pending, _, _) = login(s).await;
    let (ceremony, options) = assert_options(s, Some(&pending), "second_factor").await;
    sqlx::query(
        "UPDATE webauthn_credentials SET passkey = jsonb_set(passkey, '{cred,counter}', '1000')",
    )
    .execute(&s.db)
    .await
    .unwrap();
    let cred = sign(&mut auth, options, ORIGIN);
    let r = assert_verify(
        s,
        Some(&pending),
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(me(s, &pending).await.code(), "mfa_required");
    assert_eq!(stored_counter(s).await, 1000);
    assert_eq!(failure_reasons(s).await, ["invalid_assertion", "counter"]);
}

#[tokio::test]
async fn challenge_single_use() {
    let (env, mut auth, reg) = registered_env().await;
    let s = &env.state;

    // Connexion sans mot de passe rejouee.
    let (ceremony, options) = assert_options(s, None, "login").await;
    let cred = sign_discoverable(&mut auth, options, &reg.raw_id, env.user_id, ORIGIN);
    let body = json!({ "ceremony_id": ceremony, "credential": cred });
    assert_eq!(assert_verify(s, None, body.clone()).await.status, 200);
    let replay = assert_verify(s, None, body).await;
    assert_eq!(replay.status, 401, "{}", replay.raw);
    assert_eq!(replay.code(), "webauthn_failed");

    // Un echec consomme aussi le defi : la bonne reponse ensuite est refusee.
    let (ceremony, options) = assert_options(s, None, "login").await;
    let good = sign_discoverable(&mut auth, options, &reg.raw_id, env.user_id, ORIGIN);
    let mut bad = good.clone();
    bad["response"]["signature"] = json!(URL_SAFE_NO_PAD.encode([0u8; 70]));
    let r = assert_verify(
        s,
        None,
        json!({ "ceremony_id": ceremony, "credential": bad }),
    )
    .await;
    assert_eq!(r.status, 401);
    let r = assert_verify(
        s,
        None,
        json!({ "ceremony_id": ceremony, "credential": good }),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);

    // Second facteur : un defi lie a la session n'est utilisable qu'une fois,
    // et seulement par cette session.
    let (pending, _, _) = login(s).await;
    let (other_pending, _, _) = login(s).await;
    let (ceremony, options) = assert_options(s, Some(&pending), "second_factor").await;
    let cred = sign(&mut auth, options, ORIGIN);
    let body = json!({ "ceremony_id": ceremony, "credential": cred });
    let r = assert_verify(s, Some(&other_pending), body.clone()).await;
    assert_eq!(r.status, 401, "autre session : {}", r.raw);
    let ok = assert_verify(s, Some(&pending), body.clone()).await;
    assert_eq!(ok.status, 200, "{}", ok.raw);
    // Rejeu : la ceremonie a ete consommee (et supprimee avec l'ancienne session).
    let replay = assert_verify(s, Some(&ok.token()), body.clone()).await;
    assert_eq!(replay.status, 401, "{}", replay.raw);
    assert_eq!(replay.code(), "webauthn_failed");
    let replay = assert_verify(s, Some(&pending), body).await;
    assert_eq!(replay.status, 401, "{}", replay.raw);

    // Defi expire (5 minutes) : refuse.
    let (ceremony, options) = assert_options(s, None, "login").await;
    sqlx::query("UPDATE webauthn_ceremonies SET expires_at = now() - interval '1 second'")
        .execute(&s.db)
        .await
        .unwrap();
    let cred = sign_discoverable(&mut auth, options, &reg.raw_id, env.user_id, ORIGIN);
    let r = assert_verify(
        s,
        None,
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
}

#[tokio::test]
async fn ip_literal_rp_id_is_unavailable() {
    // Sans configuration, le RP ID est l'hote de la requete.
    let env = env_with(&[]).await;
    let s = &env.state;
    let status = |host: &'static str| {
        call(
            s,
            "GET",
            "/api/auth/webauthn/status",
            Req {
                host: Some(host),
                ..Req::default()
            },
        )
    };
    for host in ["203.0.113.7:8443", "[2001:db8::1]:8443", "203.0.113.7"] {
        let r = status(host).await;
        assert_eq!(r.status, 200, "{}", r.raw);
        assert_eq!(r.json()["available"], false, "{host}");
    }
    for host in ["panel.example.com:8443", "localhost:8443"] {
        assert_eq!(status(host).await.json()["available"], true, "{host}");
    }
    let r = call(
        s,
        "POST",
        "/api/auth/webauthn/assert/options",
        Req {
            host: Some("203.0.113.7:8443"),
            body: Some(json!({ "mode": "login" })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "webauthn_unavailable");
    let r = call(
        s,
        "POST",
        "/api/auth/webauthn/assert/options",
        Req {
            host: Some("panel.example.com:8443"),
            body: Some(json!({ "mode": "login" })),
            ..Req::default()
        },
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(
        r.json()["options"]["publicKey"]["rpId"],
        "panel.example.com"
    );

    // RP ID configure en adresse IP : indisponible aussi.
    let env = env_with(&[("CIPHERA_WEBAUTHN_RP_ID", "203.0.113.7")]).await;
    let (token, csrf, _) = login(&env.state).await;
    let r = send(
        &env.state,
        "POST",
        "/api/me/webauthn/register/options",
        Some(&token),
        Some(&csrf),
        Some(json!({ "current_password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "webauthn_unavailable");
}

/// RP ID annonce aux options de connexion sous l'hote `host`, ou le message
/// de `webauthn_unavailable`.
async fn login_rp_id(state: &AppState, host: &str) -> Result<String, String> {
    let r = call(
        state,
        "POST",
        "/api/auth/webauthn/assert/options",
        Req {
            host: Some(host),
            body: Some(json!({ "mode": "login" })),
            ..Req::default()
        },
    )
    .await;
    if r.status == 200 {
        return Ok(r.json()["options"]["publicKey"]["rpId"]
            .as_str()
            .unwrap()
            .to_string());
    }
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "webauthn_unavailable", "{}", r.raw);
    Err(r.json()["error"]["message"].as_str().unwrap().to_string())
}

/// `GET /api/auth/webauthn/status` sous l'hote `host`.
async fn status_at(state: &AppState, host: &str) -> Value {
    let r = send_at(
        state,
        Some(host),
        "GET",
        "/api/auth/webauthn/status",
        None,
        None,
        None,
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    r.json()
}

/// Refus sous un hote hors du RP ID public : le message donne l'adresse.
fn elsewhere(r: Result<String, String>, url: &str) {
    let message = r.expect_err("passkeys indisponibles hors du RP ID public");
    assert!(message.contains(url), "{message}");
    assert!(message.contains("adresse publique"), "{message}");
}

#[tokio::test]
async fn rp_id_uses_public_origin() {
    // CIPHERA_PUBLIC_URL fixe le RP ID ; seuls cet hote et ses sous-domaines
    // peuvent s'en servir.
    let env = env_with(&[
        ("CIPHERA_PUBLIC_URL", "https://Panel.Example.com:8443"),
        ("CIPHERA_PANEL_ALIASES", "admin.example.org"),
    ])
    .await;
    let s = &env.state;
    for host in [
        "panel.example.com:8443",
        "PANEL.example.com.",
        "eu.panel.example.com",
    ] {
        assert_eq!(
            login_rp_id(s, host).await.as_deref(),
            Ok("panel.example.com"),
            "{host}"
        );
        assert_eq!(
            status_at(s, host).await,
            json!({ "available": true, "public_url": null }),
            "{host}"
        );
    }
    // IP, localhost, alias, nom voisin, domaine parent : refus net, avec
    // l'adresse publique.
    for host in [
        "203.0.113.7:8443",
        "localhost:8443",
        "admin.example.org:8443",
        "evilpanel.example.com",
        "example.com",
    ] {
        elsewhere(login_rp_id(s, host).await, "https://panel.example.com:8443");
        assert_eq!(
            status_at(s, host).await,
            json!({ "available": false, "public_url": "https://panel.example.com:8443" }),
            "{host}"
        );
    }

    // Domaine du panneau confirme : il prime sur l'URL publique.
    let env = env_with(&[("CIPHERA_PUBLIC_URL", "https://panel.example.com:8443")]).await;
    let panel = Panel {
        id: Uuid::new_v4(),
        host: "panel.example.net".into(),
        confirmed: false,
    };
    // Pas encore confirme : l'URL publique fait foi.
    env.state.panel.set(Some(panel.clone()));
    assert_eq!(
        login_rp_id(&env.state, "panel.example.com:8443")
            .await
            .as_deref(),
        Ok("panel.example.com")
    );
    elsewhere(
        login_rp_id(&env.state, "panel.example.net").await,
        "https://panel.example.com:8443",
    );
    env.state.panel.set(Some(Panel {
        confirmed: true,
        ..panel
    }));
    assert_eq!(
        login_rp_id(&env.state, "panel.example.net")
            .await
            .as_deref(),
        Ok("panel.example.net")
    );
    elsewhere(
        login_rp_id(&env.state, "panel.example.com:8443").await,
        "https://panel.example.net",
    );

    // Origine publique en adresse IP (pas de RP ID possible) : l'hote de la
    // requete reste utilise, localhost compris.
    let env = env_with(&[("CIPHERA_PUBLIC_URL", "https://203.0.113.7:8443")]).await;
    assert_eq!(
        login_rp_id(&env.state, "localhost:8443").await.as_deref(),
        Ok("localhost")
    );
    assert!(login_rp_id(&env.state, "203.0.113.7:8443").await.is_err());
    assert_eq!(
        status_at(&env.state, "203.0.113.7:8443").await,
        json!({ "available": false, "public_url": null })
    );

    // Variables WebAuthn dediees : choix explicite de l'operateur, elles
    // priment toujours (sans controle de l'hote).
    let env = env_with(&[
        ("CIPHERA_PUBLIC_URL", "https://panel.example.com:8443"),
        ("CIPHERA_WEBAUTHN_RP_ID", "rp.example.com"),
    ])
    .await;
    assert_eq!(
        login_rp_id(&env.state, "panel.example.com:8443")
            .await
            .as_deref(),
        Ok("rp.example.com")
    );
}

#[tokio::test]
async fn passkey_only_account_reached_by_ip_falls_back_to_recovery_codes() {
    const PUBLIC: &str = "panel.example.com:8443";
    const BY_IP: &str = "203.0.113.7:8443";
    let env = env_with(&[("CIPHERA_PUBLIC_URL", "https://panel.example.com:8443")]).await;
    let s = &env.state;
    // Passkey enregistree a l'adresse publique : seul second facteur.
    let (token, csrf, _) = login(s).await;
    let mut auth = authenticator();
    let reg = register_at(
        s,
        &mut auth,
        &token,
        &csrf,
        Some(PUBLIC),
        "https://panel.example.com:8443",
    )
    .await;
    assert_eq!(reg.recovery_codes.len(), 10);

    // Ajout d'une cle depuis l'IP : refus net qui donne l'adresse publique.
    let r = send_at(
        s,
        Some(BY_IP),
        "POST",
        "/api/me/webauthn/register/options",
        Some(&reg.token),
        Some(&reg.csrf),
        Some(json!({ "current_password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "webauthn_unavailable");
    assert!(
        r.raw.contains("https://panel.example.com:8443"),
        "{}",
        r.raw
    );

    // Connexion par l'IP : la cle n'est pas proposee, les codes de secours le
    // sont, avec l'adresse a laquelle la cle fonctionne.
    let (pending, _, mfa) = login(s).await;
    assert!(mfa);
    let methods = send_at(
        s,
        Some(BY_IP),
        "GET",
        "/api/auth/mfa/methods",
        Some(&pending),
        None,
        None,
    )
    .await;
    assert_eq!(methods.status, 200, "{}", methods.raw);
    assert_eq!(
        methods.json(),
        json!({ "totp": false, "webauthn": false, "recovery": true,
                "public_url": "https://panel.example.com:8443" })
    );
    let r = send_at(
        s,
        Some(BY_IP),
        "POST",
        "/api/auth/webauthn/assert/options",
        Some(&pending),
        None,
        Some(json!({ "mode": "second_factor" })),
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "webauthn_unavailable");
    // A l'adresse publique, la cle est proposee.
    let methods = send_at(
        s,
        Some(PUBLIC),
        "GET",
        "/api/auth/mfa/methods",
        Some(&pending),
        None,
        None,
    )
    .await;
    assert_eq!(methods.json()["webauthn"], true, "{}", methods.raw);
    assert_eq!(methods.json()["public_url"], Value::Null);
    // Le code de secours ouvre la session depuis l'IP.
    let r = send_at(
        s,
        Some(BY_IP),
        "POST",
        "/api/auth/recovery-code",
        Some(&pending),
        None,
        Some(json!({ "code": reg.recovery_codes[0] })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test]
async fn registration_requires_csrf_and_excludes_existing_credentials() {
    let env = env().await;
    let s = &env.state;
    let mut auth = authenticator();
    let (token, csrf, _) = login(s).await;
    let r = send(
        s,
        "POST",
        "/api/me/webauthn/register/options",
        Some(&token),
        None,
        Some(json!({})),
    )
    .await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(r.code(), "csrf");

    let reg = register(s, &mut auth, &token, &csrf).await;
    let r = send(
        s,
        "POST",
        "/api/me/webauthn/register/options",
        Some(&reg.token),
        Some(&reg.csrf),
        Some(json!({ "current_password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(
        r.json()["options"]["publicKey"]["excludeCredentials"][0]["id"],
        reg.raw_id.as_str()
    );

    // Le meme credential presente deux fois : refuse.
    let ceremony = r.json()["ceremony_id"].as_str().unwrap().to_string();
    let ccr: CreationChallengeResponse =
        serde_json::from_value(r.json()["options"].clone()).unwrap();
    // SoftPasskey ignore excludeCredentials : il cree un nouveau credential, accepte.
    let cred = serde_json::to_value(auth.do_registration(origin(ORIGIN), ccr).unwrap()).unwrap();
    let ok = send(
        s,
        "POST",
        "/api/me/webauthn/register/verify",
        Some(&reg.token),
        Some(&reg.csrf),
        Some(json!({ "ceremony_id": ceremony, "name": "", "credential": cred })),
    )
    .await;
    assert_eq!(ok.status, 200, "{}", ok.raw);
    // Le compte a deja ses codes de secours : pas de nouveaux codes.
    assert_eq!(ok.json()["recovery_codes"], json!([]));
    assert_eq!(unused_codes(s).await, 10);
    let names: Vec<String> =
        sqlx::query_scalar("SELECT name FROM webauthn_credentials ORDER BY created_at")
            .fetch_all(&s.db)
            .await
            .unwrap();
    assert_eq!(names, ["YubiKey 5C", "Clé de sécurité"]);

    // Defi d'enregistrement a usage unique.
    let replay = send(
        s,
        "POST",
        "/api/me/webauthn/register/verify",
        Some(&ok.token()),
        Some(&ok.csrf()),
        Some(json!({ "ceremony_id": ceremony, "name": "x", "credential": cred })),
    )
    .await;
    assert_eq!(replay.status, 422, "{}", replay.raw);
    assert_eq!(replay.code(), "webauthn_failed");
}

#[tokio::test]
async fn delete_requires_password_and_only_own_credential() {
    let (env, _auth, reg) = registered_env().await;
    let s = &env.state;
    let path = format!("/api/me/webauthn/{}", reg.id);

    // Un autre compte ne peut pas supprimer la passkey d'Alice (404, pas 403).
    add_user(s, "bob@example.com").await;
    let (bob, bob_csrf, _) = login_as(s, "bob@example.com").await;
    let r = send(
        s,
        "DELETE",
        &path,
        Some(&bob),
        Some(&bob_csrf),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 404, "{}", r.raw);

    let wrong = send(
        s,
        "DELETE",
        &path,
        Some(&reg.token),
        Some(&reg.csrf),
        Some(json!({ "password": "wrong password" })),
    )
    .await;
    assert_eq!(wrong.status, 401, "{}", wrong.raw);
    assert_eq!(wrong.code(), "invalid_credentials");
    let no_csrf = send(
        s,
        "DELETE",
        &path,
        Some(&reg.token),
        None,
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(no_csrf.status, 403);

    let ok = send(
        s,
        "DELETE",
        &path,
        Some(&reg.token),
        Some(&reg.csrf),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(ok.status, 204, "{}", ok.raw);
    let again = send(
        s,
        "DELETE",
        &path,
        Some(&reg.token),
        Some(&reg.csrf),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(again.status, 404);
    let bad_id = send(
        s,
        "DELETE",
        "/api/me/webauthn/not-a-uuid",
        Some(&reg.token),
        Some(&reg.csrf),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(bad_id.status, 404);
    assert!(
        audit_actions(s)
            .await
            .iter()
            .any(|a| a == "mfa.webauthn.deleted")
    );
    // Plus de passkey : connexion par mot de passe sans second facteur, et
    // plus de codes de secours.
    let (_, _, mfa) = login(s).await;
    assert!(!mfa);
    assert_eq!(unused_codes(s).await, 0);
}

#[tokio::test]
async fn passkey_only_account_gets_recovery_codes_once() {
    let (env, _auth, reg) = registered_env().await;
    let s = &env.state;

    // Premiere passkey d'un compte sans TOTP : 10 codes `XXXXX-XXXXX`, une seule fois.
    assert_eq!(reg.recovery_codes.len(), 10);
    assert!(
        reg.recovery_codes
            .iter()
            .all(|c| c.len() == 11 && c.as_bytes()[5] == b'-')
    );
    assert_eq!(unused_codes(s).await, 10);
    let issued: Option<bool> = sqlx::query_scalar(
        "SELECT (detail ->> 'recovery_codes_issued')::boolean FROM audit_events \
         WHERE action = 'mfa.webauthn.registered'",
    )
    .fetch_one(&s.db)
    .await
    .unwrap();
    assert_eq!(issued, Some(true));
    let stored: Vec<String> = sqlx::query_scalar("SELECT code_hash FROM recovery_codes")
        .fetch_all(&s.db)
        .await
        .unwrap();
    assert!(stored.iter().all(|h| h.starts_with("$argon2id$")));
    assert!(!stored.iter().any(|h| reg.recovery_codes.contains(h)));

    // Cle perdue : un code de secours ouvre la session en attente de MFA.
    let (pending, _, mfa) = login(s).await;
    assert!(mfa);
    let r = send(
        s,
        "POST",
        "/api/auth/recovery-code",
        Some(&pending),
        None,
        Some(json!({ "code": reg.recovery_codes[0] })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(me(s, &r.token()).await.status, 200);
    assert_eq!(unused_codes(s).await, 9);
}

#[tokio::test]
async fn recovery_codes_reissued_only_when_none_left() {
    let env = env().await;
    let s = &env.state;
    // Tous les codes consommes : une nouvelle passkey en emet 10 nouveaux.
    sqlx::query(
        "INSERT INTO recovery_codes (id, user_id, code_hash, used_at) VALUES ($1, $2, 'x', now())",
    )
    .bind(Uuid::new_v4())
    .bind(env.user_id)
    .execute(&s.db)
    .await
    .unwrap();
    let mut auth = authenticator();
    let (token, csrf, _) = login(s).await;
    let reg = register(s, &mut auth, &token, &csrf).await;
    assert_eq!(reg.recovery_codes.len(), 10);
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM recovery_codes")
        .fetch_one(&s.db)
        .await
        .unwrap();
    assert_eq!(total, 10, "anciens codes remplaces");
}

#[tokio::test]
async fn concurrent_deletes_keep_one_method_when_org_requires_mfa() {
    let (env, mut auth, first) = registered_env().await;
    let s = &env.state;
    let second = register(s, &mut auth, &first.token, &first.csrf).await;
    require_mfa(s).await;

    // Deux passkeys supprimees en meme temps : une seule part.
    let delete = |id: String| {
        let state = s.clone();
        let (token, csrf) = (second.token.clone(), second.csrf.clone());
        async move {
            send(
                &state,
                "DELETE",
                &format!("/api/me/webauthn/{id}"),
                Some(&token),
                Some(&csrf),
                Some(json!({ "password": PASSWORD })),
            )
            .await
        }
    };
    let gate = hold_factors_lock(s, env.user_id).await;
    let a = tokio::spawn(delete(first.id.clone()));
    let b = tokio::spawn(delete(second.id.clone()));
    // Les deux suppressions ont verifie le mot de passe et attendent le verrou.
    wait_for_lock_waiters(s, 2).await;
    gate.commit().await.unwrap();
    let (a, b) = (a.await.unwrap(), b.await.unwrap());
    one_each(&a, &b, 204, 409);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM webauthn_credentials")
        .fetch_one(&s.db)
        .await
        .unwrap();
    assert_eq!(left, 1);
    assert_eq!(unused_codes(s).await, 10, "codes conserves");

    // Passkey et TOTP supprimees en meme temps : une seule part.
    let remaining: String = sqlx::query_scalar("SELECT id::text FROM webauthn_credentials")
        .fetch_one(&s.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO user_totp (user_id, secret_sealed, confirmed_at) VALUES ($1, '\\x00', now())",
    )
    .bind(env.user_id)
    .execute(&s.db)
    .await
    .unwrap();
    let totp = {
        let state = s.clone();
        let (token, csrf) = (second.token.clone(), second.csrf.clone());
        async move {
            send(
                &state,
                "DELETE",
                "/api/me/mfa/totp",
                Some(&token),
                Some(&csrf),
                Some(json!({ "password": PASSWORD })),
            )
            .await
        }
    };
    let gate = hold_factors_lock(s, env.user_id).await;
    let a = tokio::spawn(delete(remaining));
    let b = tokio::spawn(totp);
    wait_for_lock_waiters(s, 2).await;
    gate.commit().await.unwrap();
    let (a, b) = (a.await.unwrap(), b.await.unwrap());
    one_each(&a, &b, 204, 409);
    let (totp_left, keys_left): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM user_totp), (SELECT count(*) FROM webauthn_credentials)",
    )
    .fetch_one(&s.db)
    .await
    .unwrap();
    assert_eq!(totp_left + keys_left, 1, "une methode reste");
    assert_eq!(unused_codes(s).await, 10, "codes conserves");
}

#[tokio::test]
async fn totp_disable_keeps_recovery_codes_while_passkey_remains() {
    let (env, _auth, reg) = registered_env().await;
    let s = &env.state;
    sqlx::query(
        "INSERT INTO user_totp (user_id, secret_sealed, confirmed_at) VALUES ($1, '\\x00', now())",
    )
    .bind(env.user_id)
    .execute(&s.db)
    .await
    .unwrap();
    require_mfa(s).await;
    assert_eq!(unused_codes(s).await, 10);

    // 2FA imposee, passkey restante : la TOTP peut partir, les codes restent.
    let r = send(
        s,
        "DELETE",
        "/api/me/mfa/totp",
        Some(&reg.token),
        Some(&reg.csrf),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 204, "{}", r.raw);
    let (totp_left, keys_left): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM user_totp), (SELECT count(*) FROM webauthn_credentials)",
    )
    .fetch_one(&s.db)
    .await
    .unwrap();
    assert_eq!((totp_left, keys_left), (0, 1));
    assert_eq!(unused_codes(s).await, 10, "codes de secours conserves");
}

#[tokio::test]
async fn passwordless_with_another_accounts_user_handle_is_refused() {
    let (env, mut auth, reg) = registered_env().await;
    let s = &env.state;
    // Credential valide d'Alice, mais identifiant de compte de Bob.
    let bob = add_user(s, "bob@example.com").await;
    let (ceremony, options) = assert_options(s, None, "login").await;
    let cred = sign_discoverable(&mut auth, options, &reg.raw_id, bob, ORIGIN);
    let r = assert_verify(
        s,
        None,
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "webauthn_failed");
    assert!(r.set_cookie.is_none());
    assert_eq!(failure_reasons(s).await, ["unknown_credential"]);
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1")
        .bind(bob)
        .fetch_one(&s.db)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
}

#[tokio::test]
async fn second_factor_lost_race_is_not_pending() {
    let (env, mut auth, _reg) = registered_env().await;
    let s = &env.state;
    let (pending, _, _) = login(s).await;
    let (ceremony, options) = assert_options(s, Some(&pending), "second_factor").await;
    let cred = sign(&mut auth, options, ORIGIN);

    // Une autre requete tient la session (verrou de ligne) puis la complete :
    // la verification, bloquee sur ce verrou, perd la course.
    let session_id: Uuid = sqlx::query_scalar("SELECT id FROM sessions WHERE mfa_pending")
        .fetch_one(&s.db)
        .await
        .unwrap();
    let mut other = s.db.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM sessions WHERE id = $1 FOR UPDATE")
        .bind(session_id)
        .execute(&mut *other)
        .await
        .unwrap();
    let verify = {
        let state = s.clone();
        let body = json!({ "ceremony_id": ceremony, "credential": cred });
        tokio::spawn(async move { assert_verify(&state, Some(&pending), body).await })
    };
    wait_for_lock_waiters(s, 1).await;
    sqlx::query("UPDATE sessions SET mfa_pending = false WHERE id = $1")
        .bind(session_id)
        .execute(&mut *other)
        .await
        .unwrap();
    other.commit().await.unwrap();

    let r = verify.await.unwrap();
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "mfa_not_pending");
    assert!(r.set_cookie.is_none());
    // Rien n'a ete consomme : compteur inchange.
    assert_eq!(stored_counter(s).await, 0);
}

#[tokio::test]
async fn passkey_satisfies_forced_enrolment() {
    let env = env().await;
    let s = &env.state;
    sqlx::query("INSERT INTO instance (id, name, mfa_required) VALUES ($1, 'Acme', true)")
        .bind(Uuid::new_v4())
        .execute(&s.db)
        .await
        .unwrap();
    let mut auth = authenticator();
    let (token, csrf, mfa) = login(s).await;
    assert!(!mfa);
    assert_eq!(me(s, &token).await.json()["mfa_enrollment_required"], true);
    let blocked = send(s, "GET", "/api/me/webauthn", Some(&token), None, None).await;
    assert_eq!(blocked.status, 403);

    // L'enregistrement d'une passkey est ouvert a la session restreinte et la leve.
    let reg = register(s, &mut auth, &token, &csrf).await;
    let me_reply = me(s, &reg.token).await;
    assert_eq!(me_reply.json()["mfa_enrollment_required"], false);
    assert_eq!(
        send(s, "GET", "/api/me/webauthn", Some(&reg.token), None, None)
            .await
            .status,
        200
    );

    // Derniere methode alors que la 2FA est imposee : suppression refusee.
    let r = send(
        s,
        "DELETE",
        &format!("/api/me/webauthn/{}", reg.id),
        Some(&reg.token),
        Some(&reg.csrf),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "mfa_required_by_org");

    // Connexion suivante : second facteur attendu, pas d'enrolement.
    let (pending, _, mfa) = login(s).await;
    assert!(mfa);
    assert_eq!(me(s, &pending).await.code(), "mfa_required");
}

#[tokio::test]
async fn second_factor_requires_pending_session_and_is_rate_limited() {
    let (env, mut auth, _reg) = registered_env().await;
    let s = &env.state;

    // Pas de session : 401 ; session complete : 409.
    let r = send(
        s,
        "POST",
        "/api/auth/webauthn/assert/options",
        None,
        None,
        Some(json!({ "mode": "second_factor" })),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    let (pending, _, _) = login(s).await;
    let (ceremony, options) = assert_options(s, Some(&pending), "second_factor").await;
    let cred = sign(&mut auth, options, ORIGIN);
    let full = assert_verify(
        s,
        Some(&pending),
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await
    .token();
    let r = send(
        s,
        "POST",
        "/api/auth/webauthn/assert/options",
        Some(&full),
        None,
        Some(json!({ "mode": "second_factor" })),
    )
    .await;
    assert_eq!(r.status, 409, "{}", r.raw);
    assert_eq!(r.code(), "mfa_not_pending");

    // 5 echecs, puis 429.
    let (pending, _, _) = login(s).await;
    for _ in 0..5 {
        let (ceremony, options) = assert_options(s, Some(&pending), "second_factor").await;
        let cred = sign(&mut auth, options, "https://localhost:9999");
        let r = assert_verify(
            s,
            Some(&pending),
            json!({ "ceremony_id": ceremony, "credential": cred }),
        )
        .await;
        assert_eq!(r.status, 401, "{}", r.raw);
    }
    let (ceremony, options) = assert_options(s, Some(&pending), "second_factor").await;
    let cred = sign(&mut auth, options, ORIGIN);
    let r = assert_verify(
        s,
        Some(&pending),
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await;
    assert_eq!(r.status, 429, "{}", r.raw);
    assert!(r.retry_after.is_some());
}

/// 20 echecs TOTP du compte dans la journee (plafond journalier atteint) ne
/// bloquent pas la passkey du titulaire, qui ne se devine pas.
#[tokio::test]
async fn daily_totp_cap_does_not_block_passkeys() {
    let (env, mut auth, _reg) = registered_env().await;
    let s = &env.state;
    let (pending, _, _) = login(s).await;
    sqlx::query(
        "INSERT INTO mfa_attempts (at, user_id, session_id, succeeded, totp)          SELECT now() - interval '1 hour', $1, $2, false, true FROM generate_series(1, 20)",
    )
    .bind(env.user_id)
    .bind(Uuid::new_v4())
    .execute(&s.db)
    .await
    .unwrap();
    let (ceremony, options) = assert_options(s, Some(&pending), "second_factor").await;
    let cred = sign(&mut auth, options, ORIGIN);
    let r = assert_verify(
        s,
        Some(&pending),
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

/// 5 echecs TOTP du compte (fenetre de 15 minutes) ne bloquent pas la passkey.
#[tokio::test]
async fn passkey_not_limited_by_totp_failures() {
    let (env, mut auth, _reg) = registered_env().await;
    let s = &env.state;
    let (pending, _, _) = login(s).await;
    sqlx::query(
        "INSERT INTO mfa_attempts (at, user_id, session_id, succeeded, totp)          SELECT now(), $1, $2, false, true FROM generate_series(1, 5)",
    )
    .bind(env.user_id)
    .bind(Uuid::new_v4())
    .execute(&s.db)
    .await
    .unwrap();
    let (ceremony, options) = assert_options(s, Some(&pending), "second_factor").await;
    let cred = sign(&mut auth, options, ORIGIN);
    let r = assert_verify(
        s,
        Some(&pending),
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test]
async fn passwordless_is_rate_limited_per_ip() {
    let (env, mut auth, reg) = registered_env().await;
    let s = &env.state;
    for _ in 0..5 {
        let (ceremony, options) = assert_options(s, None, "login").await;
        let cred = sign_discoverable(
            &mut auth,
            options,
            &reg.raw_id,
            env.user_id,
            "https://localhost:9999",
        );
        let r = assert_verify(
            s,
            None,
            json!({ "ceremony_id": ceremony, "credential": cred }),
        )
        .await;
        assert_eq!(r.status, 401, "{}", r.raw);
    }
    let (ceremony, options) = assert_options(s, None, "login").await;
    let cred = sign_discoverable(&mut auth, options, &reg.raw_id, env.user_id, ORIGIN);
    let r = assert_verify(
        s,
        None,
        json!({ "ceremony_id": ceremony, "credential": cred }),
    )
    .await;
    assert_eq!(r.status, 429, "{}", r.raw);
}

#[tokio::test]
async fn register_options_without_password_is_401() {
    let env = env().await;
    let s = &env.state;
    let (token, csrf, _) = login(s).await;
    let options = |body| {
        send(
            s,
            "POST",
            "/api/me/webauthn/register/options",
            Some(&token),
            Some(&csrf),
            body,
        )
    };
    let r = options(None).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    let r = options(Some(json!({}))).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    let r = options(Some(json!({ "current_password": "mauvais mot de passe" }))).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "invalid_credentials");
    let ceremonies: i64 = sqlx::query_scalar("SELECT count(*) FROM webauthn_ceremonies")
        .fetch_one(&s.db)
        .await
        .unwrap();
    assert_eq!(ceremonies, 0);
    let r = options(Some(json!({ "current_password": PASSWORD }))).await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test]
async fn enrollment_session_needs_password_to_register() {
    let env = env().await;
    let s = &env.state;
    require_mfa(s).await;
    let (token, csrf, mfa) = login(s).await;
    assert!(!mfa);
    assert_eq!(me(s, &token).await.json()["mfa_enrollment_required"], true);
    let options = |body| {
        send(
            s,
            "POST",
            "/api/me/webauthn/register/options",
            Some(&token),
            Some(&csrf),
            body,
        )
    };
    let r = options(None).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "invalid_credentials");
    let r = options(Some(json!({ "current_password": PASSWORD }))).await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

async fn session_count(state: &AppState, enrollment: bool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM sessions WHERE enrollment_required = $1")
        .bind(enrollment)
        .fetch_one(&state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn first_factor_ends_other_enrollment_sessions() {
    let env = env().await;
    let s = &env.state;
    require_mfa(s).await;
    let (a, a_csrf, _) = login(s).await;
    let (b, _, _) = login(s).await;
    assert_eq!(session_count(s, true).await, 2);
    let mut auth = authenticator();
    // Premiere passkey depuis une session d'enrolement (mot de passe exige).
    let r = send(
        s,
        "POST",
        "/api/me/webauthn/register/options",
        Some(&a),
        Some(&a_csrf),
        Some(json!({ "current_password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let ccr: CreationChallengeResponse =
        serde_json::from_value(r.json()["options"].clone()).unwrap();
    let cred = serde_json::to_value(auth.do_registration(origin(ORIGIN), ccr).unwrap()).unwrap();
    let v = send(
        s,
        "POST",
        "/api/me/webauthn/register/verify",
        Some(&a),
        Some(&a_csrf),
        Some(json!({ "ceremony_id": r.json()["ceremony_id"], "name": "Cle", "credential": cred })),
    )
    .await;
    assert_eq!(v.status, 200, "{}", v.raw);
    assert_eq!(
        session_count(s, true).await,
        0,
        "plus de session restreinte"
    );
    assert_eq!(me(s, &b).await.status, 401, "l'autre session est fermee");
    assert_eq!(me(s, &v.token()).await.status, 200);
}

#[tokio::test]
async fn stale_enrollment_session_needs_password() {
    let (env, _auth, reg) = registered_env().await;
    let s = &env.state;
    // Session restee restreinte alors que le compte a deja un facteur.
    let (stale, stale_csrf, _) = login(s).await;
    let flagged = sqlx::query(
        "UPDATE sessions SET mfa_pending = false, enrollment_required = true WHERE mfa_pending",
    )
    .execute(&s.db)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(flagged, 1);
    let _ = reg;
    let options = |body| {
        send(
            s,
            "POST",
            "/api/me/webauthn/register/options",
            Some(&stale),
            Some(&stale_csrf),
            body,
        )
    };
    let r = options(None).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), "invalid_credentials");
    let r = options(Some(json!({ "current_password": PASSWORD }))).await;
    assert_eq!(r.status, 200, "{}", r.raw);
}

#[tokio::test]
async fn factor_options_are_rate_limited_after_five_wrong_passwords() {
    let env = env().await;
    let s = &env.state;
    let (token, csrf, _) = login(s).await;
    for _ in 0..5 {
        let r = send(
            s,
            "POST",
            "/api/me/webauthn/register/options",
            Some(&token),
            Some(&csrf),
            Some(json!({ "current_password": "mauvais mot de passe" })),
        )
        .await;
        assert_eq!(r.status, 401, "{}", r.raw);
    }
    let r = send(
        s,
        "POST",
        "/api/me/webauthn/register/options",
        Some(&token),
        Some(&csrf),
        Some(json!({ "current_password": PASSWORD })),
    )
    .await;
    assert_eq!(r.status, 429, "{}", r.raw);
}

#[tokio::test]
async fn factor_added_mail_sent_for_passkey() {
    let mut env = env_with(&[
        ("CIPHERA_WEBAUTHN_RP_ID", RP_ID),
        ("CIPHERA_WEBAUTHN_RP_ORIGIN", ORIGIN),
        ("CIPHERA_PUBLIC_URL", "https://panel.example.com"),
    ])
    .await;
    let outbox: Arc<Mutex<Vec<OutgoingMail>>> = Arc::default();
    let sink = outbox.clone();
    env.state.mail = Delivery::from_fn(move |_mailer, mail| {
        sink.lock().unwrap().push(mail);
        Box::pin(async { Ok(()) })
    });
    let s = &env.state;
    sqlx::query(
        "INSERT INTO smtp_settings (enabled, host, port, tls, from_address, from_name) \
         VALUES (true, 'smtp.example.com', 587, 'starttls', 'ciphera@example.com', 'Ciphera')",
    )
    .execute(&s.db)
    .await
    .unwrap();
    let (token, csrf, _) = login(s).await;
    register(s, &mut authenticator(), &token, &csrf).await;
    for _ in 0..100 {
        if !outbox.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mails = outbox.lock().unwrap().clone();
    assert_eq!(mails.len(), 1);
    assert_eq!(mails[0].to, EMAIL);
    assert!(mails[0].subject.contains("Nouveau facteur ajouté"));
}
