//! Notifications e-mail du Profil › Notifications : deploiement reussi ou en
//! echec, emission ou renouvellement de certificat en echec, certificat
//! proche de l'expiration.
//!
//! - **Mise en file a la transition** : [`enqueue`] est appele dans la
//!   transaction qui change le statut (deploiement, certificat). Rien n'est
//!   envoye depuis la requete ni depuis l'orchestrateur : la tache `notify`
//!   s'en charge ([`handle`]), avec les reprises de la file (15 s doublees).
//!   Sans SMTP active ni origine publique, rien n'est mis en file ; si SMTP
//!   est coupe entre-temps, la tache se conclut sans envoi.
//! - **Destinataires** : comptes dont le role lit la ressource (Deploiements
//!   pour un deploiement, Domaines pour le domaine d'un projet, Parametres en
//!   administration pour le domaine du panneau), canal e-mail et evenement
//!   actives (`http::me::default_prefs` sans preferences enregistrees :
//!   certificats actives pour le proprietaire et l'administrateur).
//! - **Une seule fois** : une ligne de `notification_deliveries` par
//!   evenement et destinataire, reservee (`pending`) avant l'envoi puis
//!   `sent` ; une reprise ne renvoie qu'aux destinataires restants (et a
//!   une reservation laissee par une tentative interrompue : au moins une
//!   fois).
//! - **Plafond exact par compte** : [`MAX_PER_HOUR`] e-mails par heure
//!   glissante et par budget, l'un pour les alertes (echecs, certificats),
//!   l'autre pour les deploiements reussis : des reussites nombreuses
//!   n'empechent jamais une alerte. Compte et reservation sous un verrou
//!   consultatif par compte (taches simultanees comprises). Au-dela, la
//!   notification est retenue (`suppressed`) et un resume par compte
//!   (`Event::Digest`, un seul en file) part quand l'heure se libere, une
//!   fois par heure au plus : il liste chaque alerte retenue avec son lien
//!   et compte les reussites retenues. Aucune alerte n'est perdue.
//! - **Contenu** : texte brut en francais, lien vers la page du panneau ;
//!   jamais la cause brute d'un echec (journal de build, reponse du proxy :
//!   elle peut contenir un secret), ni variable, ni secret.
//! - **Entretien horaire** (`metrics::run_rollup`) : [`sweep`] remet au
//!   resume les reservations orphelines (arret sur la derniere tentative) et
//!   replanifie le resume des comptes qui ont des notifications retenues ;
//!   [`purge`] supprime les envois de plus de 30 jours.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::authz::{self, Level, Resource, Role};
use crate::http::me::default_prefs;
use crate::jobs::worker::{HandlerError, Handlers};
use crate::jobs::{self, Job, JobKind, Schedule};
use crate::mail::{self, Mailer, OutgoingMail};
use crate::state::AppState;

/// E-mails de notification par compte, par budget et par heure glissante.
pub const MAX_PER_HOUR: i64 = 10;
const WINDOW_SECS: f64 = 3600.0;
/// Un certificat qui expire dans moins de 7 jours sans renouvellement
/// reussi est signale.
pub const EXPIRING_SOON_SECS: i64 = 7 * 86_400;
/// Tentatives de la tache (envois en echec repris, 15 s doublees).
const MAX_ATTEMPTS: i32 = 6;
/// Duree de conservation des envois (bien au-dela des reprises).
const KEEP_SECS: f64 = 30.0 * 86_400.0;

/// Evenement notifie. La charge utile ne porte que des identifiants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    DeployFailed {
        deployment_id: Uuid,
    },
    DeploySucceeded {
        deployment_id: Uuid,
    },
    /// Emission (`renewal = false`) ou renouvellement d'un certificat encore
    /// valide en echec : premier echec de la chaine `job_id`.
    CertFailed {
        domain_id: Uuid,
        job_id: Uuid,
        renewal: bool,
    },
    /// Certificat qui expire bientot (`expires_at`, secondes Unix) sans
    /// renouvellement reussi.
    CertExpiring {
        domain_id: Uuid,
        expires_at: i64,
    },
    /// Resume des notifications retenues du compte (mis en file par le
    /// plafond, jamais par une transition).
    Digest {
        user_id: Uuid,
    },
}

