use serde::Serialize;
use utoipa::ToSchema;

/// Reponse de `GET /api/health`.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// `"ok"` quand le panneau repond et joint sa base ;
    /// `"database_unavailable"` (503) sinon.
    pub status: String,
    /// Version de Ciphera : seulement pour un pair local de l'acces direct
    /// ou une session ouverte.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Sonde de sante (sans authentification ; la version est reservee au pair local ou a une session).
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
