//! En-tetes de securite poses sur TOUTES les reponses (API, interface,
//! erreurs, redirections).

use axum::Router;
use axum::http::{HeaderName, HeaderValue, header};
use tower_http::set_header::SetResponseHeaderLayer;

pub const CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'";
pub const HSTS: &str = "max-age=31536000";

fn layer(name: HeaderName, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(name, HeaderValue::from_static(value))
}

/// Ajoute les quatre en-tetes ; ils remplacent toute valeur posee en amont.
pub fn apply<S: Clone + Send + Sync + 'static>(router: Router<S>) -> Router<S> {
    router
        .layer(layer(header::CONTENT_SECURITY_POLICY, CSP))
        .layer(layer(header::STRICT_TRANSPORT_SECURITY, HSTS))
        .layer(layer(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(layer(header::REFERRER_POLICY, "no-referrer"))
}
