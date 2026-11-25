//! En-tetes de securite poses sur TOUTES les reponses (API, interface,
//! erreurs, redirections).
//!
//! HSTS : `max-age` d'un an, jamais `preload` (inscription dans les
//! navigateurs difficile a defaire). `includeSubDomains` s'ajoute seulement
//! aux reponses servies sous l'hote du domaine du panneau, et seulement si
//! cet hote est strictement sous son domaine enregistrable
//! (`panel.exemple.com`, liste des suffixes publics) ; un hote enregistrable
//! lui-meme (`exemple.com`) exige l'option du proprietaire (desactivee par
//! defaut), qui imposerait HTTPS a toute la zone.

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use tower_http::set_header::SetResponseHeaderLayer;

use crate::domain::domains::panel::{PanelSwitch, Registrable, registrable};
use crate::http::panel_host::request_host_name;

pub const CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self' https://github.com; object-src 'none'";
pub const HSTS: &str = "max-age=31536000";
/// HSTS etendu aux sous-domaines de l'hote (jamais `preload`).
pub const HSTS_SUBDOMAINS: &str = "max-age=31536000; includeSubDomains";

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

/// HSTS d'une reponse servie sous `host` (normalise), d'apres le domaine
/// du panneau `panel_host` et l'option du proprietaire `opt_in`.
pub fn hsts(host: Option<&str>, panel_host: Option<&str>, opt_in: bool) -> &'static str {
    let Some(host) = host.filter(|h| Some(*h) == panel_host) else {
        return HSTS;
    };
    match registrable(host) {
        Registrable::Below => HSTS_SUBDOMAINS,
        Registrable::Apex if opt_in => HSTS_SUBDOMAINS,
        Registrable::Apex | Registrable::Neither => HSTS,
    }
}

async fn strict_transport(
    State(panel): State<PanelSwitch>,
    request: Request,
    next: Next,
) -> Response {
    let host = request_host_name(request.headers(), request.uri());
    let mut response = next.run(request).await;
    let panel_host = panel.current().map(|p| p.host);
    let value = hsts(
        host.as_deref(),
        panel_host.as_deref(),
        panel.hsts_include_subdomains(),
    );
    response.headers_mut().insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static(value),
    );
    response
}

/// Ajoute les quatre en-tetes ; ils remplacent toute valeur posee en amont.
/// `panel` : domaine du panneau et option HSTS.
pub fn apply<S: Clone + Send + Sync + 'static>(router: Router<S>, panel: PanelSwitch) -> Router<S> {
    router
        .layer(middleware::from_fn(csp))
        .layer(middleware::from_fn_with_state(panel, strict_transport))
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

    #[test]
    fn hsts_subdomains_only_on_the_panel_host() {
        let sub = Some("panel.example.org");
        let apex = Some("example.org");
        for opt_in in [false, true] {
            assert_eq!(hsts(sub, sub, opt_in), HSTS_SUBDOMAINS);
            assert_eq!(hsts(Some("203.0.113.7"), sub, opt_in), HSTS);
            assert_eq!(hsts(None, sub, opt_in), HSTS);
            assert_eq!(hsts(sub, None, opt_in), HSTS);
            assert_eq!(hsts(Some("localhost"), Some("localhost"), opt_in), HSTS);
        }
        assert_eq!(hsts(apex, apex, false), HSTS);
        assert_eq!(hsts(apex, apex, true), HSTS_SUBDOMAINS);
        for v in [HSTS, HSTS_SUBDOMAINS] {
            assert!(!v.contains("preload"));
        }
    }
}
