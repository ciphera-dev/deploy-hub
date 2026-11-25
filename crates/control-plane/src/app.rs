use axum::Router;
use axum::http::{Method, StatusCode, Uri, header};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use shared::error::ApiError;

use crate::auth::csrf;
use crate::connectors::github;
use crate::http::{
    auth, deploy_hooks, deployments, domains, email_verification, health, invitations,
    ip_allowlist, me, mfa, notifications, organization, overview, panel_domain, password_reset,
    project_git, project_overview, projects, security_headers, settings, setup, team, update,
    variables, webauthn, ws_logs,
};
use crate::{embed, state::AppState};

/// Routeur complet du panneau : API, interface embarquee, en-tetes de securite.
pub fn build_router(state: AppState) -> Router {
    let router = Router::new()
        .route("/api/health", get(health::health))
        .merge(setup::routes())
        .merge(auth::routes())
        .merge(me::routes())
        .merge(mfa::routes())
        .merge(webauthn::routes())
        .merge(invitations::routes())
        .merge(team::routes())
        .merge(password_reset::routes())
        .merge(email_verification::routes())
        .merge(notifications::routes())
        .merge(projects::routes())
        .merge(deployments::routes())
        .merge(project_git::routes())
        .merge(project_overview::routes())
        .merge(deploy_hooks::routes())
        .merge(variables::routes())
        .merge(domains::routes())
        .merge(overview::routes())
        .merge(panel_domain::routes())
        .merge(settings::routes())
        .merge(organization::routes())
        .merge(update::routes())
        .merge(ws_logs::routes())
        .merge(github::routes())
        .fallback(fallback)
        // Toute ecriture sous /api/ exige le jeton CSRF (exemptions : csrf::EXEMPT).
        .layer(middleware::from_fn(csrf::layer))
        // Liste d'IP autorisees, avant tout le reste (exemptions :
        // ip_allowlist::exempt).
        .layer(middleware::from_fn_with_state(
            state.clone(),
            ip_allowlist::layer,
        ))
        .with_state(state);
    security_headers::apply(router)
}

async fn fallback(method: Method, uri: Uri) -> Response {
    let path = uri.path();
    if path == "/api" || path.starts_with("/api/") {
        return ApiError::new(404, "not_found", "Ressource introuvable").into_response();
    }
    // Defis ACME (`/.well-known/acme-challenge/`) : servis par le proxy du
    // noeud seulement ; jamais la page de l'interface a leur place.
    if path == "/.well-known" || path.starts_with("/.well-known/") {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    // Le repli sur l'interface ne vaut que pour la lecture d'une page.
    if method != Method::GET && method != Method::HEAD {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            [(header::ALLOW, "GET, HEAD")],
        )
            .into_response();
    }
    embed::serve(path)
}
