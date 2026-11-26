//! Version et mises a jour de Ciphera :
//! - `GET /api/system/version` (administrateur) : version installee et,
//!   seulement si `CIPHERA_UPDATE_CHECK=1` (desactive par defaut), derniere
//!   version publiee, lue dans `<CIPHERA_RELEASE_BASE_URL>/releases/latest.json`
//!   (`{ "version": "vX.Y.Z", "changelog_url": "https://…" }`). Une simple
//!   requete GET, sans aucune donnee de l'instance (ni identifiant, ni
//!   version dans l'agent utilisateur) : aucune telemetrie. Resultat garde en
//!   memoire 6 h (15 min apres un echec) ;
//! - `POST /api/system/update { version, password }` (proprietaire, CSRF,
//!   re-authentification par mot de passe) : refuse une version inferieure
//!   ou egale a la version installee, puis ecrit de facon atomique
//!   `<data_dir>/update-request.json` (version, date, HMAC avec la cle
//!   `<config_dir>/update-hmac.key`, `root:ciphera 0640`, propre aux
//!   demandes : ni l'identite age ni aucun autre secret). Cle absente
//!   (installation anterieure) : 503 `updates_unavailable`, l'updater la cree
//!   a son prochain passage. Le fichier appartient au compte
//!   `ciphera` (0600, dossier 0700) : seuls `ciphera` et root le lisent.
//!   `ciphera-updater.path` declenche alors l'updater (root), qui reverifie
//!   tout (HMAC, version, minisign, SHA-256) avant d'installer. Le fichier
//!   n'est lu que dans `/var/lib/ciphera` (`shared::update::REQUEST_DIR`) :
//!   avec un autre `CIPHERA_DATA_DIR`, 503 `updates_unavailable`. En cas
//!   d'echec, l'updater revient a la version et a la base d'avant : la
//!   reponse le dit (`notice`).

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use api_types::system::{RequestUpdate, SystemVersion, UpdateRequested};
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use shared::error::ApiError;
use shared::update::{KEY_FILE, KeyError, REQUEST_FILE, UpdateRequest, Version, load_key};

use crate::auth::{internal, rate_limit};
use crate::http::client_ip::ClientIp;
use crate::http::me::{Reauth, reauthenticate};
use crate::http::settings::{Admin, audit, owner_only};
use crate::state::AppState;

const MAX_BODY: usize = 8 * 1024;
/// Duree de validite de la derniere version lue.
const CHECK_TTL: Duration = Duration::from_secs(6 * 3600);
/// Delai avant un nouvel essai apres un echec.
const CHECK_RETRY: Duration = Duration::from_secs(15 * 60);
/// Taille maximale de `latest.json`.
const MAX_LATEST_BYTES: usize = 4096;
/// Avertissement renvoye avec une demande acceptee.
const NOTICE: &str = "Mise à jour demandée. Le panneau redémarre pendant l'opération. \
    En cas d'échec, Ciphera revient automatiquement à la version précédente et restaure \
    la base sauvegardée juste avant la mise à jour : les modifications faites pendant \
    la mise à jour (jusqu'à 60 s après le redémarrage) sont perdues.";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/system/version", get(version))
        .route("/api/system/update", post(request_update))
        .layer(DefaultBodyLimit::max(MAX_BODY))
}

/// Version de ce binaire (livree avec l'agent, le proxy et la CLI).
pub fn current_version() -> Version {
    Version::from_pkg(env!("CARGO_PKG_VERSION")).expect("version du paquet au format X.Y.Z")
}

/// Derniere version publiee, gardee en memoire.
#[derive(Clone, Default)]
pub struct LatestRelease(Arc<Mutex<Option<Cached>>>);

#[derive(Clone)]
struct Cached {
    at: Instant,
    latest: Option<Latest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Latest {
    version: Version,
    changelog_url: Option<String>,
}

#[derive(Deserialize)]
struct LatestJson {
    version: String,
    #[serde(default)]
    changelog_url: Option<String>,
}

impl LatestRelease {
    fn cached(&self) -> Option<Cached> {
        let guard = self.0.lock().unwrap_or_else(|p| p.into_inner());
        guard.clone().filter(|c| {
            let ttl = if c.latest.is_some() {
                CHECK_TTL
            } else {
                CHECK_RETRY
            };
            c.at.elapsed() < ttl
        })
    }

