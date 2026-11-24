//! Client du socket de pilotage du proxy du noeud (`/run/ciphera/proxy.sock`,
//! Unix seulement) : transmet chaque `ControlToAgent::ApplyRoutes` recu du
//! plan de controle, dans le codec `proto`.
//!
//! Seule la derniere table compte (`watch`) : une table arrivee pendant que
//! le proxy est injoignable remplace la precedente et part des la connexion.
//! Le client se reconnecte avec un delai croissant et renvoie la derniere
//! table a chaque connexion : un proxy qui redemarre (table vide) la
//! retrouve sans attendre le plan de controle.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use proto::{CertificateOutcome, ControlToAgent, ProxyReply, Route, codec};
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;
use tokio::sync::watch;

/// Chemin du socket de pilotage du proxy.
pub const DEFAULT_SOCKET: &str = "/run/ciphera/proxy.sock";
/// Delai maximal de la reponse du proxy a une table.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(10);
/// Premier delai avant de se reconnecter ; double a chaque echec.
pub const FIRST_RETRY: Duration = Duration::from_millis(500);
const MAX_RETRY: Duration = Duration::from_secs(10);

type Latest = Option<Arc<Vec<Route>>>;

/// Poignee du client : `apply` ne bloque jamais.
#[derive(Clone, Debug)]
pub struct ProxyHandle {
    tx: Arc<watch::Sender<Latest>>,
}

impl ProxyHandle {
    /// Remplace la table a transmettre au proxy.
    pub fn apply(&self, routes: Vec<Route>) {
        self.tx.send_replace(Some(Arc::new(routes)));
    }
}

/// Lance le client en tache de fond ; il s'arrete quand toutes les poignees
/// sont abandonnees.
pub fn spawn(path: PathBuf) -> ProxyHandle {
    let (tx, rx) = watch::channel(None);
    tokio::spawn(run(path, rx));
    ProxyHandle { tx: Arc::new(tx) }
}

enum Outcome {
    /// Plus aucune poignee : arret.
    Stop,
    /// Connexion perdue : se reconnecter.
    Lost,
}

async fn run(path: PathBuf, mut latest: watch::Receiver<Latest>) {
    let mut retry = FIRST_RETRY;
    let mut warned = false;
    loop {
        match UnixStream::connect(&path).await {
            Ok(stream) => {
                tracing::info!("proxy du noeud joint ({})", path.display());
                retry = FIRST_RETRY;
                warned = false;
                if let Outcome::Stop = session(stream, &mut latest).await {
                    return;
                }
                tracing::warn!("proxy du noeud : connexion perdue, reconnexion");
            }
            Err(e) => {
                if !warned {
                    tracing::warn!("proxy du noeud injoignable ({}) : {e}", path.display());
                    warned = true;
                }
            }
        }
        tokio::select! {
            () = tokio::time::sleep(retry) => {}
            // Nouvelle table : nouvel essai tout de suite ; plus de poignee :
            // arret.
            changed = latest.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
        retry = (retry * 2).min(MAX_RETRY);
    }
}

/// Une connexion : envoie la table courante (s'il y en a une), puis chaque
/// nouvelle table, jusqu'a la perte de la connexion.
async fn session(stream: UnixStream, latest: &mut watch::Receiver<Latest>) -> Outcome {
    let (mut reader, mut writer) = stream.into_split();
    let mut current = latest.borrow_and_update().clone();
    loop {
        if let Some(routes) = current.take() {
            let msg = ControlToAgent::ApplyRoutes(routes.as_ref().clone());
            if let Err(e) = codec::write_frame(&mut writer, &msg).await {
                tracing::warn!("proxy du noeud : envoi de la table : {e}");
                return Outcome::Lost;
            }
            match tokio::time::timeout(REPLY_TIMEOUT, codec::read_frame(&mut reader)).await {
                Ok(Ok(Some(ProxyReply::Applied { routes }))) => {
                    tracing::info!(routes, "proxy du noeud : table appliquee");
                }
                Ok(Ok(Some(ProxyReply::Rejected { reason }))) => {
                    tracing::error!("proxy du noeud : table refusee : {reason}");
                }
                // Jamais en reponse a une table : connexion incoherente.
                Ok(Ok(Some(ProxyReply::Certificate(_)))) => return Outcome::Lost,
                Ok(Ok(None)) | Ok(Err(_)) | Err(_) => return Outcome::Lost,
            }
        }
        // Attente de la table suivante. Le proxy n'envoie rien de lui-meme :
        // toute lecture (fin de flux comprise) signale une connexion perdue.
        let mut probe = [0u8; 1];
        tokio::select! {
            changed = latest.changed() => {
                if changed.is_err() {
                    return Outcome::Stop;
                }
                current = latest.borrow_and_update().clone();
            }
            _ = reader.read(&mut probe) => return Outcome::Lost,
        }
    }
}

/// Delai maximal d'une emission ACME par le proxy (le proxy s'arrete de
/// lui-meme a 180 s).
pub const CERTIFICATE_TIMEOUT: Duration = Duration::from_secs(200);
/// Emissions simultanees : le proxy n'accepte que 4 connexions de pilotage,
/// dont celle des tables.
const MAX_CERTIFICATE_REQUESTS: usize = 2;

/// Demande au proxy un certificat ACME pour `host`, sur une connexion a part
/// (une emission dure plusieurs secondes : la connexion des tables n'est
/// jamais bloquee). Toute erreur locale devient `Failed`.
pub async fn issue_certificate(path: &std::path::Path, host: &str) -> CertificateOutcome {
    certificate_request(
        path,
        ControlToAgent::IssueCertificate {
            host: host.to_string(),
        },
    )
    .await
}

/// Transmet au proxy un ordre de certificat (`IssueCertificate`,
/// `CheckCertificate` ou `RemoveCertificate`) sur une connexion a part et
/// renvoie son issue. Toute erreur locale devient `Failed`.
pub async fn certificate_request(
    path: &std::path::Path,
    msg: ControlToAgent,
) -> CertificateOutcome {
    static SLOTS: tokio::sync::Semaphore =
        tokio::sync::Semaphore::const_new(MAX_CERTIFICATE_REQUESTS);
    let failed = |reason: &str| CertificateOutcome::Failed {
        reason: reason.to_string(),
    };
    let Ok(_slot) = SLOTS.acquire().await else {
        return failed("emission impossible");
    };
    let mut stream = match UnixStream::connect(path).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("proxy du noeud injoignable ({}) : {e}", path.display());
            return failed("proxy du noeud injoignable");
        }
    };
    if !matches!(
        msg,
        ControlToAgent::IssueCertificate { .. }
            | ControlToAgent::CheckCertificate { .. }
            | ControlToAgent::RemoveCertificate { .. }
    ) {
        return failed("ordre de certificat attendu");
    }
    if codec::write_frame(&mut stream, &msg).await.is_err() {
        return failed("proxy du noeud injoignable");
    }
    match tokio::time::timeout(CERTIFICATE_TIMEOUT, codec::read_frame(&mut stream)).await {
        Ok(Ok(Some(ProxyReply::Certificate(outcome)))) => outcome,
        Ok(Ok(Some(ProxyReply::Rejected { reason }))) => CertificateOutcome::Failed { reason },
        Ok(Ok(Some(_))) => failed("reponse inattendue du proxy"),
        Ok(Ok(None)) | Ok(Err(_)) => failed("connexion au proxy perdue"),
        Err(_) => failed("delai de l'emission depasse"),
    }
}
