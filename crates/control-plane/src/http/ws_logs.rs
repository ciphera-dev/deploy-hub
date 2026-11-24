//! Logs en direct : `GET /api/ws/logs?deployment=<id>` ou
//! `?service=<id>[&tail=500]` (WebSocket), et
//! `GET /api/deployments/{id}/logs.txt` (telechargement).
//!
//! Avant toute mise a niveau : origine identique a celle du panneau
//! (`Origin` obligatoire : un site tiers ne peut pas ouvrir le flux avec le
//! cookie de l'utilisateur), session, droit de lecture des logs
//! (`Require<res::Logs, lvl::Read>`), cible existante dans un projet. Chaque
//! message est `{ ts, level, container, message }`, masque des valeurs
//! secretes du projet. Le client n'envoie rien (sauf la fermeture).
//!
//! Un flux ouvert reverifie ses droits (session, compte, role, liste d'IP)
//! a chaque changement signale par [`AccessChanges`] et toutes les
//! [`REVALIDATE`] : un acces retire ferme le flux (code 1008).

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::rejection::QueryRejection;
use axum::extract::ws::{
    CloseFrame, Message, WebSocket, WebSocketUpgrade, close_code,
    rejection::WebSocketUpgradeRejection,
};
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use proto::LogTarget;
use serde::Deserialize;
use shared::error::ApiError;
use shared::ids::{DeploymentId, ServerId, ServiceId};
use tokio::sync::watch;
use uuid::Uuid;

use crate::auth::internal;
use crate::auth::webauthn::request_host;
use crate::authz::{Level, Require, Resource, Role, allowed, lvl, res};
use crate::domain::logs::hub::{MAX_STREAMS, MAX_TAIL, Source, Subscription};
use crate::domain::logs::{BUILD_CONTAINER, LogEvent, store};
use crate::http::client_ip::ClientIp;
use crate::state::AppState;

/// Lignes passees envoyees a l'ouverture, par defaut.
pub const DEFAULT_TAIL: u32 = 500;
/// Delai d'envoi d'un message a un client avant de fermer son flux.
const SEND_TIMEOUT: Duration = Duration::from_secs(30);
/// Taille maximale d'un message du client (il n'envoie rien d'utile).
const MAX_CLIENT_MESSAGE: usize = 1024;
/// Reverification periodique des droits d'un flux ouvert (expiration de
/// session, changement non signale).
pub const REVALIDATE: Duration = Duration::from_secs(15);

/// Signal « des droits d'acces ont change » (sessions ou comptes revoques,
/// liste d'IP modifiee) : chaque flux ouvert reverifie les siens aussitot.
#[derive(Clone)]
pub struct AccessChanges(Arc<watch::Sender<u64>>);

impl Default for AccessChanges {
    fn default() -> Self {
        Self(Arc::new(watch::channel(0).0))
    }
}

impl AccessChanges {
    /// A appeler apres la validation de la transaction qui retire un acces.
    pub fn notify(&self) {
        self.0.send_modify(|v| *v = v.wrapping_add(1));
    }

    fn subscribe(&self) -> watch::Receiver<u64> {
        self.0.subscribe()
    }
}

/// Qui a ouvert le flux, pour le reverifier.
struct Access {
    session: Uuid,
    ip: IpAddr,
}

/// La session existe encore (complete, non expiree), son compte garde le
/// droit de lire les logs et son adresse reste autorisee. Erreur de lecture :
/// refus (le client se reconnecte).
async fn still_allowed(state: &AppState, access: &Access) -> bool {
    let role: Result<Option<String>, _> = sqlx::query_scalar(
        "SELECT u.role FROM sessions s JOIN users u ON u.id = s.user_id          WHERE s.id = $1 AND s.expires_at > now()            AND NOT s.mfa_pending AND NOT s.enrollment_required",
    )
    .bind(access.session)
    .fetch_optional(&state.db)
    .await;
    let Ok(Some(role)) = role else {
        return false;
    };
    Role::parse(&role).is_some_and(|r| allowed(r, Resource::Logs, Level::Read))
        && state.ip_allowlist.allows(state, access.ip).await
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/ws/logs", get(ws))
        .route("/api/deployments/{id}/logs.txt", get(download))
}

fn invalid(message: &str) -> ApiError {
    ApiError::new(422, "validation_failed", message)
}

fn not_found() -> ApiError {
    ApiError::new(404, "not_found", "Journal introuvable.")
}