    async fn get(&self, base_url: &str) -> Option<Latest> {
        if let Some(c) = self.cached() {
            return c.latest;
        }
        let latest = match fetch_latest(base_url).await {
            Ok(l) => Some(l),
            Err(e) => {
                tracing::warn!("verification de la derniere version : {e}");
                None
            }
        };
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(Cached {
            at: Instant::now(),
            latest: latest.clone(),
        });
        latest
    }
}

async fn fetch_latest(base_url: &str) -> Result<Latest, String> {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let client = reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .user_agent("ciphera")
        .build()
        .map_err(|e| e.to_string())?;
    let res = client
        .get(format!("{base_url}/releases/latest.json"))
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !res.status().is_success() {
        return Err(format!("reponse {}", res.status().as_u16()));
    }
    if res
        .content_length()
        .is_some_and(|n| n > MAX_LATEST_BYTES as u64)
    {
        return Err("reponse trop longue".into());
    }
    let body = read_capped(res, MAX_LATEST_BYTES).await?;
    let parsed: LatestJson = serde_json::from_slice(&body).map_err(|_| "JSON invalide")?;
    let version = parsed
        .version
        .parse()
        .map_err(|_| "version invalide".to_string())?;
    // Lien affiche dans le panneau : https seulement, caracteres surs.
    let changelog_url = parsed.changelog_url.filter(|u| {
        u.starts_with("https://")
            && u.len() <= 512
            && u.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b":/._-~#?=&%+".contains(&b))
    });
    Ok(Latest {
        version,
        changelog_url,
    })
}

/// `GET /api/system/version`
async fn version(State(state): State<AppState>, _auth: Admin) -> Result<Response, ApiError> {
    let latest = if state.cfg.update_check {
        state.releases.get(&state.cfg.release_base_url).await
    } else {
        None
    };
    let body = SystemVersion {
        current: current_version().to_string(),
        latest: latest.as_ref().map(|l| l.version.to_string()),
        changelog_url: latest.and_then(|l| l.changelog_url),
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response())
}

