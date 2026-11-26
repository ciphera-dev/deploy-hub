//! Emission et renouvellement des certificats (tache `acme_issue`).
//!
//! Le plan de controle ne voit jamais de cle : il envoie la table des routes
//! puis `IssueCertificate` a l'agent du serveur, qui la transmet au proxy du
//! noeud (ACME HTTP-01, certificat chiffre au repos). L'issue revient en
//! `AgentToControl::Certificate`.
//!
//! Limites face aux quotas de Let's Encrypt (compte partage par toute
//! l'instance) :
//! - **une seule tache par domaine** (cle `acme:<domaine>`), qu'elle soit
//!   une premiere emission, une reprise ou un renouvellement : la tache en
//!   cours libere sa cle puis planifie la suivante ;
//! - un certificat valide a plus de 30 jours de son expiration n'est jamais
//!   re-emis (verification repetee, renouvellement manuel refuse) ;
//! - le DNS est **reverifie juste avant chaque emission** : apres
//!   [`DNS_NEGATIVE_VERDICTS`] verdicts negatifs consecutifs (espaces par le
//!   delai croissant), le domaine passe en erreur DNS, sa route est retiree
//!   et la chaine s'arrete (une nouvelle verification la relance) ; un
//!   verdict positif remet le compteur a zero. Une panne du resolveur n'est
//!   jamais un verdict. Le domaine du panneau confirme ne passe jamais en
//!   erreur DNS : seulement reprise et notification ;
//! - un echec ACME est repris avec un delai croissant borne (5 min doublees,
//!   24 h au plus), jamais en rafale.
//!
//! Une route retiree (DNS en erreur, autre serveur) garde son certificat sur
//! le proxy ; seule la suppression du domaine l'efface
//! ([`super::remove_certificates`]). Une verification reussie d'un domaine au
//! certificat encore valide demande au proxy s'il le detient
//! (`CheckCertificate`) : absent (autre serveur, fichier perdu), il est
//! re-emis.
//!
//! Un echec met en file une notification e-mail (`domain::notifications`) :
//! premier echec d'une chaine, puis expiration a moins de 7 jours.
//!
//! Une panne passagere (aucun serveur, file de l'agent pleine) replanifie la
//! tache une minute plus tard sans compter d'echec. Une chaine qui s'arrete
//! malgre tout (tentatives epuisees) apparait comme erreur du certificat
//! (voir `select` du module parent).
//!
//! Aucune connexion a la base n'est tenue pendant l'attente de l'issue
//! (jusqu'a 200 s) ; la file limite les emissions simultanees
//! (`jobs::ACME_CONCURRENCY`).

use std::time::Duration;

use proto::{AgentToControl, CertificateOutcome, ControlToAgent, Route};
use serde_json::{Value, json};
use shared::ids::ServerId;
use sqlx::PgConnection;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use super::dns;
use crate::agents::registry::SendError;
use crate::domain::notifications::{self, Event};
use crate::jobs::worker::{HandlerError, Handlers};
use crate::jobs::{self, Job, JobKind, Schedule};
use crate::state::AppState;

/// Attente maximale de l'issue (le proxy borne l'emission a 180 s).
pub const WAIT: Duration = Duration::from_secs(200);
/// Renouvellement 30 jours avant la fin de validite.
pub const RENEW_BEFORE_SECS: i64 = 30 * 86_400;
/// Tentatives d'une tache (reprises des seules pannes de la base ; un echec
/// ACME est replanifie par [`retry_delay`]).
pub const MAX_ATTEMPTS: i32 = 10;
/// Premier delai de reprise apres un echec ACME.
const RETRY_BASE_SECS: i64 = 300;
/// Delai de reprise maximal.
const RETRY_MAX_SECS: i64 = 86_400;
/// Delai avant un nouvel essai apres une panne passagere (aucun serveur,
/// file de l'agent pleine).
pub const TRANSIENT_SECS: i64 = 60;
const MAX_TEXT: usize = 300;
/// Verdicts DNS negatifs consecutifs avant l'erreur DNS d'un domaine.
pub const DNS_NEGATIVE_VERDICTS: i16 = 3;

