//! Client de l'API GitHub, JWT de l'App et jetons d'installation.
//!
//! - Le JWT (RS256, 10 minutes au plus) est signe par `ring` avec la cle
//!   privee ouverte depuis `SecretBox` ; cle et JWT restent en `Zeroizing`.
//! - Les jetons d'installation ne sont gardes qu'en memoire (jamais en base,
//!   jamais journalises) et redemandes 5 minutes avant leur echeance.
//! - Les erreurs de GitHub ne sont journalisees que par leur statut : ni
//!   corps de reponse, ni en-tete, ni jeton.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_types::github::GithubRepo;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use reqwest::{Method, RequestBuilder};
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde::Deserialize;
use serde_json::json;
use shared::error::ApiError;
use zeroize::Zeroizing;

use super::{load, not_connected, unavailable};
use crate::auth::internal;
use crate::authz::{Require, lvl, res};
use crate::secrets::labels;
use crate::state::AppState;

/// Marge avant l'echeance d'un jeton d'installation : au-dela, on le redemande.
const TOKEN_REFRESH_MARGIN_SECS: i64 = 5 * 60;
/// Pages de 100 depots lues au plus par installation.
const MAX_REPO_PAGES: u32 = 10;
const API_VERSION: &str = "2022-11-28";
/// Taille maximale d'une reponse de jeton ou de metadonnees (1 Mio).
const MAX_SMALL_BODY: usize = 1024 * 1024;
/// Taille maximale d'une page de liste (installations, depots : 10 Mio).
const MAX_LIST_BODY: usize = 10 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/git/providers/github/repos", get(repos))
}

/// Echec d'un appel a GitHub (sans aucun detail sensible).
#[derive(Debug)]
pub enum GithubError {
    /// Statut HTTP inattendu.
    Status(u16),
    /// Reseau, delai ou TLS.
    Transport,
    /// Reponse illisible.
    Decode,
    /// Reponse plus grande que la borne admise.
    TooLarge,
    /// Cle privee illisible ou signature impossible.
    Key,
}

impl GithubError {
    pub fn into_api(self, context: &str) -> ApiError {
        match self {
            GithubError::Key => internal(context, "cle privee de la GitHub App inutilisable"),
            GithubError::Status(status) => {
                tracing::warn!(status, "{context} : statut GitHub inattendu");
                unavailable()
            }
            GithubError::Transport => {
                tracing::warn!("{context} : GitHub injoignable");
                unavailable()
            }
            GithubError::Decode => {
                tracing::warn!("{context} : reponse GitHub illisible");
                unavailable()
            }
            GithubError::TooLarge => {
                tracing::warn!("{context} : reponse GitHub trop volumineuse");
                unavailable()
            }
        }
    }
}

struct CachedToken {
    token: Zeroizing<String>,
    /// Echeance, en secondes depuis l'epoque Unix.
    expires_at: i64,
}

/// Client HTTP de l'API GitHub et cache memoire des jetons d'installation.
pub struct GithubClient {
    http: reqwest::Client,
    /// Par (installation, depot) : `None` = jeton non restreint (liste des
    /// depots de l'installation), `Some(nom)` = jeton limite a ce depot.
    tokens: Mutex<HashMap<(i64, Option<String>), CachedToken>>,
}

impl std::fmt::Debug for GithubClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GithubClient")
    }
}

/// Cle privee PEM (PKCS#1 `RSA PRIVATE KEY` ou PKCS#8 `PRIVATE KEY`) ouverte
/// en paire de cles `ring`.
fn key_pair(pem: &[u8]) -> Result<RsaKeyPair, GithubError> {
    let text = std::str::from_utf8(pem).map_err(|_| GithubError::Key)?;
    let mut body = Zeroizing::new(String::with_capacity(text.len()));
    let mut inside = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with("-----BEGIN ") {
            inside = true;
        } else if line.starts_with("-----END ") {
            break;
        } else if inside {
            body.push_str(line);
        }
    }
    let der = Zeroizing::new(
        STANDARD
            .decode(body.as_bytes())
            .map_err(|_| GithubError::Key)?,
    );
    RsaKeyPair::from_der(&der)
        .or_else(|_| RsaKeyPair::from_pkcs8(&der))
        .map_err(|_| GithubError::Key)
}

/// `true` si `pem` est une cle privee RSA utilisable pour signer le JWT.
pub fn valid_private_key(pem: &[u8]) -> bool {
    key_pair(pem).is_ok()
}

