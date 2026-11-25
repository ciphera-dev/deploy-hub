//! En-tetes de securite poses sur TOUTES les reponses (API, interface,
//! erreurs, redirections).

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use tower_http::set_header::SetResponseHeaderLayer;

pub const CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'";
pub const HSTS: &str = "max-age=31536000";

/// Nonce d'une page HTML (extension de la reponse) : il rejoint `style-src`
/// pour cette seule reponse. Jamais `'unsafe-inline'`.
#[derive(Clone)]
pub struct CspNonce(pub String);

fn layer(name: HeaderName, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(name, HeaderValue::from_static(value))
}

/// CSP d'une reponse : la politique fixe, avec le nonce de la page s'il y en a un.
fn policy(nonce: Option<&CspNonce>) -> HeaderValue {
    match nonce {
        Some(CspNonce(n)) => {
            let with = CSP.replacen(
                "style-src 'self'",
                &format!("style-src 'self' 'nonce-{n}'"),
                1,
            );
            HeaderValue::from_str(&with).unwrap_or_else(|_| HeaderValue::from_static(CSP))
        }
        None => HeaderValue::from_static(CSP),
    }
}

async fn csp(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let value = policy(response.extensions().get::<CspNonce>());
    response
        .headers_mut()
        .insert(header::CONTENT_SECURITY_POLICY, value);
    response
}

/// Ajoute les quatre en-tetes ; ils remplacent toute valeur posee en amont.
pub fn apply<S: Clone + Send + Sync + 'static>(router: Router<S>) -> Router<S> {
    router
        .layer(middleware::from_fn(csp))
        .layer(layer(header::STRICT_TRANSPORT_SECURITY, HSTS))
        .layer(layer(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(layer(header::REFERRER_POLICY, "no-referrer"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_joins_style_src_only() {
        let v = policy(Some(&CspNonce("abc_-".into())));
        let v = v.to_str().unwrap();
        assert!(v.contains("style-src 'self' 'nonce-abc_-';"));
        assert!(v.contains("script-src 'self';"));
        assert!(!v.contains("unsafe-inline"));
        assert_eq!(policy(None).to_str().unwrap(), CSP);
    }
}