/// Gestionnaire des taches `acme_issue`.
pub fn register(handlers: Handlers, state: AppState) -> Handlers {
    handlers.register(JobKind::AcmeIssue, move |job| {
        let state = state.clone();
        async move { issue(&state, job).await }
    })
}

/// Cle de deduplication : une tache d'emission au plus par domaine.
fn key(id: Uuid) -> String {
    format!("acme:{id}")
}

fn payload(id: Uuid, failures: u32) -> Value {
    json!({ "domain_id": id, "failures": failures })
}

/// Delai avant la reprise qui suit `failures` echecs consecutifs (0 : le
/// premier) : 5 min doublees, 24 h au plus.
pub fn retry_delay(failures: u32) -> i64 {
    RETRY_BASE_SECS
        .saturating_mul(1i64 << failures.min(20))
        .min(RETRY_MAX_SECS)
}

/// Le certificat est-il encore valide a plus de 30 jours (rien a emettre) ?
pub fn still_valid(expires_at: Option<i64>, now: i64) -> bool {
    expires_at.is_some_and(|e| e.saturating_sub(now) > RENEW_BEFORE_SECS)
}

/// Fin de validite du certificat du domaine (secondes Unix).
pub async fn expires_at(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar(
        "SELECT extract(epoch FROM cert_expires_at)::bigint FROM domains WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
    .map(Option::flatten)
}

/// Replanifie tout de suite la tache du domaine (verification DNS puis
/// emission ou controle du certificat) et remet le compteur de verdicts
/// negatifs a zero : revérification demandee par le proprietaire.
pub async fn recheck(conn: &mut PgConnection, id: Uuid, now: i64) -> sqlx::Result<()> {
    sqlx::query("UPDATE domains SET dns_failures = 0 WHERE id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    if !request(&mut *conn, id, now).await? {
        check(&mut *conn, id).await?;
    }
    // DNS confirme bon : une erreur de verification plus ancienne sur un
    // certificat encore actif n'est plus d'actualite.
    sqlx::query(
        "UPDATE domains SET cert_error = NULL, updated_at = now() \
         WHERE id = $1 AND cert_status = 'active' AND cert_error IS NOT NULL",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE jobs SET run_at = now()          WHERE dedupe_key = $1 AND status = 'queued' AND run_at > now()",
    )
    .bind(key(id))
    .execute(conn)
    .await
    .map(|_| ())
}

/// Demande l'emission du certificat du domaine, sauf s'il est encore valide
/// a plus de 30 jours (`false`). Une tache deja planifiee pour ce domaine
/// (reprise, renouvellement) est conservee, avec son echeance. Marque
/// `issuing` quand l'emission est due tout de suite.
pub async fn request(conn: &mut PgConnection, id: Uuid, now: i64) -> sqlx::Result<bool> {
    if still_valid(expires_at(conn, id).await?, now) {
        return Ok(false);
    }
    let job = jobs::enqueue_with(
        conn,
        JobKind::AcmeIssue,
        payload(id, 0),
        Some(key(id)),
        Schedule {
            run_at: None,
            max_attempts: Some(MAX_ATTEMPTS),
        },
    )
    .await?;
    sqlx::query(
        "UPDATE domains SET cert_status = 'issuing', cert_error = NULL, updated_at = now() \
         WHERE id = $1 AND cert_status <> 'active' \
         AND EXISTS (SELECT 1 FROM jobs WHERE id = $2 AND run_at <= now())",
    )
    .bind(id)
    .bind(job)
    .execute(conn)
    .await?;
    Ok(true)
}

/// Verification reussie d'un domaine au certificat encore valide : la tache
/// du domaine (renouvellement planifie) est avancee a maintenant pour
/// demander au proxy s'il detient le certificat, sans rien emettre.
pub async fn check(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<()> {
    jobs::enqueue_with(
        &mut *conn,
        JobKind::AcmeIssue,
        payload(id, 0),
        Some(key(id)),
        Schedule {
            run_at: None,
            max_attempts: Some(MAX_ATTEMPTS),
        },
    )
    .await?;
    sqlx::query(
        "UPDATE jobs SET run_at = now() \
         WHERE dedupe_key = $1 AND status = 'queued' AND run_at > now()",
    )
    .bind(key(id))
    .execute(conn)
    .await?;
    Ok(())
}

/// Texte stockable : sans caractere de controle, borne.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_TEXT)
        .collect()
}

fn db_error(context: &str, e: sqlx::Error) -> HandlerError {
    tracing::error!(error = %e, "{context}");
    HandlerError::retry(format!("{context} : base de donnees indisponible"))
}

/// Echeance du renouvellement : J-30, ou a mi-chemin pour un certificat
/// plus court.
pub fn renew_at(now: i64, not_after: i64) -> i64 {
    let at = not_after.saturating_sub(RENEW_BEFORE_SECS);
    if at > now {
        at
    } else {
        now.saturating_add((not_after.saturating_sub(now) / 2).max(3600))
    }
}

/// Planifie la tache suivante du domaine : la tache courante libere d'abord
/// sa cle (une seule tache par domaine).
async fn schedule_next(
    conn: &mut PgConnection,
    job: &Job,
    id: Uuid,
    run_at: i64,
    failures: u32,
) -> Result<(), HandlerError> {
    sqlx::query("UPDATE jobs SET dedupe_key = NULL WHERE id = $1")
        .bind(job.id)
        .execute(&mut *conn)
        .await
        .map_err(|e| db_error("cle de la tache", e))?;
    jobs::enqueue_with(
        conn,
        JobKind::AcmeIssue,
        payload(id, failures),
        Some(key(id)),
        Schedule {
            run_at: Some(run_at as f64),
            max_attempts: Some(MAX_ATTEMPTS),
        },
    )
    .await
    .map_err(|e| db_error("planification du certificat", e))?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct Target {
    host: String,
    generated: bool,
    dns_ok: bool,
    /// Sans environnement : domaine du panneau, servi par le serveur local.
    environment: Option<Uuid>,
    expires_at: Option<i64>,
    /// Domaine du panneau confirme : jamais mis en erreur DNS (le panneau
    /// reste servi), un verdict negatif passe par `failed`.
    panel_confirmed: bool,
}

async fn target(state: &AppState, id: Uuid) -> Result<Option<Target>, HandlerError> {
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(|e| db_error("connexion a la base", e))?;
    sqlx::query_as::<_, Target>(
        "SELECT host, generated, dns_status = 'ok' AS dns_ok, environment_id AS environment, \
         extract(epoch FROM cert_expires_at)::bigint AS expires_at,          panel_confirmed_at IS NOT NULL AS panel_confirmed FROM domains WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|e| db_error("lecture du domaine", e))
}

/// Gestionnaire : emet (ou renouvelle) le certificat du domaine.
pub async fn issue(state: &AppState, job: Job) -> Result<(), HandlerError> {
    let id = job
        .payload
        .get("domain_id")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| HandlerError::fatal("charge utile invalide"))?;
    let failures = job
        .payload
        .get("failures")
        .and_then(Value::as_u64)
        .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX));
    let now = state.clock.unix_secs() as i64;
    // Domaine supprime, genere (HTTP seulement) ou DNS en erreur : rien a
    // faire, la chaine s'arrete (une nouvelle verification la relance).
    let Some(t) = target(state, id).await? else {
        return Ok(());
    };
    if t.generated || !t.dns_ok {
        return Ok(());
    }
    // Certificat encore valide : on demande seulement au proxy s'il le
    // detient (verification apres une route retiree), sans appel a l'autorite.
    let checking = still_valid(t.expires_at, now);

    // DNS reverifie avant tout appel au proxy (hors connexion a la base).
    let (v4, v6) = state.cfg.node_ips();
    let checked = if v4.is_none() && v6.is_none() {
        Ok(Some(
            "IP publique du nœud inconnue : définissez CIPHERA_PUBLIC_IP.".to_string(),
        ))
    } else {
        state
            .dns
            .resolve(&t.host)
            .await
            .map(|ips| dns::verdict(&t.host, v4, v6, &ips))
    };
    match checked {
        Err(e) => {
            tracing::warn!(host = %t.host, "resolution DNS impossible : {e}");
            let reason = "Résolution DNS impossible pour le moment : nouvel essai automatique.";
            return failed(state, &job, id, &t.host, now, failures, reason).await;
        }
        Ok(Some(why)) => {
            // Domaine du panneau confirme : jamais d'erreur DNS, seulement
            // une reprise avec delai croissant et la notification.
            if t.panel_confirmed {
                return failed(state, &job, id, &t.host, now, failures, &clean(&why)).await;
            }
            // Verdict negatif : le domaine ne bouge qu'apres
            // `DNS_NEGATIVE_VERDICTS` verdicts d'affilee, espaces par le
            // delai croissant de la tache.
            return negative_verdict(state, &job, id, &t.host, now, failures, &why).await;
        }
        Ok(None) => reset_negatives(state, id).await?,
    }

    let placed = {
        let mut conn = state
            .db
            .acquire()
            .await
            .map_err(|e| db_error("connexion a la base", e))?;
        let server = match t.environment {
            Some(environment) => super::server_of(&mut conn, environment).await,
            None => super::local_server(&mut conn).await,
        };
        match server.map_err(|e| db_error("serveur du domaine", e))? {
            Some(server) => {
                let routes = super::route_table(&mut conn, &state.cfg, server)
                    .await
                    .map_err(|e| db_error("table des routes", e))?;
                Some((server, routes))
            }
            None => None,
        }
    };
    let Some((server, routes)) = placed else {
        return later(state, &job, id, now, failures, &t.host, "aucun serveur").await;
    };

    if checking {
        let order = ControlToAgent::CheckCertificate {
            host: t.host.clone(),
        };
        match exchange(state, server, routes.clone(), order, &t.host).await {
            Err(Busy) => {
                return later(state, &job, id, now, failures, &t.host, "agent occupe").await;
            }
            Ok(Some(CertificateOutcome::Issued { not_after, issuer })) => {
                let now = state.clock.unix_secs() as i64;
                return installed(state, &job, id, &t.host, server, now, not_after, &issuer).await;
            }
            Ok(Some(CertificateOutcome::Missing)) => {
                // Le proxy n'a pas le certificat (autre serveur, fichier
                // perdu) : il n'est plus affiche actif, on l'emet a nouveau.
                let mut conn = state
                    .db
                    .acquire()
                    .await
                    .map_err(|e| db_error("connexion a la base", e))?;
                sqlx::query(
                    "UPDATE domains SET cert_status = 'issuing', cert_expires_at = NULL, \
                     cert_issuer = NULL, cert_error = NULL, updated_at = now() WHERE id = $1",
                )
                .bind(id)
                .execute(&mut *conn)
                .await
                .map_err(|e| db_error("certificat du domaine", e))?;
                tracing::warn!(host = %t.host, "certificat absent du proxy : nouvelle emission");
            }
            Ok(_) => {
                // Agent injoignable ou reponse inattendue : le certificat
                // reste tel quel, nouvelle verification plus tard.
                let now = state.clock.unix_secs() as i64;
                let mut tx = state
                    .db
                    .begin()
                    .await
                    .map_err(|e| db_error("connexion a la base", e))?;
                let next = now.saturating_add(retry_delay(failures));
                schedule_next(&mut tx, &job, id, next, failures.saturating_add(1)).await?;
                tx.commit()
                    .await
                    .map_err(|e| db_error("verification du certificat", e))?;
                return Ok(());
            }
        }
    }

    let order = ControlToAgent::IssueCertificate {
        host: t.host.clone(),
    };
    let outcome = match exchange(state, server, routes, order, &t.host).await {
        Ok(outcome) => outcome,
        Err(Busy) => {
            return later(state, &job, id, now, failures, &t.host, "agent occupe").await;
        }
    };
    let now = state.clock.unix_secs() as i64;
    match outcome {
        Some(CertificateOutcome::Issued { not_after, issuer }) => {
            installed(state, &job, id, &t.host, server, now, not_after, &issuer).await
        }
        Some(CertificateOutcome::Failed { reason }) => {
            failed(state, &job, id, &t.host, now, failures, &clean(&reason)).await
        }
        Some(other) => {
            let reason =
                format!("Réponse inattendue du proxy ({other:?}) : nouvel essai automatique.");
            failed(state, &job, id, &t.host, now, failures, &reason).await
        }
        None => {
            let reason = "Agent du serveur injoignable : nouvel essai automatique.";
            failed(state, &job, id, &t.host, now, failures, reason).await
        }
    }
}

