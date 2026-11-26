//! Confirmation renforcee (step-up) des changements d'equipe privilegies :
//! creation d'une invitation, changement de role, retrait d'un membre, lien
//! de reinitialisation du mot de passe d'un membre et remise a zero de son
//! second facteur. Mot de passe, plus le code du second facteur si le compte
//! en a un ; code consomme dans la transaction de l'action ; echec = aucune
//! modification. Chaque action est auditee et les autres proprietaires sont
//! prevenus par e-mail (notification `team_change`) quand SMTP est actif.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use common::api::{Reply, Req, call};
use control_plane::auth::password;
use control_plane::domain::notifications;
use control_plane::jobs::{self, JobKind};
use control_plane::mail::{Delivery, OutgoingMail};
use control_plane::state::AppState;
use serde_json::{Value, json};
use shared::config::Config;
use uuid::Uuid;

const PASSWORD: &str = "correct horse battery staple";
const PUBLIC_URL: &str = "https://panel.example.com";

type Session = (String, String);
type Outbox = Arc<Mutex<Vec<OutgoingMail>>>;

/// Etat de test avec origine publique et boite d'envoi en memoire.
async fn env() -> (AppState, common::TestDb, Outbox) {
    let (mut state, pg) = common::state().await;
    state.cfg = Arc::new(
        Config::from_lookup(|name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
            "CIPHERA_PUBLIC_URL" => Some(PUBLIC_URL.to_string()),
            _ => None,
        })
        .unwrap(),
    );
    let outbox: Outbox = Arc::default();
    let sink = outbox.clone();
    state.mail = Delivery::from_fn(move |_mailer, mail| {
        sink.lock().unwrap().push(mail);
        Box::pin(async { Ok(()) })
    });
    (state, pg, outbox)
}

async fn smtp(state: &AppState) {
    sqlx::query(
        "INSERT INTO smtp_settings (enabled, host, port, tls, from_address, from_name) \
         VALUES (true, 'smtp.example.com', 587, 'starttls', 'ciphera@example.com', 'Ciphera')",
    )
    .execute(&state.db)
    .await
    .unwrap();
}

async fn user(state: &AppState, email: &str, role: &str) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, $3, $4, $5, now())",
    )
    .bind(id)
    .bind(email)
    .bind(password::hash(PASSWORD).unwrap())
    .bind(role)
    .bind(format!("Nom {role}"))
    .execute(&state.db)
    .await
    .unwrap();
    id
}

async fn session(state: &AppState, email: &str) -> Session {
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

async fn send(state: &AppState, s: &Session, method: &str, path: &str, body: Value) -> Reply {
    call(
        state,
        method,
        path,
        Req {
            cookie: Some(&s.0),
            csrf: Some(&s.1),
            body: Some(body),
            ..Req::default()
        },
    )
    .await
}

async fn count(state: &AppState, sql: &'static str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(&state.db).await.unwrap()
}

/// TOTP enrolee par l'API ; renvoie la session tournee et le generateur.
async fn enrol_totp(state: &AppState, s: &Session) -> (Session, totp_rs::Totp) {
    let start = send(
        state,
        s,
        "POST",
        "/api/me/mfa/totp/start",
        json!({ "current_password": PASSWORD }),
    )
    .await;
    assert_eq!(start.status, 200, "{}", start.raw);
    let uri = start.json()["otpauth_uri"].as_str().unwrap().to_string();
    let secret = uri
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("secret="))
        .unwrap()
        .to_string();
    let totp = totp_rs::Builder::new()
        .with_secret(totp_rs::Secret::try_from_base32(&secret).unwrap())
        .with_skew(0)
        .build()
        .unwrap();
    away_from_step_boundary().await;
    let confirm = send(
        state,
        s,
        "POST",
        "/api/me/mfa/totp/confirm",
        json!({ "code": totp_now(&totp) }),
    )
    .await;
    assert_eq!(confirm.status, 200, "{}", confirm.raw);
    sqlx::query("UPDATE user_totp SET last_used_step = NULL")
        .execute(&state.db)
        .await
        .unwrap();
    ((confirm.token(), confirm.csrf()), totp)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn totp_now(totp: &totp_rs::Totp) -> String {
    totp.generate(unix_now()).to_string()
}

/// Code a 6 chiffres hors des fenetres acceptees.
fn wrong_code(totp: &totp_rs::Totp) -> String {
    let now = unix_now();
    let valid: Vec<String> = [now - 30, now, now + 30]
        .iter()
        .map(|t| totp.generate(*t).to_string())
        .collect();
    let mut code = totp_now(totp);
    while valid.contains(&code) {
        code = code
            .chars()
            .map(|c| char::from_digit((c.to_digit(10).unwrap() + 1) % 10, 10).unwrap())
            .collect();
    }
    code
}

async fn away_from_step_boundary() {
    let into = unix_now() % 30;
    if into >= 26 {
        tokio::time::sleep(Duration::from_secs(31 - into)).await;
    }
}

#[derive(Clone, Copy, Debug)]
enum Action {
    Invite,
    ChangeRole,
    Remove,
    ResetLink,
    ResetMfa,
}

impl Action {
    fn audit(self) -> &'static str {
        match self {
            Action::Invite => "team.invitation.created",
            Action::ChangeRole => "team.member.role_changed",
            Action::Remove => "team.member.removed",
            Action::ResetLink => "team.member.reset_link",
            Action::ResetMfa => "team.member.reset_mfa",
        }
    }

    fn success(self) -> u16 {
        match self {
            Action::Invite | Action::ResetLink => 201,
            Action::ChangeRole | Action::ResetMfa => 204,
            Action::Remove => 200,
        }
    }
}

