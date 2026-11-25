//! HTTPS automatique du proxy : certificats ACME (HTTP-01, instant-acme),
//! stockes chiffres dans le dossier d'etat (`/var/lib/ciphera-proxy`).
//!
//! - **Defis HTTP-01** : `Challenges` garde les jetons en cours ; le service
//!   HTTP (port 80) repond a `/.well-known/acme-challenge/<jeton>` (voir
//!   `forward`).
//! - **Certificats** : `CertStore` les tient en memoire (par nom d'hote,
//!   echange atomique) et les ecrit chiffres avec age (`shared::secretbox`,
//!   le coffre du plan de controle) dans `certs/<hote>.age`. L'identite age
//!   est propre au proxy : `age.key`, mode 0600, compte `ciphera-proxy`,
//!   creee au premier demarrage. Les identifiants du compte ACME sont
//!   chiffres de meme (`acme/account-<annuaire>.age`).
//! - **Emission** : `AcmeIssuer::issue`, demandee par le plan de controle
//!   (tache `AcmeIssue`, via l'agent et le socket de pilotage). Le
//!   renouvellement a J-30 est planifie par le plan de controle.
//!
//! - **Controles** : une chaine n'est installee (a l'emission comme au
//!   chargement) que si la cle correspond au certificat feuille et que son
//!   SAN DNS couvre l'hote ; dossier d'etat, `certs/`, `acme/`, identite et
//!   fichiers chiffres doivent appartenir au compte du proxy, sans aucun
//!   droit pour le groupe ni les autres (dossier ou identite refuses au
//!   demarrage, fichier de certificat ignore).
//! - **Suppression pendant une emission** : chaque `RemoveCertificate`
//!   incremente un compteur par hote ; une emission note ce compteur a son
//!   debut (`Ticket`) et n'installe rien s'il a change entre-temps (ni
//!   certificat ni cle chiffree orphelins). Le compteur n'est tenu que
//!   pendant qu'une emission de l'hote est en cours : il est purge a la fin
//!   de la derniere.
//! - **Expiration** : un certificat expire n'est ni installe, ni charge du
//!   disque (avertissement journalise ; le plan de controle l'apprend par
//!   `Missing`), ni servi s'il expire en cours de route.
//!
//! Aucune cle privee (ni certificat, ni compte ACME) n'est journalisee ni
//! renvoyee : seules la fin de validite et l'emetteur sortent du proxy.

use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arc_swap::ArcSwap;
use instant_acme::{
    Account, AccountCredentials, AuthorizationStatus, ChallengeType, Identifier, LetsEncrypt,
    NewAccount, NewOrder, OrderStatus, RetryPolicy,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use shared::secretbox::SecretBox;
use zeroize::Zeroizing;

/// Prefixe des chemins de defi HTTP-01 (toujours servi en HTTP clair).
pub const CHALLENGE_PREFIX: &str = "/.well-known/acme-challenge/";
/// Annuaire ACME par defaut (Let's Encrypt, production).
pub const DEFAULT_DIRECTORY: &str = LetsEncrypt::Production.url();
/// Duree maximale d'une emission complete.
pub const ISSUE_TIMEOUT: Duration = Duration::from_secs(180);
/// Nombre maximal de defis en cours (borne la memoire).
const MAX_CHALLENGES: usize = 1024;
/// Longueur maximale d'un jeton de defi.
const MAX_TOKEN_LEN: usize = 128;
const KEY_FILE: &str = "age.key";
const CERTS_DIR: &str = "certs";
const ACCOUNT_DIR: &str = "acme";
/// Intervalle minimal entre deux journaux des poignees de main sans
/// certificat.
const MISS_LOG_EVERY_SECS: u64 = 60;

/// Annuaire ACME choisi par `CIPHERA_PROXY_ACME_DIRECTORY` : `production`
/// (defaut), `staging` (Let's Encrypt de test) ou une URL `https://`
/// (Pebble dans les tests).
pub fn directory_url(value: Option<&str>) -> Result<String, String> {
    match value.map(str::trim) {
        None | Some("") | Some("production") => Ok(DEFAULT_DIRECTORY.to_string()),
        Some("staging") => Ok(LetsEncrypt::Staging.url().to_string()),
        Some(url) if url.starts_with("https://") && !url.contains(char::is_whitespace) => {
            Ok(url.to_string())
        }
        Some(_) => Err("annuaire ACME invalide (production, staging ou https://...)".into()),
    }
}

/// Erreur d'emission (raison sans secret).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcmeError(pub String);

impl fmt::Display for AcmeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AcmeError {}

fn err(context: &str, e: impl fmt::Display) -> AcmeError {
    AcmeError(format!("{context} : {e}"))
}

/// Nom d'hote acceptable pour un certificat (et un nom de fichier) :
/// etiquettes `[a-z0-9-]`, au moins deux, ni adresse IP ni joker.
pub fn valid_cert_host(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && host.parse::<std::net::IpAddr>().is_err()
        && host.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

/// Jetons HTTP-01 en cours : jeton -> autorisation de cle.
#[derive(Debug, Default)]
pub struct Challenges {
    tokens: Mutex<HashMap<String, String>>,
}

impl Challenges {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.tokens.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Reponse du defi `token`, s'il est en cours.
    pub fn get(&self, token: &str) -> Option<String> {
        if !valid_token(token) {
            return None;
        }
        self.lock().get(token).cloned()
    }

    /// Publie la reponse du defi `token` (jeton base64url).
    pub fn insert(&self, token: &str, key_authorization: String) -> Result<(), AcmeError> {
        if !valid_token(token) {
            return Err(AcmeError("jeton de defi invalide".into()));
        }
        let mut tokens = self.lock();
        if tokens.len() >= MAX_CHALLENGES {
            return Err(AcmeError("trop de defis en cours".into()));
        }
        tokens.insert(token.to_string(), key_authorization);
        Ok(())
    }

    pub fn remove(&self, token: &str) {
        self.lock().remove(token);
    }
}

/// Jeton base64url (RFC 8555, section 8.3).
fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_TOKEN_LEN
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Retire les jetons a la fin de l'emission (succes, echec ou abandon).
struct TokenGuard<'a> {
    challenges: &'a Challenges,
    tokens: Vec<String>,
}

impl Drop for TokenGuard<'_> {
    fn drop(&mut self) {
        for t in &self.tokens {
            self.challenges.remove(t);
        }
    }
}