/// File d'envoi de l'agent pleine : l'ordre n'est pas parti.
struct Busy;

/// Envoie la table des routes puis `order` a l'agent de `server` et attend
/// l'issue pour `host` (`None` : agent injoignable ou sans reponse dans le
/// delai). Le proxy attend brievement la route si la table arrive apres.
async fn exchange(
    state: &AppState,
    server: Uuid,
    routes: Vec<Route>,
    order: ControlToAgent,
    host: &str,
) -> Result<Option<CertificateOutcome>, Busy> {
    // Abonne avant l'envoi : l'issue ne peut pas etre manquee.
    let mut events = state.agents.subscribe();
    let sent = state
        .agents
        .send(ServerId(server), ControlToAgent::ApplyRoutes(routes))
        .and_then(|()| state.agents.send(ServerId(server), order));
    match sent {
        Err(SendError::NotConnected) => Ok(None),
        Err(SendError::Busy) => Err(Busy),
        Ok(()) => {
            let wanted = host.to_string();
            Ok(tokio::time::timeout(WAIT, async move {
                loop {
                    match events.recv().await {
                        Ok((s, AgentToControl::Certificate { host, outcome }))
                            if s.0 == server && host == wanted =>
                        {
                            return Some(outcome);
                        }
                        Ok(_) | Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => return None,
                    }
                }
            })
            .await
            .ok()
            .flatten())
        }
    }
}