impl Event {
    /// Cle de l'evenement : une notification par cle et par destinataire.
    pub fn key(&self) -> String {
        match self {
            Event::DeployFailed { deployment_id } => format!("deploy:{deployment_id}:failed"),
            Event::DeploySucceeded { deployment_id } => format!("deploy:{deployment_id}:success"),
            Event::CertFailed {
                domain_id, job_id, ..
            } => format!("cert:{domain_id}:failed:{job_id}"),
            Event::CertExpiring {
                domain_id,
                expires_at,
            } => format!("cert:{domain_id}:expiring:{expires_at}"),
            Event::Digest { user_id } => format!("digest:{user_id}"),
        }
    }

    /// Budget du plafond horaire.
    fn budget(&self) -> &'static str {
        match self {
            Event::DeploySucceeded { .. } => "success",
            Event::Digest { .. } => "digest",
            _ => "alert",
        }
    }
}

/// Gestionnaire des taches `notify`.
pub fn register(handlers: Handlers, state: AppState) -> Handlers {
    handlers.register(JobKind::Notify, move |job| {
        let state = state.clone();
        async move { handle(&state, job).await }
    })
}

/// Met la notification en file sur `conn` (dans la transaction de la
/// transition), si l'instance peut envoyer des e-mails. Un meme evenement
/// deja en file n'est pas duplique.
pub async fn enqueue(conn: &mut PgConnection, state: &AppState, event: Event) -> sqlx::Result<()> {
    if state.public_url().is_none() {
        return Ok(());
    }
    let smtp: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM smtp_settings WHERE enabled)")
            .fetch_one(&mut *conn)
            .await?;
    if !smtp {
        return Ok(());
    }
    queue(conn, &event, None).await
}

/// Tache `notify` de l'evenement (cle `notify:<evenement>`), a `run_at` ou
/// tout de suite.
async fn queue(conn: &mut PgConnection, event: &Event, run_at: Option<f64>) -> sqlx::Result<()> {
    let key = format!("notify:{}", event.key());
    let payload = serde_json::to_value(event).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
    jobs::enqueue_with(
        conn,
        JobKind::Notify,
        payload,
        Some(key),
        Schedule {
            run_at,
            max_attempts: Some(MAX_ATTEMPTS),
        },
    )
    .await?;
    Ok(())
}

/// Reservation `pending` plus vieille que ce delai : sa tache s'est arretee
/// sur sa derniere tentative (6 tentatives, quelques minutes au plus).
const ORPHAN_SECS: f64 = 3600.0;

/// Balayage horaire (tache d'entretien) : une reservation orpheline passe
/// au resume (retenue), et chaque compte qui a des notifications retenues
/// a un resume en file (tentatives d'un resume epuisees, base restauree...).
pub async fn sweep(db: &PgPool, now: f64) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE notification_deliveries SET status = 'suppressed', sent_at = NULL \
         WHERE status = 'pending' AND event IS NOT NULL AND sent_at < to_timestamp($1::float8)",
    )
    .bind(now - ORPHAN_SECS)
    .execute(db)
    .await?;
    let users: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT user_id FROM notification_deliveries WHERE status = 'suppressed'",
    )
    .fetch_all(db)
    .await?;
    for user in users {
        let mut tx = db.begin().await?;
        lock_user(&mut tx, user).await?;
        schedule_digest(&mut tx, user, "alert", now).await?;
        tx.commit().await?;
    }
    Ok(())
}

/// Supprime les envois de plus de 30 jours (tache horaire d'entretien).
pub async fn purge(db: &PgPool, now: f64) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM notification_deliveries WHERE created_at < to_timestamp($1::float8)")
        .bind(now - KEEP_SECS)
        .execute(db)
        .await?;
    Ok(())
}

fn db_error(context: &str, e: sqlx::Error) -> HandlerError {
    tracing::error!(error = %e, "{context}");
    HandlerError::retry(format!("{context} : base de donnees indisponible"))
}

/// Ce que dit l'e-mail, et a qui il peut aller.
struct Notice {
    subject: String,
    body: String,
    /// Chemin de la page du panneau.
    path: String,
    resource: Resource,
    level: Level,
    /// Date d'expiration a afficher dans le fuseau du destinataire.
    expires_at: Option<i64>,
}

impl Notice {
    fn allows(&self, role: &str) -> bool {
        Role::parse(role).is_some_and(|role| authz::allowed(role, self.resource, self.level))
    }
}