/// Certificat installe.
#[derive(Debug)]
pub struct Installed {
    pub key: Arc<CertifiedKey>,
    /// Fin de validite (secondes Unix).
    pub not_after: i64,
    /// Nom commun de l'emetteur.
    pub issuer: String,
}

impl Installed {
    /// Le certificat est-il encore valide a `now` (secondes Unix) ?
    fn valid_at(&self, now: i64) -> bool {
        self.not_after > now
    }
}

/// Suppressions suivies pendant les emissions en cours : par hote, nombre
/// d'emissions en cours et de suppressions vues depuis le debut de la
/// premiere.
#[derive(Debug, Default)]
struct Removals {
    hosts: HashMap<String, HostRemovals>,
}

#[derive(Debug, Default, Clone, Copy)]
struct HostRemovals {
    in_flight: u32,
    removed: u64,
}

type SharedRemovals = Arc<Mutex<Removals>>;

fn lock_removals(shared: &SharedRemovals) -> std::sync::MutexGuard<'_, Removals> {
    shared.lock().unwrap_or_else(|p| p.into_inner())
}

/// Emission en cours pour un hote : nombre de suppressions de l'hote vu a
/// son debut (`CertStore::ticket`). Sa destruction (fin ou abandon de
/// l'emission) libere le suivi de l'hote.
#[derive(Debug)]
pub struct Ticket {
    host: String,
    removals: u64,
    shared: SharedRemovals,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let mut state = lock_removals(&self.shared);
        if let Some(entry) = state.hosts.get_mut(&self.host) {
            entry.in_flight = entry.in_flight.saturating_sub(1);
            if entry.in_flight == 0 {
                state.hosts.remove(&self.host);
            }
        }
    }
}

/// Certificats par nom d'hote, en memoire et (sauf en test) chiffres sur
/// disque.
pub struct CertStore {
    certs: ArcSwap<HashMap<String, Arc<Installed>>>,
    /// Verrou des ecritures ; suppressions suivies pendant les emissions.
    write: SharedRemovals,
    disk: Option<(PathBuf, Arc<SecretBox>)>,
}

impl fmt::Debug for CertStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CertStore")
            .field("hosts", &self.certs.load().len())
            .finish_non_exhaustive()
    }
}

impl CertStore {
    /// Magasin en memoire seulement (tests).
    pub fn memory() -> Self {
        Self {
            certs: ArcSwap::from_pointee(HashMap::new()),
            write: SharedRemovals::default(),
            disk: None,
        }
    }