/// JWT de l'App : `iat` recule de 60 s (horloges decalees), `exp` a 9 min.
pub fn app_jwt(app_id: i64, pem: &[u8], now: i64) -> Result<Zeroizing<String>, GithubError> {
    let key = key_pair(pem)?;
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = json!({ "iat": now - 60, "exp": now + 540, "iss": app_id.to_string() });
    let claims = URL_SAFE_NO_PAD.encode(claims.to_string().as_bytes());
    let signing_input = format!("{header}.{claims}");
    let mut signature = vec![0u8; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signing_input.as_bytes(),
        &mut signature,
    )
    .map_err(|_| GithubError::Key)?;
    Ok(Zeroizing::new(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(&signature)
    )))
}

/// Reponse de `POST /app-manifests/{code}/conversions` (champs utilises).
#[derive(Deserialize)]
pub struct ManifestConversion {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub owner: Account,
    pub html_url: String,
    pub pem: Zeroizing<String>,
    pub webhook_secret: Option<Zeroizing<String>>,
}

#[derive(Deserialize)]
pub struct Account {
    pub login: String,
}

#[derive(Deserialize)]
struct Installation {
    id: i64,
}

/// Reponse de `PATCH /app/hook/config` (champ utilise).
#[derive(Deserialize)]
struct HookConfig {
    url: Option<String>,
}

#[derive(Deserialize)]
struct AccessToken {
    token: Zeroizing<String>,
    expires_at: String,
}

#[derive(Deserialize)]
struct Commit {
    sha: String,
}

/// Metadonnees d'un depot (champ utilise).
#[derive(Deserialize)]
struct RepoMeta {
    id: i64,
}

#[derive(Deserialize)]
struct RepoPage {
    repositories: Vec<RepoJson>,
}

#[derive(Deserialize)]
struct RepoJson {
    full_name: String,
    name: String,
    owner: Account,
    private: bool,
    default_branch: Option<String>,
    updated_at: Option<String>,
}

impl GithubClient {
    /// TLS rustls + ring, racines webpki-roots ; pas de redirection suivie.
    pub fn new() -> Result<Self, String> {
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS du client GitHub : {e}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .user_agent("ciphera")
            .build()
            .map_err(|e| format!("client GitHub : {e}"))?;
        Ok(Self {
            http,
            tokens: Mutex::new(HashMap::new()),
        })
    }

