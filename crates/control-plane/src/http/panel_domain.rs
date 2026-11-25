//! Domaine du panneau (proprietaire seulement) :
//! - `GET /api/settings/panel-domain` : domaine et etat ;
//! - `PUT /api/settings/panel-domain { domain }` : DNS verifie (rien n'est
//!   enregistre s'il est faux), certificat demande par la tache `acme_issue`
//!   des domaines, route `panel` du proxy local (443 -> ecoute locale) ;
//! - `POST /api/settings/panel-domain/confirm` : depuis le nouveau domaine
//!   seulement (requete servie par le proxy du noeud), ferme l'acces 8443.
//!   Avec une GitHub App, son webhook est d'abord deplace vers le nouveau
//!   domaine par l'API GitHub, et l'URL de configuration (non modifiable par
//!   l'API) doit avoir ete changee a la main (`github_setup_url_updated`) :
//!   sans quoi les deploiements s'arreteraient avec l'acces 8443 ;
//! - `POST /api/settings/panel-domain/recheck` : DNS reverifie, tache du
//!   certificat replanifiee tout de suite (reprise apres une panne) ;
//! - `DELETE /api/settings/panel-domain` : annulation ou retrait (route et
//!   certificat retires, acces 8443 rouvert). Domaine confirme avec une
//!   GitHub App : son webhook revient d'abord sur l'acces direct (8443).
//!
//! Ecritures sous CSRF (couche du routeur) et auditees.

use std::net::SocketAddr;

use api_types::panel_domain::{ConfirmPanelDomain, PanelDomain, PanelDomainStatus, SetPanelDomain};
use axum::body::Bytes;
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use shared::error::ApiError;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::auth::webauthn::request_host;
use crate::auth::{internal, rate_limit};
use crate::connectors::github;
use crate::domain::domains::{self, acme, dns, panel};
use crate::http::client_ip::ProxyListener;
use crate::http::settings::{Admin, owner_only};
use crate::secrets::labels;
use crate::state::AppState;

const MAX_BODY: usize = 16 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/settings/panel-domain",
            get(show).put(set).delete(remove),
        )
        .route("/api/settings/panel-domain/confirm", post(confirm))
        .route("/api/settings/panel-domain/recheck", post(recheck))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

/// Le domaine du panneau change l'acces a toute l'instance : proprietaire
/// seulement ([`owner_only`]).
type Owner = Admin;

fn db(context: &'static str) -> impl Fn(sqlx::Error) -> ApiError {
    move |e| internal(context, e)
}

fn unprocessable(message: impl Into<String>) -> ApiError {
    ApiError::new(422, "validation_failed", message)
}

/// Corps JSON refuse : 413 au-dela de la limite du routeur (et non 422),
/// comme `confirm`.
fn invalid_body(rejection: JsonRejection) -> ApiError {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        ApiError::new(
            413,
            "payload_too_large",
            "Corps de requête trop volumineux.",
        )
    } else {
        unprocessable("Requête invalide.")
    }
}

async fn view(conn: &mut PgConnection) -> Result<PanelDomain, ApiError> {
    panel::view(conn).await.map_err(db("domaine du panneau"))
}

/// Abandonne une bascule echue (voir [`panel::expire`]) ; renvoie vrai si la
/// table des routes doit etre renvoyee.
async fn expire(state: &AppState, conn: &mut PgConnection) -> Result<bool, ApiError> {
    panel::expire(conn, state.clock.unix_secs())
        .await
        .map(|host| host.is_some())
        .map_err(db("echeance du domaine du panneau"))
}

/// Serialise les ecritures du domaine du panneau.
async fn lock(conn: &mut PgConnection) -> Result<(), ApiError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('ciphera.panel_domain'))")
        .execute(conn)
        .await
        .map(|_| ())
        .map_err(db("verrou du domaine du panneau"))
}

/// Valide la transaction **et** l'etat en memoire sous le meme verrou
/// d'ecriture : l'etat est pose avant le commit (le verrou consultatif ne
/// tombe qu'au commit, donc aucune autre ecriture ne s'intercale), et remis
/// tel qu'il etait si le commit echoue. La sequence est detachee de la
/// requete (voir [`panel::PanelSwitch::commit_with`]) : une deconnexion du
/// client pendant le commit ne laisse pas l'etat en memoire faux.
async fn commit_with(
    state: &AppState,
    tx: sqlx::Transaction<'static, sqlx::Postgres>,
    next: Option<panel::Panel>,
) -> Result<(), ApiError> {
    state
        .panel
        .commit_with(next, async move { tx.commit().await })
        .await
        .map_err(db("transaction"))
}