    /// Magasin du dossier d'etat : certificats de `certs/` dechiffres et
    /// charges. Le dossier `certs/` doit etre prive (sinon erreur) ; un
    /// fichier illisible, trop ouvert, d'un autre compte ou dont le
    /// certificat ne couvre pas l'hote est ignore (avertissement, sans son
    /// contenu).
    pub fn open(state_dir: &Path, sealer: Arc<SecretBox>) -> std::io::Result<Self> {
        let dir = state_dir.join(CERTS_DIR);
        create_private_dir(&dir)?;
        check_private(&dir, true)?;
        let mut certs = HashMap::new();
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let Some(host) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".age"))
                .filter(|h| valid_cert_host(h))
                .map(str::to_string)
            else {
                continue;
            };
            let loaded = check_private(&path, false)
                .and_then(|()| std::fs::read(&path))
                .map_err(|e| err("lecture", e))
                .and_then(|sealed| sealer.open(&sealed).map_err(|e| err("dechiffrement", e)))
                .and_then(|pem| parse_bundle(&pem, &host));
            match loaded {
                Ok(installed) => {
                    certs.insert(host, Arc::new(installed));
                }
                Err(e) => tracing::warn!(%host, "certificat stocke ignore : {e}"),
            }
        }
        tracing::info!(certificats = certs.len(), "certificats charges");
        Ok(Self {
            certs: ArcSwap::from_pointee(certs),
            write: SharedRemovals::default(),
            disk: Some((dir, sealer)),
        })
    }

    pub fn get(&self, host: &str) -> Option<Arc<Installed>> {
        self.certs.load().get(host).cloned()
    }

    /// Un certificat valide existe pour `host` (redirection 80 -> 443).
    pub fn has(&self, host: &str) -> bool {
        self.serving(host).is_some()
    }

    /// Certificat de `host` s'il est encore valide (jamais un expire).
    pub fn serving(&self, host: &str) -> Option<Arc<CertifiedKey>> {
        self.get(host)
            .filter(|c| c.valid_at(unix_now()))
            .map(|c| c.key.clone())
    }

    fn lock_write(&self) -> std::sync::MutexGuard<'_, Removals> {
        lock_removals(&self.write)
    }

    /// Hotes dont le compteur de suppressions est encore tenu (emissions en
    /// cours seulement).
    pub fn pending_removals(&self) -> usize {
        self.lock_write().hosts.len()
    }

    /// Debut d'une emission pour `host` (voir `install_ticket`).
    pub fn ticket(&self, host: &str) -> Ticket {
        let mut state = self.lock_write();
        let entry = state.hosts.entry(host.to_string()).or_default();
        entry.in_flight += 1;
        let removals = entry.removed;
        drop(state);
        Ticket {
            host: host.to_string(),
            removals,
            shared: self.write.clone(),
        }
    }

    /// Valide puis installe la chaine `chain_pem` et sa cle `key_pem` pour
    /// `host` : ecrite chiffree (si le magasin est sur disque), puis visible
    /// des nouvelles poignees de main.
    pub fn install(
        &self,
        host: &str,
        chain_pem: &str,
        key_pem: &str,
    ) -> Result<Arc<Installed>, AcmeError> {
        let ticket = self.ticket(host);
        self.install_ticket(&ticket, chain_pem, key_pem)
    }

    /// Comme `install`, pour l'emission commencee par `ticket` : refusee si
    /// l'hote a ete supprime depuis (rien n'est ecrit).
    pub fn install_ticket(
        &self,
        ticket: &Ticket,
        chain_pem: &str,
        key_pem: &str,
    ) -> Result<Arc<Installed>, AcmeError> {
        let host = ticket.host.as_str();
        if !valid_cert_host(host) {
            return Err(AcmeError("nom d'hote invalide".into()));
        }
        let bundle = Zeroizing::new(format!("{key_pem}\n{chain_pem}"));
        let installed = Arc::new(parse_bundle(bundle.as_bytes(), host)?);
        let removals = self.lock_write();
        if removals.hosts.get(host).map_or(0, |h| h.removed) != ticket.removals {
            return Err(AcmeError(
                "domaine supprime pendant l'emission : certificat abandonne".into(),
            ));
        }
        if let Some((dir, sealer)) = &self.disk {
            let sealed = sealer
                .seal(bundle.as_bytes())
                .map_err(|e| err("chiffrement", e))?;
            write_private(&dir.join(format!("{host}.age")), &sealed)
                .map_err(|e| err("ecriture du certificat", e))?;
        }
        let mut next = HashMap::clone(&self.certs.load());
        next.insert(host.to_string(), installed.clone());
        self.certs.store(Arc::new(next));
        drop(removals);
        Ok(installed)
    }

    /// Retire le certificat de `host` (domaine supprime) : de la memoire
    /// (plus de poignee de main SNI) et du disque (cle privee supprimee) ;
    /// une emission deja commencee pour cet hote n'installera rien. Renvoie
    /// `true` si un certificat etait installe. Jamais appele pour une route
    /// retiree.
    pub fn remove(&self, host: &str) -> bool {
        let mut removals = self.lock_write();
        // Sans emission en cours pour cet hote, il n'y a rien a invalider.
        if let Some(entry) = removals.hosts.get_mut(host) {
            entry.removed += 1;
        }
        let current = self.certs.load();
        let held = current.contains_key(host);
        if held {
            let mut next = HashMap::clone(&current);
            next.remove(host);
            self.certs.store(Arc::new(next));
        }
        if let Some((dir, _)) = &self.disk
            && valid_cert_host(host)
        {
            match std::fs::remove_file(dir.join(format!("{host}.age"))) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => tracing::warn!(%host, "certificat retire, fichier conserve : {e}"),
            }
        }
        drop(removals);
        if held {
            tracing::info!(%host, "certificat retire (domaine supprime)");
        }
        held
    }
}