#[derive(sqlx::FromRow)]
struct Recipient {
    id: Uuid,
    email: String,
    role: String,
    timezone: String,
    email_enabled: Option<bool>,
    deploy_succeeded: Option<bool>,
    deploy_failed: Option<bool>,
    certificates: Option<bool>,
}

impl Recipient {
    fn wants(&self, event: &Event) -> bool {
        let d = default_prefs(&self.role);
        let wanted = match event {
            Event::DeployFailed { .. } => self.deploy_failed.unwrap_or(d.deploy_failed),
            Event::DeploySucceeded { .. } => self.deploy_succeeded.unwrap_or(d.deploy_succeeded),
            Event::CertFailed { .. } | Event::CertExpiring { .. } => {
                self.certificates.unwrap_or(d.certificates)
            }
            Event::Digest { .. } => true,
        };
        wanted && self.email_enabled.unwrap_or(d.email_enabled)
    }
}

/// Comptes et preferences (filtre et ordre ajoutes par l'appelant).
const RECIPIENTS: &str = "SELECT u.id, u.email, u.role, u.timezone, p.email_enabled, \
     p.deploy_succeeded, p.deploy_failed, p.certificates \
     FROM users u LEFT JOIN notification_prefs p ON p.user_id = u.id";

/// Verrou de la transaction sur les envois du compte (plafond exact).
async fn lock_user(conn: &mut PgConnection, user: Uuid) -> sqlx::Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('notify:' || $1::text, 0))")
        .bind(user)
        .execute(conn)
        .await?;
    Ok(())
}

/// Issue de la reservation d'un envoi.
enum Slot {
    /// Deja envoye, retenu ou resume : rien a faire.
    Done,
    /// Reserve (ou reservation laissee par une tentative interrompue) :
    /// envoyer.
    Send,
    /// Plafond atteint : retenu, resume planifie.
    Held,
}

/// Sous le verrou du compte : envoi deja traite, place dans le budget
/// (reservation `pending`), ou notification retenue et resume planifie.
async fn reserve(db: &PgPool, event: &Event, user: Uuid, now: f64) -> sqlx::Result<Slot> {
    let key = event.key();
    let budget = event.budget();
    let mut tx = db.begin().await?;
    lock_user(&mut tx, user).await?;
    let status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM notification_deliveries WHERE event_key = $1 AND user_id = $2",
    )
    .bind(&key)
    .bind(user)
    .fetch_optional(&mut *tx)
    .await?;
    let slot = match status.as_deref() {
        Some("pending") => Slot::Send,
        Some(_) => Slot::Done,
        None => {
            let used: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM notification_deliveries WHERE user_id = $1 AND budget = $2 \
                 AND status IN ('pending', 'sent') AND sent_at > to_timestamp($3::float8)",
            )
            .bind(user)
            .bind(budget)
            .bind(now - WINDOW_SECS)
            .fetch_one(&mut *tx)
            .await?;
            let held = used >= MAX_PER_HOUR;
            let payload =
                serde_json::to_value(event).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
            sqlx::query(
                "INSERT INTO notification_deliveries \
                 (event_key, user_id, status, budget, event, created_at, sent_at) \
                 VALUES ($1, $2, $3, $4, $5, to_timestamp($6::float8), \
                         CASE WHEN $7 THEN NULL ELSE to_timestamp($6::float8) END)",
            )
            .bind(&key)
            .bind(user)
            .bind(if held { "suppressed" } else { "pending" })
            .bind(budget)
            .bind(payload)
            .bind(now)
            .bind(held)
            .execute(&mut *tx)
            .await?;
            if held {
                schedule_digest(&mut tx, user, budget, now).await?;
                Slot::Held
            } else {
                Slot::Send
            }
        }
    };
    tx.commit().await?;
    Ok(slot)
}