/// Panne passagere (aucun serveur, file de l'agent pleine) : nouvel essai
/// dans une minute, sans compter d'echec ni consommer de tentative.
async fn later(
    state: &AppState,
    job: &Job,
    id: Uuid,
    now: i64,
    failures: u32,
    host: &str,
    why: &str,
) -> Result<(), HandlerError> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| db_error("connexion a la base", e))?;
    schedule_next(
        &mut tx,
        job,
        id,
        now.saturating_add(TRANSIENT_SECS),
        failures,
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| db_error("planification du certificat", e))?;
    tracing::info!(%host, "certificat : {why}, nouvel essai dans une minute");
    Ok(())
}

/// Certificat detenu par le proxy : actif, renouvellement planifie. Domaine
/// supprime entre-temps : le certificat est retire du proxy.
#[allow(clippy::too_many_arguments)]
async fn installed(
    state: &AppState,
    job: &Job,
    id: Uuid,
    host: &str,
    server: Uuid,
    now: i64,
    not_after: i64,
    issuer: &str,
) -> Result<(), HandlerError> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| db_error("connexion a la base", e))?;
    let panel: Option<bool> = sqlx::query_scalar(
        "UPDATE domains SET cert_status = 'active', \
         cert_expires_at = to_timestamp($2::float8), cert_issuer = $3, cert_error = NULL, \
         updated_at = now() WHERE id = $1 RETURNING panel",
    )
    .bind(id)
    .bind(not_after as f64)
    .bind(clean(issuer))
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| db_error("certificat du domaine", e))?;
    let Some(panel) = panel else {
        drop(tx);
        // Ordre indexe par hote : si le meme hote a ete ajoute de nouveau
        // entre-temps, le retirer effacerait le certificat du nouveau
        // domaine (ce certificat lui sert, sa propre tache le suivra).
        let re_added: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM domains WHERE host = $1)")
                .bind(host)
                .fetch_one(&state.db)
                .await
                .map_err(|e| db_error("domaine supprime", e))?;
        if re_added {
            tracing::info!(%host, "domaine supprime puis ajoute de nouveau : certificat garde");
            return Ok(());
        }
        let order = ControlToAgent::RemoveCertificate {
            host: host.to_string(),
        };
        if let Err(e) = state.agents.send(ServerId(server), order) {
            tracing::warn!(%host, "certificat d'un domaine supprime non retire : {e}");
        }
        return Ok(());
    };
    schedule_next(&mut tx, job, id, renew_at(now, not_after), 0).await?;
    tx.commit()
        .await
        .map_err(|e| db_error("certificat du domaine", e))?;
    tracing::info!(%host, not_after, "certificat actif");
    if panel {
        // Certificat du panneau actif : sa route recoit son amont.
        super::push_routes(state).await;
    }
    Ok(())
}