/// Cle privee et chaine (PEM) -> certificat pret pour rustls, seulement si
/// la cle est celle du certificat feuille et que son SAN DNS couvre `host`.
fn parse_bundle(pem: &[u8], host: &str) -> Result<Installed, AcmeError> {
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(pem)
        .collect::<Result<_, _>>()
        .map_err(|_| AcmeError("chaine PEM illisible".into()))?;
    let leaf = chain
        .first()
        .ok_or_else(|| AcmeError("chaine de certificats vide".into()))?;
    let (_, cert) = x509_parser::parse_x509_certificate(leaf.as_ref())
        .map_err(|_| AcmeError("certificat illisible".into()))?;
    let covered = cert
        .subject_alternative_name()
        .ok()
        .flatten()
        .is_some_and(|san| {
            san.value.general_names.iter().any(|name| {
                matches!(name, x509_parser::extensions::GeneralName::DNSName(dns)
                    if dns.trim_end_matches('.').eq_ignore_ascii_case(host))
            })
        });
    if !covered {
        return Err(AcmeError(format!("le certificat ne couvre pas {host}")));
    }
    let not_after = cert.validity().not_after.timestamp();
    if not_after <= unix_now() {
        return Err(AcmeError(format!("certificat expire pour {host}")));
    }
    let issuer = cert
        .issuer()
        .iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .unwrap_or("")
        .chars()
        .take(200)
        .collect();
    let key =
        PrivateKeyDer::from_pem_slice(pem).map_err(|_| AcmeError("cle privee illisible".into()))?;
    let signing = rustls::crypto::ring::sign::any_supported_type(&key)
        .map_err(|_| AcmeError("type de cle non pris en charge".into()))?;
    let certified = CertifiedKey::new(chain, signing);
    certified
        .keys_match()
        .map_err(|_| AcmeError("la cle ne correspond pas au certificat".into()))?;
    Ok(Installed {
        key: Arc::new(certified),
        not_after,
        issuer,
    })
}

/// Choix du certificat par SNI. Sans SNI ou sans certificat pour le nom :
/// poignee de main refusee, comptee et resumee dans le journal au plus une
/// fois par minute (au lieu d'une ligne `ERROR` de Pingora par connexion).
#[derive(Debug)]
pub struct CertResolver {
    store: Arc<CertStore>,
    misses: AtomicU64,
    last_log: AtomicU64,
}

impl CertResolver {
    pub fn new(store: Arc<CertStore>) -> Self {
        Self {
            store,
            misses: AtomicU64::new(0),
            last_log: AtomicU64::new(0),
        }
    }

    fn miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
        let now = unix_now().max(0) as u64;
        let last = self.last_log.load(Ordering::Relaxed);
        if now.saturating_sub(last) >= MISS_LOG_EVERY_SECS
            && self
                .last_log
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            let total = self.misses.swap(0, Ordering::Relaxed);
            tracing::warn!(
                refusees = total,
                "poignees de main TLS refusees (SNI absent ou sans certificat)"
            );
        }
    }
}

impl ResolvesServerCert for CertResolver {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let found = hello.server_name().and_then(|name| {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            self.store.serving(&name)
        });
        if found.is_none() {
            self.miss();
        }
        found
    }
}

/// Configuration ACME du proxy.
#[derive(Clone, Debug)]
pub struct AcmeConfig {
    /// URL de l'annuaire (`directory_url`).
    pub directory: String,
    /// Racine de confiance supplementaire pour l'annuaire (Pebble) ; sans
    /// valeur, les racines du systeme.
    pub ca_file: Option<PathBuf>,
    /// Adresse de contact (`mailto:`), facultative.
    pub contact: Option<String>,
}

/// Emission des certificats (un compte ACME par annuaire, cree au premier
/// besoin puis conserve chiffre).
pub struct AcmeIssuer {
    cfg: AcmeConfig,
    state_dir: PathBuf,
    sealer: Arc<SecretBox>,
    store: Arc<CertStore>,
    challenges: Arc<Challenges>,
    account: tokio::sync::Mutex<Option<Account>>,
}

impl fmt::Debug for AcmeIssuer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AcmeIssuer")
            .field("directory", &self.cfg.directory)
            .finish_non_exhaustive()
    }
}

impl AcmeIssuer {
    pub fn new(
        cfg: AcmeConfig,
        state_dir: PathBuf,
        sealer: Arc<SecretBox>,
        store: Arc<CertStore>,
        challenges: Arc<Challenges>,
    ) -> Self {
        Self {
            cfg,
            state_dir,
            sealer,
            store,
            challenges,
            account: tokio::sync::Mutex::new(None),
        }
    }