/// `POST /api/system/update`
async fn request_update(
    State(state): State<AppState>,
    auth: Admin,
    ClientIp(ip): ClientIp,
    input: Result<Json<RequestUpdate>, JsonRejection>,
) -> Result<Response, ApiError> {
    let actor = owner_only(&auth)?;
    if state.cfg.data_dir != state.update_request_dir {
        return Err(ApiError::new(
            503,
            "updates_unavailable",
            "Ciphera : mises à jour indisponibles avec ce répertoire de données \
             (CIPHERA_DATA_DIR). L'updater ne lit les demandes que dans /var/lib/ciphera ; \
             mettre à jour en ligne de commande (sudo ciphera-cli updater --version vX.Y.Z).",
        ));
    }
    let Json(input) =
        input.map_err(|_| ApiError::new(422, "validation_failed", "Requête invalide."))?;
    let version: Version = input.version.parse().map_err(|_| {
        ApiError::new(
            422,
            "validation_failed",
            "Version invalide (attendu vX.Y.Z).",
        )
    })?;
    let current = current_version();
    if version <= current {
        return Err(ApiError::new(
            422,
            "not_newer",
            format!(
                "La version {version} n'est pas supérieure à la version installée ({current})."
            ),
        ));
    }
    // Cle des demandes lue avant la re-authentification : sans elle, rien
    // n'est demande ni consomme.
    // Absente ou refusee (taille, droits, proprietaire : reste d'une creation
    // interrompue, par exemple) : 503 documente, pas 500.
    let key_path = state.cfg.config_dir.join(KEY_FILE);
    let shown = key_path.display().to_string();
    let key = tokio::task::spawn_blocking(move || load_key(&key_path))
        .await
        .map_err(|e| internal("cle des demandes de mise a jour", e))?
        .map_err(|e| match e {
            KeyError::Missing => ApiError::new(
                503,
                "updates_unavailable",
                format!(
                    "Ciphera : clé des demandes de mise à jour absente ({shown}). \
                     La créer avec sudo systemctl start ciphera-updater.service, puis réessayer."
                ),
            ),
            KeyError::Refused(why) => {
                tracing::warn!(
                    "cle des demandes de mise a jour refusee ({shown}) : {why} ; \
                     la supprimer (sudo rm {shown}) puis la recreer avec \
                     sudo systemctl start ciphera-updater.service"
                );
                ApiError::new(
                    503,
                    "updates_unavailable",
                    format!(
                        "Ciphera : clé des demandes de mise à jour invalide ({shown}). \
                         La supprimer, la recréer avec sudo systemctl start \
                         ciphera-updater.service, puis réessayer."
                    ),
                )
            }
            other => internal("cle des demandes de mise a jour", other),
        })?;
    let ip = ip.to_string();
    let attempt = match reauthenticate(
        &state,
        &auth.user,
        &ip,
        input.password,
        "system.update.failure",
    )
    .await?
    {
        Reauth::Verified(attempt, _) => attempt,
        Reauth::Limited(response) => return Ok(response),
    };

    let mut tx = state
        .db
        .begin()
        .await
        .map_err(|e| internal("transaction", e))?;
    rate_limit::mark_mfa_success(&mut tx, attempt)
        .await
        .map_err(|e| internal("enregistrement de la tentative", e))?;
    audit(
        &mut tx,
        Some(actor),
        "system.update.request",
        json!({ "ip": ip, "from": current.to_string(), "to": version.to_string() }),
    )
    .await?;
    let request = UpdateRequest::sign(version, state.clock.unix_secs() as u64, &key[..]);
    drop(key);
    let dir = state.cfg.data_dir.clone();
    tokio::task::spawn_blocking(move || write_request(&dir, &request))
        .await
        .map_err(|e| internal("demande de mise a jour", e))?
        .map_err(|e| internal("ecriture de la demande de mise a jour", e))?;
    if let Err(e) = tx.commit().await {
        let _ = std::fs::remove_file(state.cfg.data_dir.join(REQUEST_FILE));
        return Err(internal("transaction", e));
    }
    tracing::info!("mise a jour vers {version} demandee");
    Ok((
        StatusCode::ACCEPTED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(UpdateRequested {
            version: version.to_string(),
            notice: NOTICE.to_string(),
        }),
    )
        .into_response())
}

/// Ecriture atomique : fichier temporaire 0600 dans le meme dossier, donnees
/// sur disque, puis renommage (l'unite .path ne voit jamais de demande
/// partielle).
fn write_request(dir: &Path, request: &UpdateRequest) -> std::io::Result<()> {
    let tmp = dir.join(format!(".{REQUEST_FILE}.tmp"));
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp)?;
    file.write_all(request.to_json().as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, dir.join(REQUEST_FILE))
}

/// Corps de `res`, `max` octets au plus : lu par morceaux, abandonne des que
/// la limite est depassee (corps sans longueur annoncee, `chunked`).
async fn read_capped(mut res: reqwest::Response, max: usize) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.without_url().to_string())? {
        if body.len() + chunk.len() > max {
            return Err("reponse trop longue".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Un serveur qui envoie un corps `chunked` sans fin : la lecture
    /// s'arrete a la limite au lieu de tout garder en memoire.
    #[tokio::test]
    async fn endless_chunked_body_is_cut_at_the_limit() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            let head = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                        transfer-encoding: chunked\r\n\r\n";
            if sock.write_all(head.as_bytes()).await.is_err() {
                return;
            }
            let chunk = format!("{:x}\r\n{}\r\n", 1024, "a".repeat(1024));
            while sock.write_all(chunk.as_bytes()).await.is_ok() {}
        });
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(
                rustls::ClientConfig::builder_with_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(rustls::RootCertStore::empty())
                .with_no_client_auth(),
            )
            .build()
            .unwrap();
        let res = client.get(format!("http://{addr}/")).send().await.unwrap();
        let read = tokio::time::timeout(Duration::from_secs(5), read_capped(res, 4096))
            .await
            .expect("lecture bornee");
        assert_eq!(read.unwrap_err(), "reponse trop longue");
    }
}