/// Resume du compte quand son budget se libere (plus ancien envoi de
/// l'heure + 1 h), et pas moins d'une heure apres le resume precedent. Un
/// resume deja en file est garde.
async fn schedule_digest(
    conn: &mut PgConnection,
    user: Uuid,
    budget: &str,
    now: f64,
) -> sqlx::Result<()> {
    let at: Option<f64> = sqlx::query_scalar(
        "SELECT greatest( \
           (SELECT extract(epoch FROM min(sent_at))::float8 FROM notification_deliveries \
            WHERE user_id = $1 AND budget = $2 AND status IN ('pending', 'sent') \
            AND sent_at > to_timestamp($3::float8)), \
           (SELECT extract(epoch FROM max(sent_at))::float8 FROM notification_deliveries \
            WHERE user_id = $1 AND budget = 'digest' AND status = 'sent')) + $4",
    )
    .bind(user)
    .bind(budget)
    .bind(now - WINDOW_SECS)
    .bind(WINDOW_SECS)
    .fetch_one(&mut *conn)
    .await?;
    // Budget libre et aucun resume recent : tout de suite.
    let at = at.unwrap_or(now).max(now);
    queue(conn, &Event::Digest { user_id: user }, Some(at)).await
}

/// Tache `notify` : envoie la notification a chaque destinataire qui ne l'a
/// pas encore recue, ou le resume d'un compte. Un envoi en echec demande
/// une reprise (les autres destinataires sont servis).
pub async fn handle(state: &AppState, job: Job) -> Result<(), HandlerError> {
    let event: Event = serde_json::from_value(job.payload.clone())
        .map_err(|_| HandlerError::fatal("charge utile de notification illisible"))?;
    let mailer = match mail::ready(state).await {
        Ok(Some(mailer)) => mailer,
        // SMTP coupe depuis la mise en file : rien a envoyer.
        Ok(None) => return Ok(()),
        Err(e) => {
            tracing::warn!(error = %e, "reglages SMTP illisibles");
            return Err(HandlerError::retry("reglages SMTP illisibles"));
        }
    };
    if let Event::Digest { user_id } = event {
        return digest(state, job.id, mailer, user_id).await;
    }
    let db = &state.db;
    let Some(notice) = notice(db, &event)
        .await
        .map_err(|e| db_error("lecture de l'evenement", e))?
    else {
        // Supprime entre-temps, ou issue deja remplacee : rien a dire.
        return Ok(());
    };
    let link = state.public_link(&notice.path);
    let recipients: Vec<Recipient> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{RECIPIENTS} ORDER BY u.created_at"
    )))
    .fetch_all(db)
    .await
    .map_err(|e| db_error("lecture des destinataires", e))?;
    let key = event.key();
    let mut failed = 0;
    for r in recipients {
        if !notice.allows(&r.role) || !r.wants(&event) {
            continue;
        }
        let now = state.clock.unix_secs();
        let slot = reserve(db, &event, r.id, now)
            .await
            .map_err(|e| db_error("plafond des notifications", e))?;
        if !matches!(slot, Slot::Send) {
            continue;
        }
        let message = compose(&notice, &link, &r.timezone, &r.email);
        let sent = state.mail.send(mailer.clone(), message).await;
        if let Err(e) = &sent {
            tracing::warn!(error = %e, event = %key, "notification non envoyee");
            failed += 1;
        }
        // Envoye : `sent`. Echec : reservation rendue, la reprise reessaie.
        let query = if sent.is_ok() {
            "UPDATE notification_deliveries SET status = 'sent' \
             WHERE event_key = $1 AND user_id = $2 AND status = 'pending'"
        } else {
            "DELETE FROM notification_deliveries \
             WHERE event_key = $1 AND user_id = $2 AND status = 'pending'"
        };
        sqlx::query(query)
            .bind(&key)
            .bind(r.id)
            .execute(db)
            .await
            .map_err(|e| db_error("enregistrement de l'envoi", e))?;
    }
    if failed > 0 {
        return Err(HandlerError::retry(format!(
            "{failed} notification(s) non envoyee(s)"
        )));
    }
    Ok(())
}