    fn builder(&self) -> Result<instant_acme::AccountBuilder, AcmeError> {
        match &self.cfg.ca_file {
            Some(path) => Account::builder_with_root(path),
            None => Account::builder(),
        }
        .map_err(|e| err("client ACME", e))
    }

    fn account_path(&self) -> PathBuf {
        let slug: String = self
            .cfg
            .directory
            .bytes()
            .filter(u8::is_ascii_alphanumeric)
            .map(|b| char::from(b.to_ascii_lowercase()))
            .take(120)
            .collect();
        self.state_dir
            .join(ACCOUNT_DIR)
            .join(format!("account-{slug}.age"))
    }

    /// Compte ACME : en memoire, sinon relu (chiffre), sinon cree.
    async fn account(&self) -> Result<Account, AcmeError> {
        let mut slot = self.account.lock().await;
        if let Some(account) = slot.as_ref() {
            return Ok(account.clone());
        }
        let path = self.account_path();
        if let Some(dir) = path.parent().filter(|d| d.exists()) {
            check_private(dir, true).map_err(|e| err("dossier du compte ACME", e))?;
        }
        let account = match check_private(&path, false).and_then(|()| std::fs::read(&path)) {
            Ok(sealed) => {
                let json = self
                    .sealer
                    .open(&sealed)
                    .map_err(|e| err("compte ACME", e))?;
                let creds: AccountCredentials = serde_json::from_slice(&json)
                    .map_err(|_| AcmeError("compte ACME illisible".into()))?;
                self.builder()?
                    .from_credentials(creds)
                    .await
                    .map_err(|e| err("compte ACME", e))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let contact: Vec<&str> = self.cfg.contact.as_deref().into_iter().collect();
                let (account, creds) = self
                    .builder()?
                    .create(
                        &NewAccount {
                            contact: &contact,
                            terms_of_service_agreed: true,
                            only_return_existing: false,
                        },
                        self.cfg.directory.clone(),
                        None,
                    )
                    .await
                    .map_err(|e| err("creation du compte ACME", e))?;
                let json = Zeroizing::new(
                    serde_json::to_vec(&creds).map_err(|_| AcmeError("compte ACME".into()))?,
                );
                let sealed = self
                    .sealer
                    .seal(&json)
                    .map_err(|e| err("chiffrement du compte ACME", e))?;
                if let Some(dir) = path.parent() {
                    create_private_dir(dir).map_err(|e| err("dossier du compte ACME", e))?;
                }
                write_private(&path, &sealed).map_err(|e| err("ecriture du compte ACME", e))?;
                tracing::info!(annuaire = %self.cfg.directory, "compte ACME cree");
                account
            }
            Err(e) => return Err(err("lecture du compte ACME", e)),
        };
        *slot = Some(account.clone());
        Ok(account)
    }

    /// Magasin des certificats emis.
    pub fn store(&self) -> &CertStore {
        &self.store
    }

    /// Emet (ou renouvelle) le certificat de `host` par HTTP-01 et
    /// l'installe. Renvoie sa fin de validite et son emetteur.
    pub async fn issue(&self, host: &str) -> Result<Arc<Installed>, AcmeError> {
        if !valid_cert_host(host) {
            return Err(AcmeError("nom d'hote invalide".into()));
        }
        tokio::time::timeout(ISSUE_TIMEOUT, self.issue_inner(host))
            .await
            .map_err(|_| AcmeError("delai de l'emission ACME depasse".into()))?
    }

    async fn issue_inner(&self, host: &str) -> Result<Arc<Installed>, AcmeError> {
        // Note avant tout echange : une suppression de l'hote pendant
        // l'emission fera abandonner le certificat.
        let ticket = self.store.ticket(host);
        let account = self.account().await?;
        let identifiers = [Identifier::Dns(host.to_string())];
        let mut order = account
            .new_order(&NewOrder::new(&identifiers))
            .await
            .map_err(|e| err("commande ACME", e))?;
        let mut guard = TokenGuard {
            challenges: &self.challenges,
            tokens: Vec::new(),
        };
        let mut authorizations = order.authorizations();
        while let Some(result) = authorizations.next().await {
            let mut authz = result.map_err(|e| err("autorisation ACME", e))?;
            match authz.status {
                AuthorizationStatus::Pending => {}
                AuthorizationStatus::Valid => continue,
                other => return Err(AcmeError(format!("autorisation ACME : {other:?}"))),
            }
            let mut challenge = authz
                .challenge(ChallengeType::Http01)
                .ok_or_else(|| AcmeError("aucun defi HTTP-01 propose".into()))?;
            let token = challenge.token.clone();
            self.challenges
                .insert(&token, challenge.key_authorization().as_str().to_string())?;
            guard.tokens.push(token);
            challenge
                .set_ready()
                .await
                .map_err(|e| err("defi HTTP-01", e))?;
        }
        let retry = RetryPolicy::new()
            .initial_delay(Duration::from_millis(500))
            .timeout(Duration::from_secs(90));
        let status = order
            .poll_ready(&retry)
            .await
            .map_err(|e| err("validation ACME", e))?;
        drop(guard);
        if status != OrderStatus::Ready {
            return Err(AcmeError(format!(
                "validation ACME refusee ({status:?}) : verifiez que {host} pointe vers ce serveur et que le port 80 est joignable"
            )));
        }
        let key_pem = Zeroizing::new(
            order
                .finalize()
                .await
                .map_err(|e| err("finalisation ACME", e))?,
        );
        let chain = order
            .poll_certificate(&retry)
            .await
            .map_err(|e| err("certificat ACME", e))?;
        let installed = self.store.install_ticket(&ticket, &chain, &key_pem)?;
        tracing::info!(%host, not_after = installed.not_after, "certificat installe");
        Ok(installed)
    }
}