/// Secrets du projet illisibles : aucune ligne n'est servie sans masquage.
fn masking_unavailable() -> ApiError {
    ApiError::new(
        503,
        "masking_unavailable",
        "Masquage des secrets indisponible : journal retenu, réessayez dans un instant.",
    )
}

/// Origine du panneau : `CIPHERA_PUBLIC_URL` (ou le domaine confirme) si
/// elle est definie, sinon `https://` + l'hote de la requete (nom et port
/// seulement).
fn panel_origin(state: &AppState, parts: &Parts) -> Option<String> {
    if let Some(public) = state.public_url() {
        return Some(public);
    }
    let host = request_host(&parts.headers, &parts.uri)?;
    let valid = !host.is_empty()
        && host.len() <= 260
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-.:[]".contains(&b));
    valid.then(|| format!("https://{host}"))
}

/// En-tete `Origin` identique a l'origine du panneau, sinon 403.
pub struct SameOrigin;

impl FromRequestParts<AppState> for SameOrigin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let given = parts
            .headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());
        // Le domaine du panneau (avant comme apres sa confirmation) est
        // aussi une origine du panneau.
        let allowed = [panel_origin(state, parts), state.panel.origin()];
        match given {
            Some(given)
                if allowed
                    .iter()
                    .flatten()
                    .any(|o| given.eq_ignore_ascii_case(o)) =>
            {
                Ok(SameOrigin)
            }
            _ => Err(ApiError::new(403, "origin", "Origine refusée.")),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WsQuery {
    deployment: Option<String>,
    service: Option<String>,
    tail: Option<u32>,
}

/// Flux pret a ouvrir : lecteur inscrit et lignes passees.
struct Opened {
    subscription: Subscription,
    history: Vec<LogEvent>,
}

async fn open_deployment(state: &AppState, id: &str, tail: u32) -> Result<Opened, ApiError> {
    let id = Uuid::parse_str(id).map_err(|_| not_found())?;
    let row: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
        "SELECT p.id, d.server_id FROM deployments d JOIN projects p ON p.id = d.project_id \
         WHERE d.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture du deploiement", e))?;
    let (project, server) = row.ok_or_else(not_found)?;
    // Les lignes d'un build arrivent sans demande : rien a suivre chez
    // l'agent. Inscrit avant la lecture de l'historique (aucune ligne
    // manquee ; les doublons sont ecartes a l'envoi).
    let subscription = state.logs.subscribe(
        Source {
            target: LogTarget::Deployment(DeploymentId(id)),
            project,
            container: BUILD_CONTAINER.to_string(),
            follow: None,
            origin: server.map(ServerId),
        },
        0,
    );
    let redactor = state
        .logs
        .project_redactor(project)
        .await
        .map_err(|_| masking_unavailable())?;
    let history = store::tail(&state.db, id, i64::from(tail))
        .await
        .map_err(|e| internal("lecture du journal", e))?
        .into_iter()
        .map(|row| row.event(&redactor))
        .collect();
    Ok(Opened {
        subscription,
        history,
    })
}