/// Resume des notifications retenues du compte : chaque alerte encore
/// d'actualite avec son lien, le nombre de deploiements reussis. Marque
/// `digested` apres l'envoi (un arret entre les deux renvoie le resume).
///
/// La tache libere d'abord sa cle : une notification retenue pendant
/// l'envoi planifie alors son propre resume (sinon dedupliquee sur la
/// tache en cours, elle attendrait un autre evenement). Un resume parti il
/// y a moins d'une heure replanifie celui-ci a l'heure echue.
async fn digest(
    state: &AppState,
    job: shared::ids::JobId,
    mailer: Mailer,
    user: Uuid,
) -> Result<(), HandlerError> {
    let db = &state.db;
    sqlx::query("UPDATE jobs SET dedupe_key = NULL WHERE id = $1")
        .bind(job)
        .execute(db)
        .await
        .map_err(|e| db_error("cle du resume", e))?;
    let last: Option<f64> = sqlx::query_scalar(
        "SELECT extract(epoch FROM max(sent_at))::float8 FROM notification_deliveries \
         WHERE user_id = $1 AND budget = 'digest' AND status = 'sent'",
    )
    .bind(user)
    .fetch_one(db)
    .await
    .map_err(|e| db_error("dernier resume", e))?;
    if let Some(next) = last.map(|l| l + WINDOW_SECS)
        && next > state.clock.unix_secs()
    {
        let mut conn = db
            .acquire()
            .await
            .map_err(|e| db_error("connexion a la base", e))?;
        queue(&mut conn, &Event::Digest { user_id: user }, Some(next))
            .await
            .map_err(|e| db_error("planification du resume", e))?;
        return Ok(());
    }
    let recipient: Option<Recipient> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{RECIPIENTS} WHERE u.id = $1")))
            .bind(user)
            .fetch_optional(db)
            .await
            .map_err(|e| db_error("lecture du destinataire", e))?;
    let Some(r) = recipient else {
        return Ok(());
    };
    let held: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT event_key, event FROM notification_deliveries \
         WHERE user_id = $1 AND status = 'suppressed' ORDER BY created_at",
    )
    .bind(user)
    .fetch_all(db)
    .await
    .map_err(|e| db_error("notifications retenues", e))?;
    if held.is_empty() {
        return Ok(());
    }
    let mut alerts = Vec::new();
    let mut successes = 0;
    for (_, payload) in &held {
        let Ok(event) = serde_json::from_value::<Event>(payload.clone()) else {
            continue;
        };
        // Supprimee ou issue remplacee depuis : plus rien a dire.
        let Some(notice) = notice(db, &event)
            .await
            .map_err(|e| db_error("lecture de l'evenement", e))?
        else {
            continue;
        };
        if !notice.allows(&r.role) || !r.wants(&event) {
            continue;
        }
        if matches!(event, Event::DeploySucceeded { .. }) {
            successes += 1;
        } else {
            alerts.push((notice.subject, state.public_link(&notice.path)));
        }
    }
    let total = alerts.len() + successes;
    if total > 0 {
        let message = digest_mail(&r.email, &alerts, successes);
        if let Err(e) = state.mail.send(mailer, message).await {
            tracing::warn!(error = %e, "resume des notifications non envoye");
            return Err(HandlerError::retry("resume des notifications non envoye"));
        }
    }
    let keys: Vec<String> = held.into_iter().map(|(k, _)| k).collect();
    let now = state.clock.unix_secs();
    let mut tx = db
        .begin()
        .await
        .map_err(|e| db_error("enregistrement du resume", e))?;
    sqlx::query(
        "UPDATE notification_deliveries SET status = 'digested' \
         WHERE user_id = $1 AND status = 'suppressed' AND event_key = ANY($2)",
    )
    .bind(user)
    .bind(&keys)
    .execute(&mut *tx)
    .await
    .map_err(|e| db_error("enregistrement du resume", e))?;
    if total > 0 {
        sqlx::query(
            "INSERT INTO notification_deliveries \
             (event_key, user_id, status, budget, created_at, sent_at) \
             VALUES ($1, $2, 'sent', 'digest', to_timestamp($3::float8), to_timestamp($3::float8)) \
             ON CONFLICT (event_key, user_id) DO NOTHING",
        )
        .bind(format!("digest:{user}:{}", now as i64))
        .bind(user)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_error("enregistrement du resume", e))?;
    }
    // Retenues pendant l'envoi : resume suivant (une heure plus tard).
    let remaining: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM notification_deliveries \
         WHERE user_id = $1 AND status = 'suppressed')",
    )
    .bind(user)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| db_error("notifications retenues", e))?;
    if remaining {
        queue(
            &mut tx,
            &Event::Digest { user_id: user },
            Some(now + WINDOW_SECS),
        )
        .await
        .map_err(|e| db_error("planification du resume", e))?;
    }
    tx.commit()
        .await
        .map_err(|e| db_error("enregistrement du resume", e))
}