/// Requete arrivee par le proxy du noeud (ecoute locale, pair en boucle
/// locale, HTTPS) sous l'hote du domaine du panneau.
struct ViaPanelDomain(bool);

impl FromRequestParts<AppState> for ViaPanelDomain {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let loopback = parts.extensions.get::<ProxyListener>().is_some()
            && parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .is_some_and(|ConnectInfo(addr)| addr.ip().to_canonical().is_loopback());
        let https = parts
            .headers
            .get("x-forwarded-proto")
            .is_some_and(|v| v.as_bytes() == b"https");
        let host = request_host(&parts.headers, &parts.uri).map(|h| {
            let h = h.rsplit_once(':').map_or(h, |(name, port)| {
                if port.bytes().all(|b| b.is_ascii_digit()) {
                    name
                } else {
                    h
                }
            });
            h.trim_end_matches('.').to_ascii_lowercase()
        });
        let panel = state.panel.current().map(|p| p.host);
        Ok(ViaPanelDomain(
            loopback && https && host.is_some() && host == panel,
        ))
    }
}

/// `GET /api/settings/panel-domain`
async fn show(State(state): State<AppState>, auth: Owner) -> Result<Response, ApiError> {
    owner_only(&auth)?;
    let mut conn = state
        .db
        .acquire()
        .await
        .map_err(db("connexion a la base"))?;
    let expired = expire(&state, &mut conn).await?;
    let body = view(&mut conn).await?;
    drop(conn);
    if expired {
        domains::push_routes(&state).await;
    }
    Ok(Json(body).into_response())
}

/// `PUT /api/settings/panel-domain`
async fn set(
    State(state): State<AppState>,
    auth: Owner,
    body: Result<Json<SetPanelDomain>, JsonRejection>,
) -> Result<Response, ApiError> {
    let actor = owner_only(&auth)?;
    let Json(input) = body.map_err(invalid_body)?;
    let host = domains::normalize_host(&input.domain).map_err(unprocessable)?;
    if domains::in_wildcard_zone(&host) {
        return Err(unprocessable(
            "Les noms sslip.io et nip.io ne peuvent pas servir de domaine du panneau : utilisez un domaine que vous possédez.",
        ));
    }
    let (v4, v6) = state.cfg.node_ips();
    if v4.is_none() && v6.is_none() {
        return Err(ApiError::new(
            422,
            "node_ip_unknown",
            "IP publique du nœud inconnue : définissez CIPHERA_PUBLIC_IP.",
        ));
    }
    if state.panel.current().is_some_and(|p| p.confirmed) {
        return Err(active_conflict());
    }
    // Resolution hors transaction (jusqu'a 5 s) ; DNS faux : rien n'est
    // enregistre.
    let error = match state.dns.resolve(&host).await {
        Ok(ips) => dns::verdict(&host, v4, v6, &ips),
        Err(e) => {
            tracing::warn!(%host, "resolution DNS impossible : {e}");
            Some(format!(
                "Résolution DNS de {host} impossible pour le moment : réessayez."
            ))
        }
    };
    if let Some(message) = error {
        return Err(ApiError::new(422, "dns_error", message));
    }

    let mut tx = state.db.begin().await.map_err(db("transaction"))?;
    lock(&mut tx).await?;
    let existing: Option<(Uuid, String, bool)> =
        sqlx::query_as("SELECT id, host, panel_confirmed_at IS NOT NULL FROM domains WHERE panel")
            .fetch_optional(&mut *tx)
            .await
            .map_err(db("domaine du panneau"))?;
    let mut replaced = None;
    let id = match existing {
        Some((_, _, true)) => return Err(active_conflict()),
        Some((id, current, false)) if current == host => {
            domains::record_dns(&mut tx, id, None)
                .await
                .map_err(db("verification du domaine"))?;
            id
        }
        other => {
            if let Some((_, current, _)) = other {
                panel::remove(&mut tx)
                    .await
                    .map_err(db("domaine du panneau"))?;
                replaced = Some(current);
            }
            insert(&mut tx, &host, actor).await?
        }
    };
    // Chaque enregistrement ouvre une nouvelle fenetre de bascule.
    sqlx::query("UPDATE domains SET panel_expires_at = to_timestamp($2::float8) WHERE id = $1")
        .bind(id)
        .bind(state.clock.unix_secs() + panel::SWITCH_TTL_SECS)
        .execute(&mut *tx)
        .await
        .map_err(db("echeance du domaine du panneau"))?;
    let requested = acme::request(&mut tx, id, state.clock.unix_secs() as i64)
        .await
        .map_err(db("demande de certificat"))?;
    if !requested {
        acme::check(&mut tx, id)
            .await
            .map_err(db("verification du certificat"))?;
    }
    panel::audit(
        &mut tx,
        Some(actor),
        "panel_domain.set",
        json!({ "domain": &host, "replaced": &replaced }),
    )
    .await
    .map_err(db("audit du domaine du panneau"))?;
    let body = view(&mut tx).await?;
    commit_with(
        &state,
        tx,
        Some(panel::Panel {
            id,
            host,
            confirmed: false,
        }),
    )
    .await?;
    domains::push_routes(&state).await;
    if let Some(old) = replaced {
        domains::remove_certificates(&state, &[old]);
    }
    Ok((StatusCode::ACCEPTED, Json(body)).into_response())
}

