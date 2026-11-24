//! Erreur d'API commune : `{ "error": { "code", "message" } }`.

use std::fmt;

/// Erreur renvoyee par l'API. `code` est stable et lisible par une machine ;
/// `message` est destine a l'utilisateur et ne doit jamais contenir de secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
    /// Statut HTTP.
    pub status: u16,
}

impl ApiError {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

#[cfg(feature = "axum")]
impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = axum::http::StatusCode::from_u16(self.status)
            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        let body = serde_json::json!({
            "error": { "code": self.code, "message": self.message }
        });
        (status, axum::Json(body)).into_response()
    }
}

#[cfg(all(test, feature = "axum"))]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn into_response_has_status_and_json_shape() {
        let response = ApiError::new(404, "not_found", "Projet introuvable").into_response();
        assert_eq!(response.status().as_u16(), 404);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value,
            serde_json::json!({ "error": { "code": "not_found", "message": "Projet introuvable" } })
        );
    }

    #[tokio::test]
    async fn invalid_status_falls_back_to_500() {
        let response = ApiError::new(99, "bad", "x").into_response();
        assert_eq!(response.status().as_u16(), 500);
    }
}
