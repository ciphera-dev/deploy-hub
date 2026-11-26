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

/// Execute les taches `notify` en file ; renvoie les avis d'equipe envoyes.
async fn deliver(state: &AppState, outbox: &Outbox) -> Vec<OutgoingMail> {
    for _ in 0..20 {
        let Some(job) = jobs::claim(&state.db, &state.clock, "test").await.unwrap() else {
            break;
        };
        let id = job.id;
        notifications::handle(state, job).await.unwrap();
        jobs::complete(&state.db, &state.clock, id, "test")
            .await
            .unwrap();
    }
    outbox
        .lock()
        .unwrap()
        .iter()
        .filter(|m| m.subject.starts_with("Équipe Ciphera"))
        .cloned()
        .collect()
}

/// Avis de securite : envoye meme si le proprietaire a coupe la preference
/// « Équipe » et tous les e-mails (plafond horaire seul).
#[tokio::test]
async fn team_notice_ignores_preferences() {
    let (state, _pg, outbox) = env().await;
    smtp(&state).await;
    user(&state, "owner@example.com", "owner").await;
    let owner2 = user(&state, "owner2@example.com", "owner").await;
    sqlx::query(
        "INSERT INTO notification_prefs \
         (user_id, email_enabled, deploy_succeeded, deploy_failed, certificates, team) \
         VALUES ($1, false, false, false, false, false)",
    )
    .bind(owner2)
    .execute(&state.db)
    .await
    .unwrap();
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
    let sent = deliver(&state, &outbox).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "owner2@example.com");
}

/// Destinataires figes avant le changement : le second proprietaire d'une
/// organisation a deux proprietaires est prevenu de son propre retrait et
/// de sa retrogradation.
#[tokio::test]
async fn removed_or_demoted_owner_is_still_notified() {
    for remove in [true, false] {
        let (state, _pg, outbox) = env().await;
        smtp(&state).await;
        user(&state, "owner@example.com", "owner").await;
        let owner2 = user(&state, "owner2@example.com", "owner").await;
        let owner = session(&state, "owner@example.com").await;
        let r = if remove {
            send(
                &state,
                &owner,
                "DELETE",
                &format!("/api/team/members/{owner2}"),
                json!({ "password": PASSWORD }),
            )
            .await
        } else {
            send(
                &state,
                &owner,
                "PATCH",
                &format!("/api/team/members/{owner2}"),
                json!({ "password": PASSWORD, "role": "developer" }),
            )
            .await
        };
        assert!(r.status == 200 || r.status == 204, "{remove} {}", r.raw);
        let left: i64 =
            sqlx::query_scalar("SELECT count(*) FROM users WHERE id = $1 AND role = 'owner'")
                .bind(owner2)
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(left, 0, "{remove}");
        let sent = deliver(&state, &outbox).await;
        assert_eq!(sent.len(), 1, "retrait={remove}");
        assert_eq!(sent[0].to, "owner2@example.com");
        assert!(
            sent[0].text.contains("quelles que soient ses"),
            "{}",
            sent[0].text
        );
    }
}