fn active_conflict() -> ApiError {
    ApiError::new(
        409,
        "panel_domain_active",
        "Un domaine du panneau est déjà actif : retirez-le d'abord.",
    )
}

/// Insere le domaine du panneau (DNS verifie). Hote deja pris : 409.
async fn insert(conn: &mut PgConnection, host: &str, actor: Uuid) -> Result<Uuid, ApiError> {
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO domains (id, host, path_prefix, panel, dns_status, dns_checked_at, created_by) \
         VALUES ($1, $2, '/', true, 'ok', now(), $3) ON CONFLICT DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(host)
    .bind(actor)
    .fetch_optional(conn)
    .await
    .map_err(db("domaine du panneau"))?;
    id.ok_or_else(|| {
        ApiError::new(
            409,
            "host_taken",
            "Ce domaine est déjà utilisé par un projet de cette instance.",
        )
    })
}

/// `POST /api/settings/panel-domain/confirm`
async fn confirm(
    State(state): State<AppState>,
    auth: Owner,
    via: ViaPanelDomain,
    body: Bytes,
) -> Result<Response, ApiError> {
    let actor = owner_only(&auth)?;
    // Corps facultatif : vide ou `null` valent les reglages par defaut.
    let input: ConfirmPanelDomain = if body.is_empty() {
        ConfirmPanelDomain::default()
    } else {
        serde_json::from_slice::<Option<ConfirmPanelDomain>>(&body)
            .map_err(|_| unprocessable("Requête invalide."))?
            .unwrap_or_default()
    };
    let mut tx = state.db.begin().await.map_err(db("transaction"))?;
    lock(&mut tx).await?;
    if expire(&state, &mut tx).await? {
        tx.commit().await.map_err(db("transaction"))?;
        domains::push_routes(&state).await;
        return Err(not_ready());
    }
    let current = view(&mut tx).await?;
    let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM domains WHERE panel")
        .fetch_optional(&mut *tx)
        .await
        .map_err(db("domaine du panneau"))?;
    let (Some(host), Some(id)) = (current.domain.clone(), id) else {
        return Err(ApiError::new(404, "not_found", "Aucun domaine du panneau."));
    };
    if current.status == PanelDomainStatus::Active {
        return Ok(Json(current).into_response());
    }
    if !via.0 {
        return Err(ApiError::new(
            409,
            "confirm_from_domain",
            format!(
                "Ouvrez le panneau sur https://{host} pour confirmer : l'accès actuel reste ouvert jusque-là."
            ),
        ));
    }
    if current.status != PanelDomainStatus::AwaitingConfirmation {
        return Err(not_ready());
    }
    move_github_app(&state, &host, input.github_setup_url_updated).await?;
    sqlx::query("UPDATE domains SET panel_confirmed_at = now(), updated_at = now() WHERE panel")
        .execute(&mut *tx)
        .await
        .map_err(db("confirmation du domaine du panneau"))?;
    panel::audit(
        &mut tx,
        Some(actor),
        "panel_domain.confirm",
        json!({ "domain": host }),
    )
    .await
    .map_err(db("audit du domaine du panneau"))?;
    let body = view(&mut tx).await?;
    commit_with(
        &state,
        tx,
        Some(panel::Panel {
            id,
            host: host.clone(),
            confirmed: true,
        }),
    )
    .await?;
    tracing::info!(%host, "domaine du panneau confirme : acces direct ferme");
    Ok(Json(body).into_response())
}

fn not_ready() -> ApiError {
    ApiError::new(409, "not_ready", "Certificat du domaine pas encore actif.")
}

