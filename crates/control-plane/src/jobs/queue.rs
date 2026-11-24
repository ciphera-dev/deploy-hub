//! Requetes de la file. L'heure vient de l'horloge injectable de l'application
//! (`Clock`), jamais de `now()` : les tests peuvent ainsi faire echoir un bail.

use serde_json::Value;
use shared::ids::JobId;
use sqlx::{PgConnection, PgPool};

use super::{
    ACME_CONCURRENCY, Failure, Job, JobKind, LEASE, MAX_ERROR_CHARS, NOTIFY_CHANNEL, Result,
};
use crate::state::Clock;

/// Ajoute une tache. Avec une `dedupe_key` deja portee par une tache non
/// terminale, renvoie l'identifiant de cette derniere (aucune erreur).
pub async fn enqueue(
    db: &PgPool,
    kind: JobKind,
    payload: Value,
    dedupe_key: Option<String>,
) -> Result<JobId> {
    let mut conn = db.acquire().await?;
    enqueue_in(&mut conn, kind, payload, dedupe_key).await
}

/// Comme [`enqueue`], sur une connexion donnee : dans une transaction, la
/// tache (et le reveil des travailleurs) n'existe qu'au `COMMIT`.
pub async fn enqueue_in(
    conn: &mut PgConnection,
    kind: JobKind,
    payload: Value,
    dedupe_key: Option<String>,
) -> Result<JobId> {
    enqueue_with(conn, kind, payload, dedupe_key, Schedule::default()).await
}

/// Echeance et tentatives d'une tache (valeurs par defaut de la table si
/// absentes : tout de suite, 5 tentatives).
#[derive(Clone, Copy, Debug, Default)]
pub struct Schedule {
    /// Premiere execution au plus tot (secondes Unix).
    pub run_at: Option<f64>,
    pub max_attempts: Option<i32>,
}

/// Comme [`enqueue_in`], avec une echeance et un nombre de tentatives.
pub async fn enqueue_with(
    conn: &mut PgConnection,
    kind: JobKind,
    payload: Value,
    dedupe_key: Option<String>,
    schedule: Schedule,
) -> Result<JobId> {
    let payload = payload.to_string();
    // Reboucle si la tache existante se termine entre l'insertion refusee et
    // sa relecture.
    for _ in 0..5 {
        let id = JobId::new();
        let inserted: Option<JobId> = sqlx::query_scalar(
            "INSERT INTO jobs (id, kind, payload, dedupe_key, run_at, max_attempts) \
             VALUES ($1, $2, $3::jsonb, $4, COALESCE(to_timestamp($5::float8), now()), COALESCE($6, 5)) \
             ON CONFLICT (dedupe_key) WHERE dedupe_key IS NOT NULL AND status IN ('queued', 'running') \
             DO NOTHING RETURNING id",
        )
        .bind(id)
        .bind(kind.as_str())
        .bind(&payload)
        .bind(&dedupe_key)
        .bind(schedule.run_at)
        .bind(schedule.max_attempts)
        .fetch_optional(&mut *conn)
        .await?;
        if let Some(id) = inserted {
            // Reveil des travailleurs ; au pire ils interrogent a l'echeance.
            let _ = sqlx::query("SELECT pg_notify($1, $2)")
                .bind(NOTIFY_CHANNEL)
                .bind(kind.as_str())
                .execute(&mut *conn)
                .await;
            return Ok(id);
        }
        if let Some(key) = &dedupe_key {
            let existing: Option<JobId> = sqlx::query_scalar(
                "SELECT id FROM jobs WHERE dedupe_key = $1 AND status IN ('queued', 'running')",
            )
            .bind(key)
            .fetch_optional(&mut *conn)
            .await?;
            if let Some(id) = existing {
                return Ok(id);
            }
        }
    }
    Err(sqlx::Error::RowNotFound)
}

#[derive(sqlx::FromRow)]
struct JobRow {
    id: JobId,
    kind: String,
    payload: String,
    attempts: i32,
    max_attempts: i32,
}