/// Compte un verdict DNS negatif de plus (dans la transaction de la suite
/// donnee au verdict) ; renvoie le total consecutif.
async fn count_negative(conn: &mut PgConnection, id: Uuid) -> Result<i16, HandlerError> {
    sqlx::query_scalar(
        "UPDATE domains SET dns_failures = LEAST(dns_failures + 1, 100) WHERE id = $1 \
         RETURNING dns_failures",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
    .map(|n| n.unwrap_or(0))
    .map_err(|e| db_error("verdicts DNS du domaine", e))
}

/// Verdict DNS negatif d'un domaine (hors panneau confirme) : le compteur et
/// la suite (reprise avec delai croissant, ou erreur DNS au
/// [`DNS_NEGATIVE_VERDICTS`]-ieme verdict) sont ecrits dans une seule
/// transaction.
async fn negative_verdict(
    state: &AppState,
    job: &Job,
    id: Uuid,
    host: &str,
    now: i64,
    failures: u32,
    why: &str,
) -> Result<(), HandlerError> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| db_error("connexion a la base", e))?;
    if count_negative(&mut tx, id).await? >= DNS_NEGATIVE_VERDICTS {
        dns_moved(&mut tx, id, now, why).await?;
        tx.commit()
            .await
            .map_err(|e| db_error("verification du domaine", e))?;
        tracing::warn!(%host, "DNS plus correct : emission arretee ({why})");
        super::push_routes(state).await;
        return Ok(());
    }
    failed_in(state, &mut tx, job, id, host, now, failures, &clean(why)).await?;
    tx.commit()
        .await
        .map_err(|e| db_error("erreur du certificat", e))?;
    Ok(())
}