fn digest_mail(to: &str, alerts: &[(String, String)], successes: usize) -> OutgoingMail {
    let total = alerts.len() + successes;
    let s = |n: usize| if n > 1 { "s" } else { "" };
    let mut text = format!(
        "Bonjour,\n\n{total} notification{p} {verb} été retenue{p} : au plus {MAX_PER_HOUR} \
         e-mails par heure pour les alertes, autant pour les déploiements réussis.\n",
        p = s(total),
        verb = if total > 1 { "ont" } else { "a" },
    );
    if !alerts.is_empty() {
        text.push('\n');
        for (subject, link) in alerts {
            let _ = writeln!(text, "- {} : {link}", one_line(subject));
        }
    }
    if successes > 0 {
        let _ = writeln!(
            text,
            "\n{successes} déploiement{p} réussi{p} (détail dans le panneau).",
            p = s(successes)
        );
    }
    text.push_str(
        "\nVous recevez cet e-mail selon vos préférences de notification \
         (Profil › Notifications).\n",
    );
    OutgoingMail {
        to: to.to_string(),
        subject: format!("Ciphera : {total} notification{p} retenue{p}", p = s(total)),
        text,
    }
}

/// (numero, statut, etape, commit, projet, environnement).
type DeploymentRow = (i64, String, Option<String>, Option<String>, String, String);

/// Objet et texte de l'evenement ; `None` si son objet n'existe plus ou si
/// l'issue notifiee n'est plus la sienne (echec presume devenu reussite).
async fn notice(db: &PgPool, event: &Event) -> sqlx::Result<Option<Notice>> {
    match event {
        Event::DeployFailed { deployment_id } | Event::DeploySucceeded { deployment_id } => {
            let row: Option<DeploymentRow> = sqlx::query_as(
                "SELECT d.number, d.status, d.stage, d.commit_sha, p.name, e.name \
                     FROM deployments d JOIN projects p ON p.id = d.project_id \
                     JOIN environments e ON e.id = d.environment_id WHERE d.id = $1",
            )
            .bind(deployment_id)
            .fetch_optional(db)
            .await?;
            let Some((number, status, stage, commit, project, environment)) = row else {
                return Ok(None);
            };
            let failed = matches!(event, Event::DeployFailed { .. });
            if status != if failed { "failed" } else { "success" } {
                return Ok(None);
            }
            let commit = commit
                .map(|c| format!(" (commit {})", &c[..c.len().min(7)]))
                .unwrap_or_default();
            let (subject, body) = if failed {
                let stage = stage
                    .map(|s| format!(" à l'étape « {s} »"))
                    .unwrap_or_default();
                (
                    format!("Échec du déploiement #{number} de {project}"),
                    format!(
                        "Le déploiement #{number} du projet {project} ({environment}){commit} \
                         a échoué{stage}.\n\
                         La version précédente reste en ligne. La cause et les logs sont \
                         dans le panneau :"
                    ),
                )
            } else {
                (
                    format!("Déploiement #{number} de {project} réussi"),
                    format!(
                        "Le déploiement #{number} du projet {project} ({environment}){commit} \
                         est en ligne."
                    ),
                )
            };
            Ok(Some(Notice {
                subject,
                body,
                path: format!("/deployments/{deployment_id}"),
                resource: Resource::Deployments,
                level: Level::Read,
                expires_at: None,
            }))
        }
        Event::Digest { .. } => Ok(None),
        Event::CertFailed { domain_id, .. } | Event::CertExpiring { domain_id, .. } => {
            let row: Option<(String, bool, Option<Uuid>, Option<i64>)> = sqlx::query_as(
                "SELECT host, panel, project_id, extract(epoch FROM cert_expires_at)::bigint \
                 FROM domains WHERE id = $1",
            )
            .bind(domain_id)
            .fetch_optional(db)
            .await?;
            let Some((host, panel, project, current)) = row else {
                return Ok(None);
            };
            let (path, resource, level) = match (panel, project) {
                (true, _) => (
                    "/settings/panel-domain".to_string(),
                    Resource::Settings,
                    Level::Admin,
                ),
                (false, Some(p)) => (
                    format!("/projects/{p}/domains"),
                    Resource::Domains,
                    Level::Read,
                ),
                (false, None) => return Ok(None),
            };
            let (subject, body, expires_at) = match event {
                Event::CertExpiring { expires_at, .. } => {
                    // Renouvele entre-temps : plus rien a signaler.
                    if current != Some(*expires_at) {
                        return Ok(None);
                    }
                    (
                        format!("Le certificat de {host} expire bientôt"),
                        format!(
                            "Le certificat HTTPS de {host} n'a pas pu être renouvelé et expire \
                             le {{expiry}}. Ciphera réessaie automatiquement ; vérifiez le DNS \
                             du domaine dans le panneau :"
                        ),
                        Some(*expires_at),
                    )
                }
                Event::CertFailed { renewal: true, .. } => (
                    format!("Renouvellement du certificat de {host} en échec"),
                    format!(
                        "Le renouvellement du certificat HTTPS de {host} a échoué. Le certificat \
                         actuel reste valide jusqu'au {{expiry}} ; Ciphera réessaie \
                         automatiquement. Détails dans le panneau :"
                    ),
                    current,
                ),
                _ => (
                    format!("Échec du certificat de {host}"),
                    format!(
                        "Le certificat HTTPS de {host} n'a pas pu être émis. Ciphera réessaie \
                         automatiquement ; vérifiez le DNS du domaine dans le panneau :"
                    ),
                    None,
                ),
            };
            Ok(Some(Notice {
                subject,
                body,
                path,
                resource,
                level,
                expires_at,
            }))
        }
    }
}