/// Identite age du proxy (`<state_dir>/age.key`), creee en 0600 si absente.
pub fn load_or_create_key(state_dir: &Path) -> Result<SecretBox, String> {
    create_private_dir(state_dir).map_err(|e| format!("{} : {e}", state_dir.display()))?;
    check_private(state_dir, true).map_err(|e| format!("{} : {e}", state_dir.display()))?;
    let path = state_dir.join(KEY_FILE);
    match std::fs::symlink_metadata(&path) {
        Ok(_) => {
            check_private(&path, false).map_err(|e| format!("{} : {e}", path.display()))?;
            SecretBox::load(&path).map_err(|e| format!("{} : {e}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let (sealer, line) = SecretBox::generate();
            write_new_private(&path, line.as_bytes())
                .map_err(|e| format!("{} : {e}", path.display()))?;
            tracing::info!("identite de chiffrement des certificats creee");
            Ok(sealer)
        }
        Err(e) => Err(format!("{} : {e}", path.display())),
    }
}

/// Dossier (`dir`) ou fichier ordinaire prive du compte du proxy : ni lien
/// symbolique, proprietaire = uid effectif, aucun droit pour le groupe ni
/// les autres (0700 / 0600).
#[cfg(unix)]
fn check_private(path: &Path, dir: bool) -> std::io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let refuse = |why: String| {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            why,
        ))
    };
    let meta = std::fs::symlink_metadata(path)?;
    let (kind_ok, kind, mode) = if dir {
        (meta.is_dir(), "un dossier", "0700")
    } else {
        (meta.is_file(), "un fichier ordinaire", "0600")
    };
    if !kind_ok {
        return refuse(format!("doit etre {kind} (lien symbolique refuse)"));
    }
    if meta.uid() != rustix::process::geteuid().as_raw() {
        return refuse("doit appartenir au compte du proxy".into());
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return refuse(format!(
            "doit etre en mode {mode} (reserve au seul compte du proxy)"
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_private(path: &Path, dir: bool) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if (dir && meta.is_dir()) || (!dir && meta.is_file()) {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "type de fichier inattendu",
        ))
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

fn private_options() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn write_new_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = private_options().create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Ecriture atomique (fichier temporaire 0600 puis renommage).
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let written = write_new_private(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn self_signed(host: &str) -> (String, String) {
        let ck = rcgen::generate_simple_self_signed(vec![host.to_string()]).unwrap();
        (ck.cert.pem(), ck.signing_key.serialize_pem())
    }

    fn expired(host: &str) -> (String, String) {
        let mut params = rcgen::CertificateParams::new(vec![host.to_string()]).unwrap();
        params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        params.not_after = rcgen::date_time_ymd(2021, 1, 1);
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        (cert.pem(), key.serialize_pem())
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ciphera-proxy-acme-{}-{}",
            std::process::id(),
            unix_now_nanos()
        ));
        create_private_dir(&dir).unwrap();
        dir
    }

    fn unix_now_nanos() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn directory_defaults_to_lets_encrypt_production() {
        assert_eq!(directory_url(None).unwrap(), DEFAULT_DIRECTORY);
        assert!(DEFAULT_DIRECTORY.starts_with("https://acme-v02.api.letsencrypt.org"));
        assert!(directory_url(Some("staging")).unwrap().contains("staging"));
        assert_eq!(
            directory_url(Some("https://localhost:14000/dir")).unwrap(),
            "https://localhost:14000/dir"
        );
        assert!(directory_url(Some("http://localhost:14000/dir")).is_err());
    }

    #[test]
    fn certificate_hosts_are_dns_names_only() {
        assert!(valid_cert_host("app.example.com"));
        assert!(!valid_cert_host("localhost"));
        assert!(!valid_cert_host("203.0.113.7"));
        assert!(!valid_cert_host("*.example.com"));
        assert!(!valid_cert_host("../etc.passwd"));
        assert!(!valid_cert_host("App.Example.com"));
    }

    #[test]
    fn challenge_tokens_are_base64url() {
        let c = Challenges::new();
        c.insert("abc-DEF_123", "abc-DEF_123.thumb".into()).unwrap();
        assert_eq!(c.get("abc-DEF_123").as_deref(), Some("abc-DEF_123.thumb"));
        assert!(c.insert("../x", "y".into()).is_err());
        assert_eq!(c.get("../x"), None);
        c.remove("abc-DEF_123");
        assert_eq!(c.get("abc-DEF_123"), None);
    }

    #[test]
    fn store_keeps_keys_encrypted_on_disk_and_reloads_them() {
        let dir = temp_dir();
        let sealer = Arc::new(load_or_create_key(&dir).unwrap());
        let store = CertStore::open(&dir, sealer.clone()).unwrap();
        let (chain, key) = self_signed("app.example.com");
        let installed = store.install("app.example.com", &chain, &key).unwrap();
        assert!(installed.not_after > unix_now());
        assert!(store.has("app.example.com"));

        let on_disk = std::fs::read(dir.join("certs").join("app.example.com.age")).unwrap();
        let text = String::from_utf8_lossy(&on_disk);
        assert!(!text.contains("PRIVATE KEY") && !text.contains("CERTIFICATE"));

        // Nouveau demarrage : meme identite relue, certificat recharge.
        let sealer = Arc::new(load_or_create_key(&dir).unwrap());
        let reloaded = CertStore::open(&dir, sealer).unwrap();
        let again = reloaded.get("app.example.com").unwrap();
        assert_eq!(again.not_after, installed.not_after);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for f in [dir.join("age.key"), dir.join("certs/app.example.com.age")] {
                let mode = std::fs::metadata(&f).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o600, "{}", f.display());
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_deletes_the_certificate_and_its_file() {
        let dir = temp_dir();
        let sealer = Arc::new(load_or_create_key(&dir).unwrap());
        let store = CertStore::open(&dir, sealer).unwrap();
        for host in ["keep.example.com", "gone.example.com"] {
            let (chain, key) = self_signed(host);
            store.install(host, &chain, &key).unwrap();
        }
        assert!(store.remove("gone.example.com"));
        assert!(store.has("keep.example.com") && !store.has("gone.example.com"));
        assert!(dir.join("certs/keep.example.com.age").exists());
        assert!(!dir.join("certs/gone.example.com.age").exists());
        assert!(!store.remove("gone.example.com"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// N4 : un domaine supprime pendant son emission ne laisse ni
    /// certificat ni cle chiffree ; une emission commencee apres la
    /// suppression (hote ajoute de nouveau) s'installe normalement.
    #[test]
    fn removal_during_issuance_discards_the_late_certificate() {
        let dir = temp_dir();
        let sealer = Arc::new(load_or_create_key(&dir).unwrap());
        let store = CertStore::open(&dir, sealer).unwrap();
        let host = "late.example.com";
        let (chain, key) = self_signed(host);
        let ticket = store.ticket(host);
        assert!(!store.remove(host), "rien a retirer encore");
        let err = store.install_ticket(&ticket, &chain, &key).unwrap_err();
        assert!(err.0.contains("supprime"), "{err}");
        assert!(!store.has(host));
        assert!(!dir.join("certs/late.example.com.age").exists());

        let ticket = store.ticket(host);
        store.install_ticket(&ticket, &chain, &key).unwrap();
        assert!(store.has(host));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// M5 : la cle doit etre celle du certificat feuille, et le certificat
    /// doit couvrir l'hote (SAN DNS) ; au chargement comme a l'emission.
    #[test]
    fn install_checks_key_match_and_san() {
        let store = CertStore::memory();
        let (chain_a, _) = self_signed("a.example.com");
        let (_, key_b) = self_signed("a.example.com");
        let err = store
            .install("a.example.com", &chain_a, &key_b)
            .unwrap_err();
        assert!(err.0.contains("cle"), "{err}");
        let (chain, key) = self_signed("other.example.com");
        let err = store.install("a.example.com", &chain, &key).unwrap_err();
        assert!(err.0.contains("a.example.com"), "{err}");
        assert!(!store.has("a.example.com"));
        let (chain, key) = self_signed("A.Example.com");
        store.install("a.example.com", &chain, &key).unwrap();

        // Fichier stocke sous le nom d'un autre hote : ignore au chargement.
        let dir = temp_dir();
        let sealer = Arc::new(load_or_create_key(&dir).unwrap());
        let disk = CertStore::open(&dir, sealer.clone()).unwrap();
        let (chain, key) = self_signed("b.example.com");
        disk.install("b.example.com", &chain, &key).unwrap();
        std::fs::rename(
            dir.join("certs/b.example.com.age"),
            dir.join("certs/c.example.com.age"),
        )
        .unwrap();
        let reloaded = CertStore::open(&dir, sealer).unwrap();
        assert!(!reloaded.has("c.example.com") && !reloaded.has("b.example.com"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// M10 : dossier d'etat, identite et certificats lisibles du seul compte
    /// du proxy ; sinon refus (dossier, identite) ou fichier ignore.
    #[cfg(unix)]
    #[test]
    fn loose_state_permissions_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path, m: u32| {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap()
        };
        let dir = temp_dir();
        mode(&dir, 0o755);
        let err = load_or_create_key(&dir).unwrap_err();
        assert!(err.contains("0700"), "{err}");
        mode(&dir, 0o700);
        let sealer = Arc::new(load_or_create_key(&dir).unwrap());

        mode(&dir.join("age.key"), 0o640);
        assert!(load_or_create_key(&dir).is_err());
        mode(&dir.join("age.key"), 0o600);

        let store = CertStore::open(&dir, sealer.clone()).unwrap();
        let (chain, key) = self_signed("loose.example.com");
        store.install("loose.example.com", &chain, &key).unwrap();
        mode(&dir.join("certs/loose.example.com.age"), 0o644);
        let reloaded = CertStore::open(&dir, sealer.clone()).unwrap();
        assert!(
            !reloaded.has("loose.example.com"),
            "fichier trop ouvert ignore"
        );
        mode(&dir.join("certs"), 0o750);
        assert!(CertStore::open(&dir, sealer).is_err());

        // Lien symbolique a la place du dossier d'etat : refuse.
        let link = dir.with_extension("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert!(load_or_create_key(&link).is_err());
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_refuses_mismatched_or_garbage_material() {
        let store = CertStore::memory();
        assert!(
            store
                .install("app.example.com", "pas un pem", "rien")
                .is_err()
        );
        assert!(store.install("bad host", "", "").is_err());
        assert!(!store.has("app.example.com"));
    }

    /// Un certificat expire n'est ni installe, ni charge du disque, ni servi.
    #[test]
    fn expired_cert_not_loaded() {
        let host = "old.example.com";
        let (chain, key) = expired(host);
        let memory = CertStore::memory();
        let err = memory.install(host, &chain, &key).unwrap_err();
        assert!(err.0.contains("expire"), "{err}");
        assert!(!memory.has(host));

        // Fichier chiffre valide mais expire, a cote d'un certificat valide.
        let dir = temp_dir();
        let sealer = Arc::new(load_or_create_key(&dir).unwrap());
        let store = CertStore::open(&dir, sealer.clone()).unwrap();
        let (good_chain, good_key) = self_signed("fresh.example.com");
        store
            .install("fresh.example.com", &good_chain, &good_key)
            .unwrap();
        let sealed = sealer
            .seal(
                format!(
                    "{key}
{chain}"
                )
                .as_bytes(),
            )
            .unwrap();
        write_private(&dir.join("certs").join(format!("{host}.age")), &sealed).unwrap();
        let reloaded = CertStore::open(&dir, sealer).unwrap();
        assert!(reloaded.has("fresh.example.com"));
        assert!(
            !reloaded.has(host),
            "certificat expire ignore au chargement"
        );
        assert!(reloaded.get(host).is_none());

        // Expire apres le chargement : plus servi non plus.
        let held = reloaded.get("fresh.example.com").unwrap();
        let stale = Installed {
            key: held.key.clone(),
            not_after: unix_now() - 1,
            issuer: String::new(),
        };
        let mut next = HashMap::clone(&reloaded.certs.load());
        next.insert("fresh.example.com".to_string(), Arc::new(stale));
        reloaded.certs.store(Arc::new(next));
        assert!(!reloaded.has("fresh.example.com"));
        assert!(reloaded.serving("fresh.example.com").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Le compteur de suppressions ne garde que les hotes dont une emission
    /// est en cours ; il est purge des que la derniere est terminee.
    #[test]
    fn removals_purged() {
        let store = CertStore::memory();
        // Aucune emission en cours : rien a retenir.
        store.remove("a.example.com");
        assert_eq!(store.pending_removals(), 0);

        let (chain, key) = self_signed("b.example.com");
        let first = store.ticket("b.example.com");
        let second = store.ticket("b.example.com");
        store.remove("b.example.com");
        assert_eq!(store.pending_removals(), 1);
        assert!(store.install_ticket(&first, &chain, &key).is_err());
        drop(first);
        assert_eq!(store.pending_removals(), 1, "une emission reste en cours");
        drop(second);
        assert_eq!(store.pending_removals(), 0);

        // Installation sans accroc : rien ne subsiste non plus.
        store.install("b.example.com", &chain, &key).unwrap();
        assert_eq!(store.pending_removals(), 0);
    }
}