/// Verdict positif : le compteur de verdicts negatifs repart de zero.
async fn reset_negatives(state: &AppState, id: Uuid) -> Result<(), HandlerError> {
    sqlx::query("UPDATE domains SET dns_failures = 0 WHERE id = $1 AND dns_failures <> 0")
        .bind(id)
        .execute(&state.db)
        .await
        .map(|_| ())
        .map_err(|e| db_error("verdicts DNS du domaine", e))
}

/// Le DNS ne pointe plus vers le noeud : erreur DNS visible, route retiree,
/// aucune reprise (un certificat encore valide reste affiche actif).
async fn dns_moved(
    conn: &mut PgConnection,
    id: Uuid,
    now: i64,
    why: &str,
) -> Result<(), HandlerError> {
    sqlx::query(
        "UPDATE domains SET dns_status = 'error', dns_error = $2, dns_checked_at = now(), \
         cert_status = CASE WHEN cert_expires_at > to_timestamp($3::float8) \
           THEN 'active' ELSE 'none' END, \
         cert_error = NULL, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(clean(why))
    .bind(now as f64)
    .execute(conn)
    .await
    .map(|_| ())
    .map_err(|e| db_error("verification du domaine", e))
}

/// Notification d'un echec : expiration a moins de 7 jours (une fois par
/// certificat), sinon premier echec de la chaine (emission ou
/// renouvellement) ; rien pour les reprises.
fn failure_notice(
    id: Uuid,
    job: &Job,
    now: i64,
    failures: u32,
    valid_until: Option<i64>,
) -> Option<Event> {
    match valid_until {
        Some(expires_at) if expires_at.saturating_sub(now) <= notifications::EXPIRING_SOON_SECS => {
            Some(Event::CertExpiring {
                domain_id: id,
                expires_at,
            })
        }
        _ if failures == 0 => Some(Event::CertFailed {
            domain_id: id,
            job_id: job.id.0,
            renewal: valid_until.is_some(),
        }),
        _ => None,
    }
}

/// Note l'echec (le certificat encore valide reste actif), met en file sa
/// notification ([`failure_notice`]) et replanifie avec un delai croissant
/// borne.
async fn failed(
    state: &AppState,
    job: &Job,
    id: Uuid,
    host: &str,
    now: i64,
    failures: u32,
    reason: &str,
) -> Result<(), HandlerError> {
    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| db_error("connexion a la base", e))?;
    failed_in(state, &mut tx, job, id, host, now, failures, reason).await?;
    tx.commit()
        .await
        .map_err(|e| db_error("erreur du certificat", e))
}

