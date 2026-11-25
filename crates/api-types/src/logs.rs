//! Journaux : flux en direct (`GET /api/ws/logs`, WebSocket) et
//! telechargement (`GET /api/deployments/{id}/logs.txt`). Le flux envoie des
//! messages JSON `{ ts, level, container, message }`, masques des valeurs
//! secretes du projet ; le client n'envoie rien.

/// Flux de journaux en direct (developer et plus ; lecture des logs).
/// Mise a niveau WebSocket : l'en-tete `Origin` doit etre celui du panneau,
/// sinon 403 `origin`. Un seul de `deployment` et `service` est donne. Un
/// acces retire en cours de flux ferme la connexion (code 1008).
#[utoipa::path(
    get,
    path = "/api/ws/logs",
    tag = "deployments",
    params(
        ("deployment" = Option<String>, Query, description = "Identifiant du deploiement"),
        ("service" = Option<String>, Query, description = "Identifiant du service"),
        ("tail" = Option<u32>, Query, description = "Lignes passees envoyees a l'ouverture (500 par defaut)"),
    ),
    responses(
        (status = 101, description = "Flux ouvert (mise a niveau WebSocket)"),
        (status = 403, description = "Role insuffisant, ou origine refusee (`origin`)"),
        (status = 404, description = "Deploiement ou service introuvable"),
        (status = 422, description = "Parametres invalides"),
        (status = 429, description = "Trop de flux ouverts"),
    )
)]
#[allow(dead_code)]
pub fn stream_logs() {}

/// Telecharge le journal d'un deploiement en texte (4 Mio au plus), masque
/// des valeurs secretes du projet.
#[utoipa::path(
    get,
    path = "/api/deployments/{id}/logs.txt",
    tag = "deployments",
    params(("id" = String, Path, description = "Identifiant du deploiement")),
    responses(
        (status = 200, description = "Journal en texte brut", content_type = "text/plain", body = String),
        (status = 403, description = "Role insuffisant"),
        (status = 404, description = "Journal introuvable"),
        (status = 429, description = "Trop de telechargements en cours (`Retry-After`)"),
    )
)]
#[allow(dead_code)]
pub fn download_deployment_logs() {}