    /// Oublie les jetons d'installation (App deconnectee).
    pub fn forget_tokens(&self) {
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.clear();
        }
    }

    fn request(&self, method: Method, url: String) -> RequestBuilder {
        self.http
            .request(method, url)
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", API_VERSION)
    }

    /// Reponse JSON bornee a 1 Mio (jetons, metadonnees).
    async fn send<T: for<'de> Deserialize<'de>>(request: RequestBuilder) -> Result<T, GithubError> {
        Self::send_bounded(request, MAX_SMALL_BODY).await
    }

    /// Reponse JSON bornee a `max` octets : la longueur annoncee trop grande
    /// est refusee d'emblee, puis la lecture s'arrete des que la borne est
    /// passee (corps sans longueur annoncee compris).
    async fn send_bounded<T: for<'de> Deserialize<'de>>(
        request: RequestBuilder,
        max: usize,
    ) -> Result<T, GithubError> {
        let mut response = request.send().await.map_err(|_| GithubError::Transport)?;
        let status = response.status();
        if !status.is_success() {
            return Err(GithubError::Status(status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|len| len > max as u64)
        {
            return Err(GithubError::TooLarge);
        }
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| GithubError::Transport)? {
            if body.len() + chunk.len() > max {
                return Err(GithubError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| GithubError::Decode)
    }

    /// Echange le code temporaire du flux manifest contre l'App (sans
    /// authentification). `code` doit etre valide par l'appelant.
    pub async fn convert_manifest(
        &self,
        base: &str,
        code: &str,
    ) -> Result<ManifestConversion, GithubError> {
        Self::send(self.request(
            Method::POST,
            format!("{base}/app-manifests/{code}/conversions"),
        ))
        .await
    }

    /// Deplace le webhook de l'App vers `url` (`PATCH /app/hook/config`,
    /// JWT de l'App) ; la configuration renvoyee doit porter `url`. Le secret
    /// du webhook n'est ni envoye ni change.
    pub async fn set_webhook_url(
        &self,
        base: &str,
        app_id: i64,
        pem: &[u8],
        now: i64,
        url: &str,
    ) -> Result<(), GithubError> {
        let jwt = app_jwt(app_id, pem, now)?;
        let config: HookConfig = Self::send(
            self.request(Method::PATCH, format!("{base}/app/hook/config"))
                .bearer_auth(&*jwt)
                .json(&json!({ "url": url })),
        )
        .await?;
        if config.url.as_deref() == Some(url) {
            Ok(())
        } else {
            Err(GithubError::Decode)
        }
    }

    async fn installations(&self, base: &str, jwt: &str) -> Result<Vec<Installation>, GithubError> {
        Self::send_bounded(
            self.request(
                Method::GET,
                format!("{base}/app/installations?per_page=100"),
            )
            .bearer_auth(jwt),
            MAX_LIST_BODY,
        )
        .await
    }

    /// Jeton d'installation : celui du cache s'il vit encore plus de 5 min,
    /// sinon un nouveau (signe par le JWT de l'App). Avec `repo` (nom seul,
    /// sans proprietaire), le jeton est limite a ce depot en lecture du
    /// contenu ; sans `repo`, il couvre toute l'installation (liste des
    /// depots seulement). Un cache distinct par (installation, depot).
    async fn installation_token(
        &self,
        base: &str,
        jwt: &str,
        installation: i64,
        repo: Option<&str>,
        now: i64,
    ) -> Result<Zeroizing<String>, GithubError> {
        let key = (installation, repo.map(str::to_string));
        if let Ok(tokens) = self.tokens.lock()
            && let Some(cached) = tokens.get(&key)
            && cached.expires_at - now > TOKEN_REFRESH_MARGIN_SECS
        {
            return Ok(cached.token.clone());
        }
        let mut request = self
            .request(
                Method::POST,
                format!("{base}/app/installations/{installation}/access_tokens"),
            )
            .bearer_auth(jwt);
        if let Some(repo) = repo {
            request = request.json(&json!({
                "repositories": [repo],
                "permissions": { "contents": "read" },
            }));
        }
        let fresh: AccessToken = Self::send(request).await?;
        // Echeance illisible : jeton utilise une fois, pas mis en cache.
        let expires_at = fresh
            .expires_at
            .parse::<jiff::Timestamp>()
            .map_or(now, |t| t.as_second());
        if let Ok(mut tokens) = self.tokens.lock() {
            tokens.insert(
                key,
                CachedToken {
                    token: fresh.token.clone(),
                    expires_at,
                },
            );
        }
        Ok(fresh.token)
    }

    async fn installation_repos(
        &self,
        base: &str,
        token: &str,
    ) -> Result<Vec<RepoJson>, GithubError> {
        let mut all = Vec::new();
        for page in 1..=MAX_REPO_PAGES {
            let batch: RepoPage = Self::send_bounded(
                self.request(
                    Method::GET,
                    format!("{base}/installation/repositories?per_page=100&page={page}"),
                )
                .bearer_auth(token),
                MAX_LIST_BODY,
            )
            .await?;
            let last = batch.repositories.len() < 100;
            all.extend(batch.repositories);
            if last {
                break;
            }
        }
        Ok(all)
    }

    /// Jeton d'installation limite en lecture du contenu de `repository`
    /// (`proprietaire/nom`, deja valide) : `None` si l'App n'est pas installee
    /// sur ce depot. Le jeton n'est garde qu'en memoire (cache par
    /// installation et depot).
    pub async fn repository_token(
        &self,
        base: &str,
        app_id: i64,
        pem: &[u8],
        repository: &str,
        now: i64,
    ) -> Result<Option<Zeroizing<String>>, GithubError> {
        let jwt = app_jwt(app_id, pem, now)?;
        let name = repository.rsplit('/').next().unwrap_or(repository);
        let installation: Result<Installation, _> = Self::send(
            self.request(
                Method::GET,
                format!("{base}/repos/{repository}/installation"),
            )
            .bearer_auth(&*jwt),
        )
        .await;
        match installation {
            Ok(i) => self
                .installation_token(base, &jwt, i.id, Some(name), now)
                .await
                .map(Some),
            Err(GithubError::Status(404)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Identifiant numerique de `repository` (`GET /repos/{depot}`), avec le
    /// jeton d'installation s'il y en a un.
    pub async fn repository_id(
        &self,
        base: &str,
        repository: &str,
        token: Option<&str>,
    ) -> Result<i64, GithubError> {
        let mut request = self.request(Method::GET, format!("{base}/repos/{repository}"));
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let meta: RepoMeta = Self::send(request).await?;
        Ok(meta.id)
    }

    /// Commit en tete de `branch` (deja validee) dans `repository`, avec le
    /// jeton d'installation s'il y en a un (sinon depot public).
    pub async fn branch_head(
        &self,
        base: &str,
        repository: &str,
        branch: &str,
        token: Option<&str>,
    ) -> Result<String, GithubError> {
        let mut request = self.request(
            Method::GET,
            format!("{base}/repos/{repository}/commits/{branch}"),
        );
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let commit: Commit = Self::send(request).await?;
        shared::validate::commit_sha(&commit.sha).map_err(|_| GithubError::Decode)?;
        Ok(commit.sha)
    }

    /// Depots de toutes les installations de l'App, tries par nom.
    pub async fn repositories(
        &self,
        base: &str,
        app_id: i64,
        pem: &[u8],
        now: i64,
    ) -> Result<Vec<GithubRepo>, GithubError> {
        let jwt = app_jwt(app_id, pem, now)?;
        let mut repos: Vec<GithubRepo> = Vec::new();
        for installation in self.installations(base, &jwt).await? {
            let token = self
                .installation_token(base, &jwt, installation.id, None, now)
                .await?;
            for r in self.installation_repos(base, &token).await? {
                repos.push(GithubRepo {
                    full_name: r.full_name,
                    name: r.name,
                    owner: r.owner.login,
                    private: r.private,
                    default_branch: r.default_branch.unwrap_or_else(|| "main".to_string()),
                    updated_at: r.updated_at,
                });
            }
        }
        repos.sort_by_key(|r| r.full_name.to_ascii_lowercase());
        repos.dedup_by(|a, b| a.full_name.eq_ignore_ascii_case(&b.full_name));
        Ok(repos)
    }
}

/// Lien d'un nouveau projet avec son depot : `via_app` si l'App est
/// installee sur le depot (alors plus de clone anonyme), et l'id du depot.
#[derive(Debug, Clone, Copy, Default)]
pub struct RepoLink {
    pub via_app: bool,
    pub repo_id: Option<i64>,
}

/// Determine le lien d'un depot `proprietaire/nom` (deja valide) a la
/// creation d'un projet. Sans App, ou App non installee sur le depot : projet
/// public ordinaire. L'id du depot est lu avec le jeton restreint ; s'il
/// manque, le premier build le retient.
pub async fn resolve_link(state: &AppState, repository: &str) -> Result<RepoLink, ApiError> {
    let Some(app) = load(&state.db)
        .await
        .map_err(|e| internal("lecture de la GitHub App", e))?
    else {
        return Ok(RepoLink::default());
    };
    let pem = state
        .secrets
        .open_bound(labels::GITHUB_PRIVATE_KEY, &app.private_key_sealed)
        .map_err(|e| internal("ouverture de la cle de la GitHub App", e))?;
    let base = &state.cfg.github_api_url;
    let now = state.clock.unix_secs() as i64;
    // GitHub injoignable : la creation ne depend pas de lui ; le premier
    // build qui obtient un jeton d'installation lie le projet a l'App.
    let token = match state
        .github
        .repository_token(base, app.app_id, &pem, repository, now)
        .await
    {
        Ok(Some(token)) => token,
        Ok(None) => return Ok(RepoLink::default()),
        Err(e) => {
            tracing::warn!(error = ?e, "lien du depot GitHub non determine a la creation");
            return Ok(RepoLink::default());
        }
    };
    let repo_id = match state
        .github
        .repository_id(base, repository, Some(&token))
        .await
    {
        Ok(id) => Some(id),
        Err(e) => {
            tracing::warn!(error = ?e, "id du depot GitHub non lu a la creation");
            None
        }
    };
    Ok(RepoLink {
        via_app: true,
        repo_id,
    })
}

/// `GET /api/git/providers/github/repos`
async fn repos(
    State(state): State<AppState>,
    _auth: Require<res::Projects, lvl::Write>,
) -> Result<Response, ApiError> {
    let app = load(&state.db)
        .await
        .map_err(|e| internal("lecture de la GitHub App", e))?
        .ok_or_else(not_connected)?;
    let pem = state
        .secrets
        .open_bound(labels::GITHUB_PRIVATE_KEY, &app.private_key_sealed)
        .map_err(|e| internal("ouverture de la cle de la GitHub App", e))?;
    let now = state.clock.unix_secs() as i64;
    let repos = state
        .github
        .repositories(&state.cfg.github_api_url, app.app_id, &pem, now)
        .await
        .map_err(|e| e.into_api("liste des depots GitHub"))?;
    Ok(Json(repos).into_response())
}
