use serde::Serialize;
use utoipa::ToSchema;

/// Reponse de `GET /api/health`.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// `"ok"` quand le panneau repond et joint sa base ;
    /// `"database_unavailable"` (503) sinon.
    pub status: String,
    /// Version de Ciphera.
    pub version: String,
}

/// Sonde de sante (sans authentification).
#[utoipa::path(
    get,
    path = "/api/health",
    tag = "system",
    responses(
        (status = 200, description = "Le panneau repond et joint sa base", body = HealthResponse),
        (status = 503, description = "Base injoignable", body = HealthResponse)
    )
)]
#[allow(dead_code)]
pub fn health() {}
