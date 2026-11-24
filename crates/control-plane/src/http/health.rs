use std::time::Duration;

use api_types::health::HealthResponse;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::state::AppState;

/// Delai de la verification de la base.
const DB_TIMEOUT: Duration = Duration::from_secs(3);

/// `GET /api/health` : le panneau repond et joint sa base (sans
/// authentification). 503 `database_unavailable` sinon : l'updater ne
/// declare reussie qu'une version qui sert et joint sa base.
pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let db_ok = tokio::time::timeout(DB_TIMEOUT, sqlx::query("SELECT 1").execute(&state.db))
        .await
        .is_ok_and(|r| r.is_ok());
    let (code, status) = if db_ok {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "database_unavailable")
    };
    (
        code,
        Json(HealthResponse {
            status: status.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }),
    )
}