/// GitHub App connectee : son webhook passe sur `https://<host>` par l'API
/// (JWT de l'App, `PATCH /app/hook/config`). L'URL de configuration (Setup
/// URL) ne se change pas par l'API : sans l'accord explicite qu'elle a ete
/// changee a la main, la confirmation est refusee avec la consigne. Un echec
/// de GitHub refuse aussi la confirmation (l'acces 8443 reste ouvert).
async fn move_github_app(
    state: &AppState,
    host: &str,
    setup_url_updated: bool,
) -> Result<(), ApiError> {
    let Some(app) = github::load(&state.db)
        .await
        .map_err(db("lecture de la GitHub App"))?
    else {
        return Ok(());
    };
    if !setup_url_updated {
        return Err(ApiError::new(
            409,
            "github_setup_url",
            format!(
                "Avant de confirmer : sur GitHub, dans les réglages de l'App « {} » (Settings → Developer settings → GitHub Apps), remplacez l'URL de configuration (Setup URL) par https://{host}/connectors, puis confirmez à nouveau avec « github_setup_url_updated »: true. Le webhook sera déplacé automatiquement.",
                app.name
            ),
        ));
    }
    let pem = state
        .secrets
        .open_bound(labels::GITHUB_PRIVATE_KEY, &app.private_key_sealed)
        .map_err(|e| internal("ouverture de la cle de la GitHub App", e))?;
    let url = format!("https://{host}/v1/webhooks/github");
    state
        .github
        .set_webhook_url(
            &state.cfg.github_api_url,
            app.app_id,
            &pem,
            state.clock.unix_secs() as i64,
            &url,
        )
        .await
        .map_err(|e| {
            tracing::warn!("webhook de la GitHub App non deplace : {e:?}");
            ApiError::new(
                502,
                "github_hook_not_updated",
                "Le webhook de la GitHub App n'a pas pu être déplacé vers le nouveau domaine : réessayez. L'accès actuel reste ouvert.",
            )
        })?;
    tracing::info!(%host, "webhook de la GitHub App deplace vers le domaine du panneau");
    Ok(())
}

/// Domaine confirme retire : le webhook de la GitHub App revient sur
/// l'acces direct (8443), sinon ses livraisons echoueraient sans bruit. Un
/// echec de GitHub refuse le retrait (rien n'est change).
async fn move_github_app_back(state: &AppState) -> Result<(), ApiError> {
    let failed = || {
        ApiError::new(
            502,
            "github_hook_not_updated",
            "Le webhook de la GitHub App n'a pas pu être ramené sur l'accès direct (8443) : réessayez. Le domaine reste actif.",
        )
    };
    let Some(origin) = github::direct_origin(&state.cfg) else {
        if github::load(&state.db)
            .await
            .map_err(db("lecture de la GitHub App"))?
            .is_some()
        {
            tracing::warn!(
                "acces direct inconnu (CIPHERA_PUBLIC_URL, CIPHERA_PUBLIC_IP) : webhook non deplace"
            );
            return Err(failed());
        }
        return Ok(());
    };
    match github::move_webhook(
        &state.db,
        &state.cfg,
        &state.secrets,
        &state.github,
        state.clock.unix_secs() as i64,
        &origin,
    )
    .await
    {
        Ok(Some(url)) => {
            tracing::info!(%url, "webhook de la GitHub App ramene sur l'acces direct");
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(e) => {
            tracing::warn!("webhook de la GitHub App non deplace : {e:?}");
            Err(failed())
        }
    }
}

/// `POST /api/settings/panel-domain/recheck` : DNS reverifie (hors
/// transaction) ; correct, la verification et l'emission sont replanifiees
/// tout de suite (compteur de verdicts negatifs remis a zero) ; faux, 422 et
/// rien n'est modifie.
async fn recheck(State(state): State<AppState>, auth: Owner) -> Result<Response, ApiError> {
    let actor = owner_only(&auth)?;
    let Some(panel) = state.panel.current() else {
        return Err(not_found());
    };
    // Resolution DNS et verrou : au plus 5 revérifications par quart d'heure
    // et par proprietaire (espace separe des connexions).
    if let rate_limit::Admission::Limited(retry_after) = rate_limit::admit(
        &state.db,
        &rate_limit::IpKey::user_action("panel-recheck", actor),
        &rate_limit::email_key("panel-recheck"),
    )
    .await
    .map_err(db("limitation de debit"))?
    {
        let mut res = ApiError::new(
            429,
            "rate_limited",
            "Trop de revérifications. Réessayez dans quelques minutes.",
        )
        .into_response();
        res.headers_mut()
            .insert(axum::http::header::RETRY_AFTER, retry_after.into());
        return Ok(res);
    }
    let (v4, v6) = state.cfg.node_ips();
    if v4.is_none() && v6.is_none() {
        return Err(ApiError::new(
            422,
            "node_ip_unknown",
            "IP publique du nœud inconnue : définissez CIPHERA_PUBLIC_IP.",
        ));
    }
    let host = panel.host;
    let error = match state.dns.resolve(&host).await {
        Ok(ips) => dns::verdict(&host, v4, v6, &ips),
        Err(e) => {
            tracing::warn!(%host, "resolution DNS impossible : {e}");
            Some(format!(
                "Résolution DNS de {host} impossible pour le moment : réessayez."
            ))
        }
    };
    if let Some(message) = error {
        return Err(ApiError::new(422, "dns_error", message));
    }
    let mut tx = state.db.begin().await.map_err(db("transaction"))?;
    lock(&mut tx).await?;
    // Domaine retire ou remplace pendant la resolution : 404.
    let id: Option<Uuid> = sqlx::query_scalar("SELECT id FROM domains WHERE panel AND host = $1")
        .bind(&host)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db("domaine du panneau"))?;
    let Some(id) = id else {
        return Err(not_found());
    };
    domains::record_dns(&mut tx, id, None)
        .await
        .map_err(db("verification du domaine"))?;
    // Une bascule non confirmee ouvre une nouvelle fenetre, comme
    // l'enregistrement du domaine.
    sqlx::query(
        "UPDATE domains SET panel_expires_at = to_timestamp($2::float8)          WHERE id = $1 AND panel_confirmed_at IS NULL",
    )
    .bind(id)
    .bind(state.clock.unix_secs() + panel::SWITCH_TTL_SECS)
    .execute(&mut *tx)
    .await
    .map_err(db("echeance du domaine du panneau"))?;
    acme::recheck(&mut tx, id, state.clock.unix_secs() as i64)
        .await
        .map_err(db("replanification du certificat"))?;
    panel::audit(
        &mut tx,
        Some(actor),
        "panel_domain.recheck",
        json!({ "domain": &host }),
    )
    .await
    .map_err(db("audit du domaine du panneau"))?;
    let body = view(&mut tx).await?;
    tx.commit().await.map_err(db("transaction"))?;
    domains::push_routes(&state).await;
    Ok((StatusCode::ACCEPTED, Json(body)).into_response())
}