/// [`failed`] dans la transaction de l'appelant (qui la valide).
#[allow(clippy::too_many_arguments)]
async fn failed_in(
    state: &AppState,
    tx: &mut PgConnection,
    job: &Job,
    id: Uuid,
    host: &str,
    now: i64,
    failures: u32,
    reason: &str,
) -> Result<(), HandlerError> {
    // Fin de validite du certificat encore valide (renouvellement en echec).
    let valid_until: Option<Option<i64>> = sqlx::query_scalar(
        "UPDATE domains SET cert_error = $2, \
         cert_status = CASE WHEN cert_expires_at > to_timestamp($3::float8) THEN 'active' ELSE 'error' END, \
         updated_at = now() WHERE id = $1 \
         RETURNING CASE WHEN cert_expires_at > to_timestamp($3::float8) \
           THEN extract(epoch FROM cert_expires_at)::bigint END",
    )
    .bind(id)
    .bind(reason)
    .bind(now as f64)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| db_error("erreur du certificat", e))?;
    if let Some(event) = failure_notice(id, job, now, failures, valid_until.flatten()) {
        notifications::enqueue(&mut *tx, state, event)
            .await
            .map_err(|e| db_error("notification du certificat", e))?;
    }
    let next = now.saturating_add(retry_delay(failures));
    schedule_next(&mut *tx, job, id, next, failures.saturating_add(1)).await?;
    tracing::warn!(%host, next, "emission du certificat en echec : {reason}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renewal_is_30_days_before_expiry_or_halfway() {
        let now = 1_000_000;
        assert_eq!(renew_at(now, now + 90 * 86_400), now + 60 * 86_400);
        assert_eq!(renew_at(now, now + 6 * 86_400), now + 3 * 86_400);
        assert_eq!(renew_at(now, now + 60), now + 3600);
    }

    #[test]
    fn acme_retries_back_off_from_5_minutes_to_one_day() {
        assert_eq!(retry_delay(0), 300);
        assert_eq!(retry_delay(1), 600);
        assert_eq!(retry_delay(4), 4800);
        assert_eq!(retry_delay(9), 86_400);
        assert_eq!(retry_delay(u32::MAX), 86_400);
    }

    #[test]
    fn a_certificate_valid_for_more_than_30_days_is_kept() {
        let now = 1_000_000;
        assert!(still_valid(Some(now + 31 * 86_400), now));
        assert!(!still_valid(Some(now + 30 * 86_400), now));
        assert!(!still_valid(None, now));
    }

    #[test]
    fn deadline_arithmetic_saturates_at_the_limits() {
        // Une echeance annoncee par l'agent ne doit jamais faire deborder.
        assert!(still_valid(Some(i64::MAX), i64::MIN));
        assert!(!still_valid(Some(i64::MIN), i64::MAX));
        assert!(!still_valid(Some(i64::MIN), 0));
        assert_eq!(renew_at(i64::MAX, i64::MAX), i64::MAX);
        assert_eq!(renew_at(i64::MAX, i64::MIN), i64::MAX);
        assert_eq!(renew_at(i64::MIN, i64::MIN), i64::MIN + 3600);
        assert_eq!(renew_at(i64::MIN, i64::MAX), i64::MAX - RENEW_BEFORE_SECS);
        assert_eq!(renew_at(0, 0), 3600);
        assert_eq!(i64::MAX.saturating_add(retry_delay(u32::MAX)), i64::MAX);
    }
}