/// Date d'expiration dans le fuseau du destinataire (UTC a defaut).
fn format_expiry(at: i64, timezone: &str) -> String {
    let Ok(ts) = jiff::Timestamp::from_second(at) else {
        return "(date inconnue)".to_string();
    };
    let tz = jiff::tz::TimeZone::get(timezone).unwrap_or(jiff::tz::TimeZone::UTC);
    ts.to_zoned_datetime(tz)
        .strftime("%d/%m/%Y à %H:%M (%Z)")
        .to_string()
}

/// Sans caractere de controle (objet d'un en-tete).
fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn compose(notice: &Notice, link: &str, timezone: &str, to: &str) -> OutgoingMail {
    let body = match notice.expires_at {
        Some(at) => notice
            .body
            .replace("{expiry}", &format_expiry(at, timezone)),
        None => notice.body.replace("{expiry}", "sa date d'expiration"),
    };
    OutgoingMail {
        to: to.to_string(),
        subject: one_line(&notice.subject),
        text: format!(
            "Bonjour,\n\n{body}\n\n{link}\n\nVous recevez cet e-mail selon vos \
             préférences de notification (Profil › Notifications).\n"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_payload_round_trips_and_keys_are_distinct() {
        let id = Uuid::nil();
        let events = [
            Event::DeployFailed { deployment_id: id },
            Event::DeploySucceeded { deployment_id: id },
            Event::CertFailed {
                domain_id: id,
                job_id: id,
                renewal: false,
            },
            Event::CertExpiring {
                domain_id: id,
                expires_at: 1,
            },
        ];
        let keys: std::collections::HashSet<_> = events.iter().map(Event::key).collect();
        assert_eq!(keys.len(), events.len());
        for e in events {
            let v = serde_json::to_value(&e).unwrap();
            assert_eq!(serde_json::from_value::<Event>(v).unwrap(), e);
        }
    }

    #[test]
    fn the_subject_never_carries_control_characters() {
        let notice = Notice {
            subject: "Échec de web
Bcc: x@example.com"
                .into(),
            body: "corps".into(),
            path: "/deployments/x".into(),
            resource: Resource::Deployments,
            level: Level::Read,
            expires_at: None,
        };
        let mail = compose(
            &notice,
            "https://p.example.com/deployments/x",
            "UTC",
            "a@example.com",
        );
        assert!(
            !mail.subject.chars().any(char::is_control),
            "{:?}",
            mail.subject
        );
        assert!(
            mail.subject.contains("Bcc: x@example.com"),
            "texte garde, sur une ligne"
        );
    }

    #[test]
    fn expiry_is_shown_in_the_recipient_time_zone() {
        // 2026-01-01T00:00:00Z
        assert_eq!(
            format_expiry(1_767_225_600, "Europe/Paris"),
            "01/01/2026 à 01:00 (CET)"
        );
        assert_eq!(
            format_expiry(1_767_225_600, "pas/un-fuseau"),
            "01/01/2026 à 00:00 (UTC)"
        );
    }
}