fn not_found() -> ApiError {
    ApiError::new(404, "not_found", "Aucun domaine du panneau.")
}

/// `DELETE /api/settings/panel-domain`
async fn remove(State(state): State<AppState>, auth: Owner) -> Result<Response, ApiError> {
    let actor = owner_only(&auth)?;
    let mut tx = state.db.begin().await.map_err(db("transaction"))?;
    lock(&mut tx).await?;
    let confirmed: Option<bool> =
        sqlx::query_scalar("SELECT panel_confirmed_at IS NOT NULL FROM domains WHERE panel")
            .fetch_optional(&mut *tx)
            .await
            .map_err(db("domaine du panneau"))?;
    if confirmed == Some(true) {
        move_github_app_back(&state).await?;
    }
    let removed = panel::remove(&mut tx)
        .await
        .map_err(db("domaine du panneau"))?;
    if let Some(host) = &removed {
        panel::audit(
            &mut tx,
            Some(actor),
            "panel_domain.delete",
            json!({ "domain": host }),
        )
        .await
        .map_err(db("audit du domaine du panneau"))?;
    }
    commit_with(&state, tx, None).await?;
    if let Some(host) = removed {
        domains::push_routes(&state).await;
        domains::remove_certificates(&state, &[host]);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn put(body: Vec<u8>) -> StatusCode {
        async fn handler(body: Result<Json<SetPanelDomain>, JsonRejection>) -> Response {
            match body.map_err(invalid_body) {
                Ok(_) => StatusCode::OK.into_response(),
                Err(e) => e.into_response(),
            }
        }
        let app: Router = Router::new()
            .fallback(handler)
            .layer(DefaultBodyLimit::max(MAX_BODY));
        let req = Request::put("/")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        app.oneshot(req).await.unwrap().status()
    }

    #[tokio::test]
    async fn json_body_over_the_limit_is_413_not_422() {
        let mut big = br#"{"domain":"panel.example.org"}"#.to_vec();
        big.resize(MAX_BODY + 1024, b' ');
        assert_eq!(put(big).await, StatusCode::PAYLOAD_TOO_LARGE);
        let ok = br#"{"domain":"panel.example.org"}"#.to_vec();
        assert_eq!(put(ok).await, StatusCode::OK);
        assert_eq!(put(b"{".to_vec()).await, StatusCode::UNPROCESSABLE_ENTITY);
    }
}
