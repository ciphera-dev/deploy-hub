use std::net::SocketAddr;
use std::time::Duration;

use api_types::health::HealthResponse;
use axum::Json;
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::StatusCode;
use axum::http::request::Parts;

use crate::auth::CurrentUser;
use crate::http::client_ip::ProxyListener;
use crate::state::AppState;

/// Delai de la verification de la base.
const DB_TIMEOUT: Duration = Duration::from_secs(3);

/// Le pair est-il en boucle locale, arrive par une ecoute que le proxy ne
/// relaie pas ? Sur l'ecoute locale du proxy, le pair est toujours le proxy :
/// il ne dit rien du client reel.
fn local_peer(parts: &Parts) -> bool {
    let via_proxy = parts.extensions.get::<ProxyListener>().is_some();
    !via_proxy
        && parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .is_some_and(|ConnectInfo(addr)| addr.ip().to_canonical().is_loopback())
}

/// `GET /api/health` : le panneau repond et joint sa base (sans
/// authentification). 503 `database_unavailable` sinon : l'updater ne
/// declare reussie qu'une version qui sert et joint sa base. La version n'est
/// donnee qu'a un pair local de l'acces direct ou a une session ouverte.
pub async fn health(
    State(state): State<AppState>,
    mut parts: Parts,
) -> (StatusCode, Json<HealthResponse>) {
    let db_ok = tokio::time::timeout(DB_TIMEOUT, sqlx::query("SELECT 1").execute(&state.db))
        .await
        .is_ok_and(|r| r.is_ok());
    let (code, status) = if db_ok {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "database_unavailable")
    };
    let show_version = local_peer(&parts)
        || (db_ok
            && CurrentUser::from_request_parts(&mut parts, &state)
                .await
                .is_ok());
    (
        code,
        Json(HealthResponse {
            status: status.to_string(),
            version: show_version.then(|| env!("CARGO_PKG_VERSION").to_string()),
        }),
    )
}