/// Cible neuve de l'action : un developpeur (avec un second facteur pour la
/// remise a zero) ; pour une invitation, l'adresse invitee.
async fn target(state: &AppState, action: Action, n: usize) -> (Uuid, String) {
    let email = format!("cible-{n}@example.com");
    if let Action::Invite = action {
        return (Uuid::nil(), email);
    }
    let id = user(state, &email, "developer").await;
    if let Action::ResetMfa = action {
        sqlx::query(
            "INSERT INTO user_totp (user_id, secret_sealed, confirmed_at) VALUES ($1, '\\x00', now())",
        )
        .bind(id)
        .execute(&state.db)
        .await
        .unwrap();
    }
    (id, email)
}

/// Appel de l'action avec les champs de confirmation `step_up` (objet JSON).
async fn act(
    state: &AppState,
    s: &Session,
    action: Action,
    target: &(Uuid, String),
    step_up: Value,
) -> Reply {
    let mut body = step_up;
    let id = target.0;
    let (method, path) = match action {
        Action::Invite => {
            body["email"] = json!(target.1);
            body["role"] = json!("owner");
            ("POST", "/api/team/invitations".to_string())
        }
        Action::ChangeRole => {
            body["role"] = json!("owner");
            ("PATCH", format!("/api/team/members/{id}"))
        }
        Action::Remove => ("DELETE", format!("/api/team/members/{id}")),
        Action::ResetLink => ("POST", format!("/api/team/members/{id}/reset-link")),
        Action::ResetMfa => ("POST", format!("/api/team/members/{id}/reset-mfa")),
    };
    send(state, s, method, &path, body).await
}

/// L'action a-t-elle modifie sa cible ?
async fn changed(state: &AppState, action: Action, target: &(Uuid, String)) -> bool {
    let (id, email) = target;
    let sql = match action {
        Action::Invite => "SELECT EXISTS (SELECT 1 FROM invitations WHERE email = $2)",
        Action::ChangeRole => {
            "SELECT NOT EXISTS (SELECT 1 FROM users WHERE id = $1 AND role = 'developer')"
        }
        Action::Remove => "SELECT NOT EXISTS (SELECT 1 FROM users WHERE id = $1)",
        Action::ResetLink => "SELECT EXISTS (SELECT 1 FROM password_resets WHERE user_id = $1)",
        Action::ResetMfa => "SELECT NOT EXISTS (SELECT 1 FROM user_totp WHERE user_id = $1)",
    };
    sqlx::query_scalar(sql)
        .bind(id)
        .bind(email)
        .fetch_one(&state.db)
        .await
        .unwrap()
}

async fn audit_count(state: &AppState, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE action = $1")
        .bind(action)
        .fetch_one(&state.db)
        .await
        .unwrap()
}