/// Sans adresse publique : aucun avis en file (avertissement journalise),
/// l'action et l'audit restent.
#[tokio::test]
async fn without_public_url_no_notice_is_queued() {
    let (mut state, _pg, _outbox) = env().await;
    state.cfg = Arc::new(
        Config::from_lookup(|name| match name {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
            _ => None,
        })
        .unwrap(),
    );
    smtp(&state).await;
    user(&state, "owner@example.com", "owner").await;
    user(&state, "owner2@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    let t = target(&state, Action::ResetLink, 1).await;
    let r = act(
        &state,
        &owner,
        Action::ResetLink,
        &t,
        json!({ "password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, 201, "{}", r.raw);
    assert_eq!(audit_count(&state, "team.member.reset_link").await, 1);
    assert_eq!(team_notices(&state).await, 0);
}

/// Rangs relus sous verrou apres le step-up : la cible promue administrateur
/// pendant la verification (transaction concurrente qui tient sa ligne) n'est
/// plus geree par un administrateur ; rien n'est emis.
#[tokio::test]
async fn rank_is_rechecked_under_lock_after_step_up() {
    for action in [Action::ResetLink, Action::ResetMfa] {
        let (state, _pg, _outbox) = env().await;
        user(&state, "owner@example.com", "owner").await;
        user(&state, "admin@example.com", "admin").await;
        let admin = session(&state, "admin@example.com").await;
        let t = target(&state, action, 1).await;
        // Promotion non validee : le premier controle (sans verrou) voit
        // encore un developpeur.
        let mut hold = state.db.begin().await.unwrap();
        sqlx::query("UPDATE users SET role = 'admin' WHERE id = $1")
            .bind(t.0)
            .execute(&mut *hold)
            .await
            .unwrap();
        let request = {
            let (state, admin, t) = (state.clone(), admin.clone(), t.clone());
            tokio::spawn(async move {
                act(&state, &admin, action, &t, json!({ "password": PASSWORD })).await
            })
        };
        // La requete attend le verrou de la cible (step-up deja passe).
        let mut waiting = false;
        for _ in 0..400 {
            waiting = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM pg_stat_activity \
                 WHERE datname = current_database() AND wait_event_type = 'Lock' AND query ILIKE '%FOR UPDATE%')",
            )
            .fetch_one(&state.db)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(waiting, "{action:?} : la requete n'a pas atteint le verrou");
        hold.commit().await.unwrap();
        let r = request.await.unwrap();
        assert_eq!(r.status, 403, "{action:?} {}", r.raw);
        assert!(!changed(&state, action, &t).await, "{action:?}");
        assert_eq!(audit_count(&state, action.audit()).await, 0, "{action:?}");
    }
}

/// Meme code envoye deux fois en meme temps : une seule action aboutit.
#[tokio::test]
async fn concurrent_replay_succeeds_once() {
    let (state, _pg, _outbox) = env().await;
    user(&state, "owner@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    let (owner, totp) = enrol_totp(&state, &owner).await;
    let a = target(&state, Action::ResetLink, 1).await;
    let b = target(&state, Action::ResetLink, 2).await;
    away_from_step_boundary().await;
    let code = totp_now(&totp);
    let body = json!({ "password": PASSWORD, "code": code });
    let (ra, rb) = tokio::join!(
        act(&state, &owner, Action::ResetLink, &a, body.clone()),
        act(&state, &owner, Action::ResetLink, &b, body.clone()),
    );
    let mut statuses = [ra.status, rb.status];
    statuses.sort_unstable();
    assert_eq!(statuses, [201, 401], "{} / {}", ra.raw, rb.raw);
    let loser = if ra.status == 401 { &ra } else { &rb };
    assert_eq!(loser.code(), json!("invalid_code"));
    let issued = u8::from(changed(&state, Action::ResetLink, &a).await)
        + u8::from(changed(&state, Action::ResetLink, &b).await);
    assert_eq!(issued, 1);
    assert_eq!(audit_count(&state, "team.member.reset_link").await, 1);
}

fn smtp_body(password: &str, code: Option<&str>) -> Value {
    let mut body = json!({
        "enabled": true,
        "host": "smtp.changed.example.com",
        "port": 587,
        "tls": "starttls",
        "username": "",
        "from_address": "ciphera@example.com",
        "from_name": "Ciphera",
        "current_password": password,
    });
    if let Some(code) = code {
        body["code"] = json!(code);
    }
    body
}

/// Plafond d'essais partage entre les actions (et avec les reglages SMTP) :
/// apres 5 mauvais mots de passe dans la session, 429 sur toute action.
#[tokio::test]
async fn attempt_cap_is_shared_between_actions() {
    let (state, _pg, _outbox) = env().await;
    user(&state, "owner@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    let actions = [
        Action::Invite,
        Action::ChangeRole,
        Action::Remove,
        Action::ResetLink,
        Action::ResetMfa,
    ];
    for (n, action) in actions.into_iter().enumerate() {
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
    }
    // Sixieme essai, bon mot de passe, sur une autre action : limite.
    let t = target(&state, Action::ResetLink, 9).await;
    let r = act(
        &state,
        &owner,
        Action::ResetLink,
        &t,
        json!({ "password": PASSWORD }),
    )
    .await;
    assert_eq!(r.status, 429, "{}", r.raw);
    assert!(!changed(&state, Action::ResetLink, &t).await);
    let r = send(
        &state,
        &owner,
        "PUT",
        "/api/settings/notifications",
        smtp_body(PASSWORD, None),
    )
    .await;
    assert_eq!(r.status, 429, "{}", r.raw);
}

/// Reglages SMTP : meme step-up ; refus = reglages inchanges.
#[tokio::test]
async fn smtp_settings_require_step_up() {
    let (state, _pg, _outbox) = env().await;
    smtp(&state).await;
    user(&state, "owner@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    let (owner, totp) = enrol_totp(&state, &owner).await;
    let path = "/api/settings/notifications";
    let mut no_confirm = smtp_body(PASSWORD, None);
    no_confirm
        .as_object_mut()
        .unwrap()
        .remove("current_password");
    let r = send(&state, &owner, "PUT", path, no_confirm).await;
    assert_eq!(r.status, 422, "{}", r.raw);
    let r = send(&state, &owner, "PUT", path, smtp_body(PASSWORD, None)).await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), json!("second_factor_required"));
    away_from_step_boundary().await;
    let r = send(
        &state,
        &owner,
        "PUT",
        path,
        smtp_body("wrong password value", Some(&totp_now(&totp))),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), json!("invalid_credentials"));
    let r = send(
        &state,
        &owner,
        "PUT",
        path,
        smtp_body(PASSWORD, Some(&wrong_code(&totp))),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.raw);
    assert_eq!(r.code(), json!("invalid_code"));
    let host: String = sqlx::query_scalar("SELECT host FROM smtp_settings")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(host, "smtp.example.com");
    let mut unknown = smtp_body(PASSWORD, Some(&totp_now(&totp)));
    unknown["extra"] = json!(true);
    let r = send(&state, &owner, "PUT", path, unknown).await;
    assert_eq!(r.status, 422, "champ inconnu : {}", r.raw);
    let r = send(
        &state,
        &owner,
        "PUT",
        path,
        smtp_body(PASSWORD, Some(&totp_now(&totp))),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.raw);
    let host: String = sqlx::query_scalar("SELECT host FROM smtp_settings")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(host, "smtp.changed.example.com");
    assert!(!r.raw.contains(PASSWORD));
}

/// Champ inconnu dans un corps de step-up : refuse (`deny_unknown_fields`).
#[tokio::test]
async fn step_up_bodies_reject_unknown_fields() {
    let (state, _pg, _outbox) = env().await;
    user(&state, "owner@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    for (n, action) in [
        Action::Invite,
        Action::ChangeRole,
        Action::Remove,
        Action::ResetLink,
        Action::ResetMfa,
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
            json!({ "password": PASSWORD, "otp": "123456" }),
        )
        .await;
        assert_eq!(r.status, 422, "{action:?} {}", r.raw);
        assert!(!changed(&state, action, &t).await, "{action:?}");
    }
}

/// Attend qu'une requete de la base de test soit bloquee sur `FOR UPDATE`.
async fn wait_for_row_lock(state: &AppState) -> bool {
    for _ in 0..400 {
        let waiting: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_stat_activity \
             WHERE datname = current_database() AND wait_event_type = 'Lock' \
             AND query ILIKE '%FOR UPDATE%')",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        if waiting {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

/// Role de `user` change par une transaction concurrente non validee : le
/// premier controle de la requete voit encore l'ancien role. Valide une fois
/// la requete bloquee sur le verrou de ligne (step-up deja passe).
async fn demote_during_step_up(
    state: &AppState,
    user: Uuid,
    role: &str,
    start: impl FnOnce() -> tokio::task::JoinHandle<Reply>,
) -> Reply {
    let mut hold = state.db.begin().await.unwrap();
    sqlx::query("UPDATE users SET role = $2 WHERE id = $1")
        .bind(user)
        .bind(role)
        .execute(&mut *hold)
        .await
        .unwrap();
    // La requete ne demarre qu'une fois la modification en attente.
    let request = start();
    let waiting = wait_for_row_lock(state).await;
    hold.commit().await.unwrap();
    let r = request.await.unwrap();
    assert!(
        waiting,
        "la requete n'a pas attendu le verrou de ligne ({})",
        r.raw
    );
    r
}

/// n2 : proprietaire retrograde pendant le step-up des reglages SMTP : refus
/// sous verrou, reglages inchanges.
#[tokio::test]
async fn smtp_save_rechecks_owner_under_lock() {
    let (state, _pg, _outbox) = env().await;
    smtp(&state).await;
    let owner_id = user(&state, "owner@example.com", "owner").await;
    user(&state, "owner2@example.com", "owner").await;
    let owner = session(&state, "owner@example.com").await;
    let start = || {
        let (state, owner) = (state.clone(), owner.clone());
        tokio::spawn(async move {
            send(
                &state,
                &owner,
                "PUT",
                "/api/settings/notifications",
                smtp_body(PASSWORD, None),
            )
            .await
        })
    };
    let r = demote_during_step_up(&state, owner_id, "admin", start).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    let host: String = sqlx::query_scalar("SELECT host FROM smtp_settings")
        .fetch_one(&state.db)
        .await
        .unwrap();
    assert_eq!(host, "smtp.example.com");
}

/// n2 : invitant retrograde pendant le step-up : refus sous verrou, aucune
/// invitation.
#[tokio::test]
async fn invitation_rechecks_inviter_under_lock() {
    let (state, _pg, _outbox) = env().await;
    user(&state, "owner@example.com", "owner").await;
    let admin_id = user(&state, "admin@example.com", "admin").await;
    let admin = session(&state, "admin@example.com").await;
    let start = || {
        let (state, admin) = (state.clone(), admin.clone());
        tokio::spawn(async move {
            send(
                &state,
                &admin,
                "POST",
                "/api/team/invitations",
                json!({ "email": "cible@example.com", "role": "developer", "password": PASSWORD }),
            )
            .await
        })
    };
    let r = demote_during_step_up(&state, admin_id, "viewer", start).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert_eq!(count(&state, "SELECT count(*) FROM invitations").await, 0);
}

/// n1 : administrateur retrograde pendant le step-up d'une remise a zero du
/// second facteur : refus sous verrou, facteur intact.
#[tokio::test]
async fn mfa_reset_rechecks_actor_under_lock() {
    let (state, _pg, _outbox) = env().await;
    user(&state, "owner@example.com", "owner").await;
    let admin_id = user(&state, "admin@example.com", "admin").await;
    let admin = session(&state, "admin@example.com").await;
    let t = target(&state, Action::ResetMfa, 1).await;
    let start = || {
        let (state, admin, t) = (state.clone(), admin.clone(), t.clone());
        tokio::spawn(async move {
            act(
                &state,
                &admin,
                Action::ResetMfa,
                &t,
                json!({ "password": PASSWORD }),
            )
            .await
        })
    };
    let r = demote_during_step_up(&state, admin_id, "viewer", start).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert!(!changed(&state, Action::ResetMfa, &t).await);
    assert_eq!(audit_count(&state, Action::ResetMfa.audit()).await, 0);
}

/// n1 : refus sous verrou apres un step-up reussi : le code TOTP n'est pas
/// consomme, il sert encore a la demande valide suivante.
#[tokio::test]
async fn refusal_under_lock_leaves_the_code_unused() {
    let (state, _pg, _outbox) = env().await;
    user(&state, "owner@example.com", "owner").await;
    user(&state, "admin@example.com", "admin").await;
    let admin = session(&state, "admin@example.com").await;
    let (admin, totp) = enrol_totp(&state, &admin).await;
    let refused = target(&state, Action::ResetLink, 1).await;
    let allowed = target(&state, Action::ResetLink, 2).await;
    away_from_step_boundary().await;
    let code = totp_now(&totp);
    let start = || {
        let (state, admin, t) = (state.clone(), admin.clone(), refused.clone());
        let body = json!({ "password": PASSWORD, "code": code });
        tokio::spawn(async move { act(&state, &admin, Action::ResetLink, &t, body).await })
    };
    // Cible promue administrateur pendant le step-up de l'administrateur.
    let r = demote_during_step_up(&state, refused.0, "admin", start).await;
    assert_eq!(r.status, 403, "{}", r.raw);
    assert!(!changed(&state, Action::ResetLink, &refused).await);
    let r = act(
        &state,
        &admin,
        Action::ResetLink,
        &allowed,
        json!({ "password": PASSWORD, "code": code }),
    )
    .await;
    assert_eq!(r.status, 201, "meme code reutilise : {}", r.raw);
    assert!(changed(&state, Action::ResetLink, &allowed).await);
}