async fn open_service(state: &AppState, id: &str, tail: u32) -> Result<Opened, ApiError> {
    let id = Uuid::parse_str(id).map_err(|_| not_found())?;
    // Serveur du deploiement actuel de l'environnement (dernier reussi).
    let row: Option<(Uuid, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT p.id, s.name, \
                (SELECT d.server_id FROM deployments d \
                 WHERE d.environment_id = s.environment_id AND d.status = 'success' \
                 ORDER BY d.finished_at DESC, d.number DESC LIMIT 1) \
         FROM services s \
         JOIN environments e ON e.id = s.environment_id \
         JOIN projects p ON p.id = e.project_id \
         WHERE s.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture du service", e))?;
    let (project, name, server) = row.ok_or_else(not_found)?;
    let subscription = state.logs.subscribe(
        Source {
            target: LogTarget::Service(ServiceId(id)),
            project,
            container: name,
            follow: server.map(|s| (ServerId(s), tail)),
            origin: server.map(ServerId),
        },
        tail as usize,
    );
    Ok(Opened {
        subscription,
        history: Vec::new(),
    })
}

/// `GET /api/ws/logs`
async fn ws(
    State(state): State<AppState>,
    _origin: SameOrigin,
    ClientIp(ip): ClientIp,
    auth: Require<res::Logs, lvl::Read>,
    query: Result<Query<WsQuery>, QueryRejection>,
    upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, ApiError> {
    let Query(q) = query.map_err(|_| invalid("Requête invalide."))?;
    let tail = q.tail.unwrap_or(DEFAULT_TAIL);
    if tail > MAX_TAIL {
        return Err(invalid("Nombre de lignes invalide (0 à 1000)."));
    }
    let upgrade = upgrade
        .map_err(|_| ApiError::new(400, "websocket_required", "Connexion WebSocket attendue."))?;
    if state.logs.streams() >= MAX_STREAMS {
        return Err(ApiError::new(
            503,
            "too_many_streams",
            "Trop de flux de logs ouverts. Réessayez plus tard.",
        ));
    }
    let opened = match (q.deployment.as_deref(), q.service.as_deref()) {
        (Some(id), None) => open_deployment(&state, id, tail).await?,
        (None, Some(id)) => open_service(&state, id, tail).await?,
        _ => return Err(invalid("Indiquez un déploiement ou un service.")),
    };
    Ok(upgrade
        .max_message_size(MAX_CLIENT_MESSAGE)
        .on_upgrade(move |socket| {
            let access = Access {
                session: auth.user.session.id,
                ip,
            };
            stream(socket, opened, state, access)
        })
        .into_response())
}

async fn send(socket: &mut WebSocket, event: &LogEvent) -> bool {
    let Ok(json) = serde_json::to_string(event) else {
        return false;
    };
    matches!(
        tokio::time::timeout(SEND_TIMEOUT, socket.send(Message::Text(json.into()))).await,
        Ok(Ok(()))
    )
}

async fn stream(mut socket: WebSocket, opened: Opened, state: AppState, access: Access) {
    let Opened {
        mut subscription,
        history,
    } = opened;
    let mut changes = state.access.subscribe();
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + REVALIDATE, REVALIDATE);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Lignes de l'historique deja recues en direct : ecartees une fois.
    let last = history.last().map(|e| e.ts_ms);
    let mut seen: HashSet<(i64, String)> = history
        .iter()
        .map(|e| (e.ts_ms, e.message.clone()))
        .collect();
    for event in &history {
        if !send(&mut socket, event).await {
            return;
        }
    }
    loop {
        tokio::select! {
            event = subscription.next() => {
                let Some(event) = event else { break };
                if let Some(last) = last
                    && !seen.is_empty()
                {
                    if event.ts_ms > last {
                        seen.clear();
                    } else if seen.remove(&(event.ts_ms, event.message.clone())) {
                        continue;
                    }
                }
                if !send(&mut socket, &event).await {
                    break;
                }
            }
            msg = socket.recv() => match msg {
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
            changed = changes.changed() => {
                if changed.is_err() || !still_allowed(&state, &access).await {
                    revoked(&mut socket).await;
                    break;
                }
            }
            _ = tick.tick() => {
                if !still_allowed(&state, &access).await {
                    revoked(&mut socket).await;
                    break;
                }
            }
        }
    }
}

/// Fermeture d'un flux dont l'acces a ete retire.
async fn revoked(socket: &mut WebSocket) {
    let frame = CloseFrame {
        code: close_code::POLICY,
        reason: "Accès révoqué".into(),
    };
    let _ = tokio::time::timeout(SEND_TIMEOUT, socket.send(Message::Close(Some(frame)))).await;
}

/// `GET /api/deployments/{id}/logs.txt`
async fn download(
    State(state): State<AppState>,
    _auth: Require<res::Logs, lvl::Read>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = Uuid::parse_str(&id).map_err(|_| not_found())?;
    // Telechargements simultanes bornes (chacun lit jusqu'a 4 Mio).
    let Some(_slot) = state.logs.try_download() else {
        let mut res = ApiError::new(
            429,
            "rate_limited",
            "Trop de téléchargements de journaux en cours : réessayez dans un instant.",
        )
        .into_response();
        res.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("2"));
        return Ok(res);
    };
    let row: Option<(Uuid, i64)> = sqlx::query_as(
        "SELECT p.id, d.number FROM deployments d JOIN projects p ON p.id = d.project_id \
         WHERE d.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| internal("lecture du deploiement", e))?;
    let (project, number) = row.ok_or_else(not_found)?;
    let redactor = state
        .logs
        .project_redactor(project)
        .await
        .map_err(|_| masking_unavailable())?;
    let body = store::download(&state.db, id, &redactor)
        .await
        .map_err(|e| internal("lecture du journal", e))?;
    let disposition =
        HeaderValue::from_str(&format!("attachment; filename=\"deployment-{number}.txt\""))
            .map_err(|e| internal("en-tete du telechargement", e))?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (header::CONTENT_DISPOSITION, disposition),
            (
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        body,
    )
        .into_response())
}