/// Taches `notify` d'un changement d'equipe en file.
async fn team_notices(state: &AppState) -> i64 {
    count(
        state,
        "SELECT count(*) FROM jobs WHERE kind = 'notify' AND payload->>'event' = 'team_change'",
    )
    .await
}

/// Deroule complet d'une action : refus sans step-up, avec le mot de passe
/// seul, avec un mauvais code, puis succes, puis code rejoue ; rien ne change
/// tant que le step-up echoue.
async fn scenario(action: Action) {
    let (state, _pg, _outbox) = env().await;
    smtp(&state).await;
    user(&state, "owner@example.com", "owner").await;
    user(&state, "owner2@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    let (owner, totp) = enrol_totp(&state, &owner).await;
    let first = target(&state, action, 1).await;

    // Sans step-up : refuse.
    let r = act(&state, &owner, action, &first, json!({})).await;
    assert!(
        r.status == 401 || r.status == 422,
        "{action:?} sans step-up : {} {}",
        r.status,
        r.raw
    );
    // Mot de passe seul : le second facteur est exige.
    let r = act(
        &state,
        &owner,
        action,
        &first,
        json!({ "password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, 401, "{action:?} {}", r.raw);
    assert_eq!(r.code(), json!("second_factor_required"), "{action:?}");
    // Mauvais code : refuse.
    away_from_step_boundary().await;
    let r = act(
        &state,
        &owner,
        action,
        &first,
        json!({ "password": PASSWORD, "code": wrong_code(&totp) }),
    )
    .await;
    assert_eq!(r.status, 401, "{action:?} {}", r.raw);
    assert_eq!(r.code(), json!("invalid_code"), "{action:?}");
    // Bon code, mauvais mot de passe : refuse.
    let r = act(
        &state,
        &owner,
        action,
        &first,
        json!({ "password": "wrong password value", "code": totp_now(&totp) }),
    )
    .await;
    assert_eq!(r.status, 401, "{action:?} {}", r.raw);
    assert_eq!(r.code(), json!("invalid_credentials"), "{action:?}");
    assert!(
        !changed(&state, action, &first).await,
        "{action:?} : modifie sans step-up"
    );
    assert_eq!(audit_count(&state, action.audit()).await, 0, "{action:?}");
    assert_eq!(team_notices(&state).await, 0, "{action:?}");

    // Mot de passe et code TOTP : l'action est faite, auditee et notifiee.
    away_from_step_boundary().await;
    let code = totp_now(&totp);
    let r = act(
        &state,
        &owner,
        action,
        &first,
        json!({ "password": PASSWORD, "code": code }),
    )
    .await;
    assert_eq!(r.status, action.success(), "{action:?} {}", r.raw);
    assert!(changed(&state, action, &first).await, "{action:?}");
    assert_eq!(audit_count(&state, action.audit()).await, 1, "{action:?}");
    assert_eq!(team_notices(&state).await, 1, "{action:?}");
    let detail: String =
        sqlx::query_scalar("SELECT detail::text FROM audit_events WHERE action = $1")
            .bind(action.audit())
            .fetch_one(&state.db)
            .await
            .unwrap();
    assert!(
        !detail.contains(PASSWORD) && !detail.contains(&code),
        "{detail}"
    );

    // Le meme code rejoue sur une autre cible : refuse, rien ne change.
    let second = target(&state, action, 2).await;
    let r = act(
        &state,
        &owner,
        action,
        &second,
        json!({ "password": PASSWORD, "code": code }),
    )
    .await;
    assert_eq!(r.status, 401, "{action:?} {}", r.raw);
    assert_eq!(r.code(), json!("invalid_code"), "{action:?}");
    assert!(
        !changed(&state, action, &second).await,
        "{action:?} : code rejoue"
    );
    assert_eq!(audit_count(&state, action.audit()).await, 1, "{action:?}");
}

#[tokio::test]
async fn invitation_requires_step_up() {
    scenario(Action::Invite).await;
}

#[tokio::test]
async fn role_change_requires_step_up() {
    scenario(Action::ChangeRole).await;
}

#[tokio::test]
async fn member_removal_requires_step_up() {
    scenario(Action::Remove).await;
}

#[tokio::test]
async fn member_reset_link_requires_step_up() {
    scenario(Action::ResetLink).await;
}

#[tokio::test]
async fn member_mfa_reset_requires_step_up() {
    scenario(Action::ResetMfa).await;
}

/// Compte sans second facteur : le mot de passe suffit (comme les autres
/// actions protegees).
#[tokio::test]
async fn password_alone_suffices_without_second_factor() {
    let (state, _pg, _outbox) = env().await;
    user(&state, "owner@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    for (n, action) in [
        Action::Invite,
        Action::ChangeRole,
        Action::ResetLink,
        Action::ResetMfa,
        Action::Remove,
    ]
    .into_iter()
    .enumerate()
    {
        let t = target(&state, action, n).await;
        let r = act(
            &state,
            &owner,
            action,
            &t,
            json!({ "password": "wrong password value" }),
        )
        .await;
        assert_eq!(r.status, 401, "{action:?} {}", r.raw);
        assert_eq!(r.code(), json!("invalid_credentials"), "{action:?}");
        assert!(!changed(&state, action, &t).await, "{action:?}");
        let r = act(&state, &owner, action, &t, json!({ "password": PASSWORD })).await;
        assert_eq!(r.status, action.success(), "{action:?} {}", r.raw);
        assert!(changed(&state, action, &t).await, "{action:?}");
    }
}

/// Les autres proprietaires recoivent l'avis ; ni l'auteur, ni un
/// administrateur. Ni mot de passe ni lien dans l'e-mail.
#[tokio::test]
async fn other_owners_are_notified() {
    let (state, _pg, outbox) = env().await;
    smtp(&state).await;
    user(&state, "owner@example.com", "owner").await;
    user(&state, "owner2@example.com", "owner").await;
    user(&state, "admin@example.com", "admin").await;
    let owner = session(&state, "owner@example.com").await;
    let t = target(&state, Action::Invite, 1).await;
    let r = act(
        &state,
        &owner,
        Action::Invite,
        &t,
        json!({ "password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let t = target(&state, Action::ResetLink, 2).await;
    let r = act(
        &state,
        &owner,
        Action::ResetLink,
        &t,
        json!({ "password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    let link = r.json()["link"].as_str().unwrap().to_string();
    assert_eq!(team_notices(&state).await, 2);

    for _ in 0..10 {
        let Some(job) = jobs::claim(&state.db, &state.clock, "test").await.unwrap() else {
            break;
        };
        assert_eq!(job.kind, JobKind::Notify);
        let id = job.id;
        notifications::handle(&state, job).await.unwrap();
        jobs::complete(&state.db, &state.clock, id, "test")
            .await
            .unwrap();
    }
    // Avis d'equipe seulement (l'invite recoit aussi son lien, a part).
    let sent: Vec<OutgoingMail> = outbox
        .lock()
        .unwrap()
        .iter()
        .filter(|m| m.subject.starts_with("Équipe Ciphera"))
        .cloned()
        .collect();
    assert_eq!(sent.len(), 2);
    for mail in &sent {
        assert_eq!(mail.to, "owner2@example.com");
        assert!(!mail.text.contains(PASSWORD));
        let token = link.rsplit('/').next().unwrap();
        assert!(
            !mail.text.contains(token),
            "lien de reinitialisation dans l'avis"
        );
        assert!(mail.text.contains("owner@example.com"), "{}", mail.text);
        assert!(mail.text.contains("/settings/team"), "{}", mail.text);
    }
    assert!(sent.iter().any(|m| {
        m.text
            .contains("au rôle Propriétaire pour cible-1@example.com")
    }));
}

/// Sans SMTP : aucune notification en file, l'audit reste.
#[tokio::test]
async fn without_smtp_only_the_audit_is_written() {
    let (state, _pg, _outbox) = env().await;
    user(&state, "owner@example.com", "owner").await;
    user(&state, "owner2@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    let t = target(&state, Action::Remove, 1).await;
    let r = act(
        &state,
        &owner,
        Action::Remove,
        &t,
        json!({ "password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    assert_eq!(audit_count(&state, "team.member.removed").await, 1);
    assert_eq!(team_notices(&state).await, 0);
}