/// Prend la tache la plus urgente (priorite, puis `run_at`) : en file et
/// echue, ou en cours mais au bail echu. Les emissions ACME ont leur propre
/// limite ([`ACME_CONCURRENCY`] en cours sur toute l'instance) : une emission
/// attend jusqu'a 200 s et ne doit pas priver les deploiements de
/// travailleurs. Marque la prise (bail de 5 minutes,
/// tentative comptee) dans la meme instruction, sous `FOR UPDATE SKIP LOCKED`.
pub async fn claim(db: &PgPool, clock: &Clock, worker_id: &str) -> Result<Option<Job>> {
    let now = clock.unix_secs();
    // Bail echu a la derniere tentative : plus de reprise.
    sqlx::query(
        "UPDATE jobs SET status = 'dead', finished_at = to_timestamp($1::float8), \
         locked_by = NULL, lease_until = NULL, \
         last_error = 'bail expire a la derniere tentative' \
         WHERE status = 'running' AND lease_until <= to_timestamp($1::float8) \
         AND attempts >= max_attempts",
    )
    .bind(now)
    .execute(db)
    .await?;

    let row: Option<JobRow> = sqlx::query_as(
        "WITH next AS ( \
             SELECT id FROM jobs \
             WHERE ((status = 'queued' AND run_at <= to_timestamp($1::float8)) \
                OR (status = 'running' AND lease_until <= to_timestamp($1::float8) \
                    AND attempts < max_attempts)) \
               AND (kind <> 'acme_issue' OR (SELECT count(*) FROM jobs r \
                    WHERE r.kind = 'acme_issue' AND r.status = 'running' \
                    AND r.lease_until > to_timestamp($1::float8)) < $4) \
             ORDER BY priority DESC, run_at, created_at \
             LIMIT 1 FOR UPDATE SKIP LOCKED) \
         UPDATE jobs j SET status = 'running', locked_by = $2, \
             lease_until = to_timestamp($1::float8) + make_interval(secs => $3::float8), \
             attempts = j.attempts + 1 \
         FROM next WHERE j.id = next.id \
         RETURNING j.id, j.kind, j.payload::text AS payload, j.attempts, j.max_attempts",
    )
    .bind(now)
    .bind(worker_id)
    .bind(LEASE.as_secs_f64())
    .bind(ACME_CONCURRENCY)
    .fetch_optional(db)
    .await?;
    row.map(|r| {
        let kind = r
            .kind
            .parse()
            .map_err(|e: String| sqlx::Error::Decode(e.into()))?;
        let payload =
            serde_json::from_str(&r.payload).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        Ok(Job {
            id: r.id,
            kind,
            payload,
            attempts: r.attempts,
            max_attempts: r.max_attempts,
        })
    })
    .transpose()
}

/// Prolonge le bail de 5 minutes. `false` si le travailleur n'est plus le
/// detenteur (bail echu ou tache reprise).
pub async fn heartbeat(db: &PgPool, clock: &Clock, id: JobId, worker_id: &str) -> Result<bool> {
    let now = clock.unix_secs();
    let done = sqlx::query(
        "UPDATE jobs SET lease_until = to_timestamp($1::float8) + make_interval(secs => $4::float8) \
         WHERE id = $2 AND status = 'running' AND locked_by = $3 \
         AND lease_until > to_timestamp($1::float8)",
    )
    .bind(now)
    .bind(id)
    .bind(worker_id)
    .bind(LEASE.as_secs_f64())
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Termine la tache avec succes. `false` si le travailleur n'est pas (ou plus)
/// le detenteur du bail.
pub async fn complete(db: &PgPool, clock: &Clock, id: JobId, worker_id: &str) -> Result<bool> {
    let now = clock.unix_secs();
    let done = sqlx::query(
        "UPDATE jobs SET status = 'done', finished_at = to_timestamp($1::float8), \
         locked_by = NULL, lease_until = NULL \
         WHERE id = $2 AND status = 'running' AND locked_by = $3 \
         AND lease_until > to_timestamp($1::float8)",
    )
    .bind(now)
    .bind(id)
    .bind(worker_id)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Signale un echec. `Failure::Retry` remet la tache en file apres le delai
/// tant qu'il reste des tentatives ; sinon, ou avec `Failure::Terminal`, elle
/// passe a `dead`. L'erreur est tronquee et nettoyee des caracteres de
/// controle (elle ne doit contenir aucun secret). `false` si le travailleur
/// n'est pas (ou plus) le detenteur du bail.
pub async fn fail(
    db: &PgPool,
    clock: &Clock,
    id: JobId,
    worker_id: &str,
    error: &str,
    failure: Failure,
) -> Result<bool> {
    let now = clock.unix_secs();
    let retry_secs = match failure {
        Failure::Retry(d) => Some(d.as_secs_f64()),
        Failure::Terminal => None,
    };
    let done = sqlx::query(
        "UPDATE jobs SET \
           status = CASE WHEN $5::float8 IS NOT NULL AND attempts < max_attempts \
                         THEN 'queued' ELSE 'dead' END, \
           run_at = CASE WHEN $5::float8 IS NOT NULL AND attempts < max_attempts \
                         THEN to_timestamp($1::float8) + make_interval(secs => $5::float8) \
                         ELSE run_at END, \
           finished_at = CASE WHEN $5::float8 IS NOT NULL AND attempts < max_attempts \
                              THEN NULL ELSE to_timestamp($1::float8) END, \
           last_error = $4, locked_by = NULL, lease_until = NULL \
         WHERE id = $2 AND status = 'running' AND locked_by = $3 \
         AND lease_until > to_timestamp($1::float8)",
    )
    .bind(now)
    .bind(id)
    .bind(worker_id)
    .bind(sanitize_error(error))
    .bind(retry_secs)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Erreur stockable : sans caractere de controle (NUL refuse par PostgreSQL),
/// tronquee a `MAX_ERROR_CHARS` caracteres.
fn sanitize_error(error: &str) -> String {
    let mut out: String = error
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .take(MAX_ERROR_CHARS)
        .collect();
    if error.chars().count() > MAX_ERROR_CHARS {
        out.push('…');
    }
    out
}
