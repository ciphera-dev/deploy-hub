//! Renouvellement des certificats emis par la CA de l'instance (`ciphera
//! init`) :
//!
//! - lien des agents (`tls/link.{crt,key}`) et acces direct du panneau
//!   (`tls/panel.{crt,key}`), 730 jours : renouveles par le plan de controle
//!   lui-meme aux deux tiers de leur validite, au demarrage puis toutes les
//!   [`CHECK_EVERY`], avec rechargement a chaud ([`renew_own`]) ;
//! - certificat client de chaque agent, 365 jours : a la connexion d'un
//!   agent dont le certificat a passe les deux tiers, un nouveau certificat
//!   (meme cle, meme identifiant) lui est envoye sur le lien
//!   (`ControlToAgent::RenewedCertificate`, [`renew_agent`]) et son empreinte
//!   notee en `servers.next_cert_fingerprint` ; elle remplace l'ancienne a
//!   la premiere connexion qui la presente ;
//! - certificat deja expire (machine eteinte longtemps) : `ciphera admin
//!   renew-certs` ([`renew_all`]), voir `docs/operations/certificats.md` ;
//!   seul le certificat du serveur local enregistre, emis par la CA de
//!   l'instance, est re-signe.
//!
//! Certificat et cle d'une paire sont remplaces ensemble ([`replace_pair`] :
//! fichiers prepares, manifeste, renommages), et un remplacement
//! interrompu est repris avant toute lecture ([`recover_pairs`]).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use shared::ids::ServerId;
use sqlx::PgPool;

use super::servers;
use super::transport::{CA_CRT, LINK_CRT, LINK_KEY};

/// Cle privee de la CA (relative au dossier de configuration).
pub const CA_KEY: &str = "ca/ca.key";
/// Certificat et cle de l'acces direct du panneau.
pub const PANEL_CRT: &str = "tls/panel.crt";
pub const PANEL_KEY: &str = "tls/panel.key";
/// Certificat de l'agent dans son dossier (`/etc/ciphera-agent`).
pub const AGENT_CRT: &str = "agent.crt";
/// Intervalle de verification des certificats du plan de controle.
pub const CHECK_EVERY: Duration = Duration::from_secs(12 * 3600);

/// Certificats serveur du plan de controle : (certificat, cle).
pub const OWN_CERTS: [(&str, &str); 2] = [(LINK_CRT, LINK_KEY), (PANEL_CRT, PANEL_KEY)];

/// Secondes Unix.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// CA de l'instance (`ca/ca.crt`, `ca/ca.key`).
pub fn load_ca(config_dir: &Path) -> io::Result<pki::Ca> {
    let cert = fs::read_to_string(config_dir.join(CA_CRT))?;
    let key = zeroize::Zeroizing::new(fs::read_to_string(config_dir.join(CA_KEY))?);
    pki::Ca::load(&cert, &key).map_err(io::Error::other)
}

/// Fichier temporaire d'ecriture de `path` : `.<nom>.renew`, meme dossier.
fn staged(path: &Path) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.join(format!(".{name}.renew"))
}

/// Manifeste d'une paire certificat + cle en cours de remplacement :
/// `.<certificat>.pair`, a cote du certificat `crt` (relatif au dossier de
/// configuration). Sa presence signifie que les deux fichiers prepares
/// sont complets sur disque et doivent remplacer l'ancienne paire.
pub fn pair_manifest(config_dir: &Path, crt: &str) -> PathBuf {
    let path = config_dir.join(crt);
    let dir = path.parent().unwrap_or(config_dir).to_path_buf();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.join(format!(".{name}.pair"))
}

/// Ecrit `bytes` dans `tmp`, fichier neuf au proprietaire et aux droits de
/// `path` (0600 si `path` n'existe pas), donnees sur disque. `path` doit
/// etre un fichier ordinaire s'il existe (jamais a travers un lien).
fn stage(path: &Path, tmp: &Path, bytes: &[u8]) -> io::Result<()> {
    let previous = match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() || !m.is_file() => {
            return Err(io::Error::other(format!(
                "{} n'est pas un fichier ordinaire",
                path.display()
            )));
        }
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let _ = fs::remove_file(tmp);
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        opts.mode(previous.as_ref().map_or(0o600, |m| m.mode() & 0o777));
    }
    let mut file = opts.open(tmp)?;
    #[cfg(unix)]
    if let Some(m) = &previous {
        use std::os::unix::fs::MetadataExt;
        if let Err(e) = std::os::unix::fs::fchown(&file, Some(m.uid()), Some(m.gid())) {
            let _ = fs::remove_file(tmp);
            return Err(e);
        }
    }
    #[cfg(not(unix))]
    let _ = previous;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        let _ = fs::remove_file(tmp);
    }
    written
}

/// Dossier `dir` sur disque (renommages durables) ; sans effet hors Unix.
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// Remplace `path` par `bytes` : fichier temporaire du meme dossier, au
/// proprietaire et aux droits du fichier remplace (0600 s'il n'existe pas),
/// donnees sur disque, renommage, dossier sur disque. Jamais a travers un
/// lien.
pub fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = staged(path);
    stage(path, &tmp, bytes)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    sync_dir(dir)
}

/// Remplace la paire `crt` + `key` (relatifs au dossier de configuration,
/// meme dossier) sans jamais laisser un certificat et une cle qui ne vont
/// pas ensemble : les deux fichiers sont prepares sur disque, puis le
/// manifeste ([`pair_manifest`]) est ecrit (point de non-retour), puis le
/// certificat et la cle sont renommes et le manifeste retire. Une coupure
/// est reprise par [`recover_pair`] : avant le manifeste, l'ancienne paire
/// reste ; apres, la nouvelle est terminee.
pub fn replace_pair(
    config_dir: &Path,
    (crt, key): (&str, &str),
    cert_pem: &[u8],
    key_pem: &[u8],
) -> io::Result<()> {
    let (crt_path, key_path) = (config_dir.join(crt), config_dir.join(key));
    let dir = crt_path.parent().unwrap_or(config_dir);
    let manifest = pair_manifest(config_dir, crt);
    let (crt_tmp, key_tmp) = (staged(&crt_path), staged(&key_path));
    let prepared = stage(&key_path, &key_tmp, key_pem)
        .and_then(|()| stage(&crt_path, &crt_tmp, cert_pem))
        .and_then(|()| sync_dir(dir))
        .and_then(|()| stage(&crt_path, &manifest, format!("{crt}\n{key}\n").as_bytes()))
        .and_then(|()| sync_dir(dir));
    if let Err(e) = prepared {
        for tmp in [&manifest, &crt_tmp, &key_tmp] {
            let _ = fs::remove_file(tmp);
        }
        return Err(e);
    }
    finish_pair(
        dir,
        &manifest,
        [(&crt_tmp, &crt_path), (&key_tmp, &key_path)],
    )
}

/// Apres le manifeste : fichiers prepares encore presents renommes a leur
/// place (certificat puis cle), dossier sur disque, manifeste retire.
fn finish_pair(dir: &Path, manifest: &Path, moves: [(&Path, &Path); 2]) -> io::Result<()> {
    for (tmp, path) in moves {
        match fs::symlink_metadata(tmp) {
            Ok(m) if m.is_file() => fs::rename(tmp, path)?,
            Ok(_) => {
                return Err(io::Error::other(format!(
                    "{} n'est pas un fichier ordinaire",
                    tmp.display()
                )));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    sync_dir(dir)?;
    fs::remove_file(manifest)?;
    sync_dir(dir)
}

/// Reprise d'un remplacement de paire interrompu (voir [`replace_pair`]) :
/// manifeste present, la nouvelle paire est terminee ; absent, les fichiers
/// prepares sont supprimes et l'ancienne paire reste. Vrai si une paire a
/// ete terminee.
pub fn recover_pair(config_dir: &Path, (crt, key): (&str, &str)) -> io::Result<bool> {
    let (crt_path, key_path) = (config_dir.join(crt), config_dir.join(key));
    let dir = crt_path.parent().unwrap_or(config_dir);
    let manifest = pair_manifest(config_dir, crt);
    let (crt_tmp, key_tmp) = (staged(&crt_path), staged(&key_path));
    match fs::symlink_metadata(&manifest) {
        Ok(_) => {
            finish_pair(
                dir,
                &manifest,
                [(&crt_tmp, &crt_path), (&key_tmp, &key_path)],
            )?;
            Ok(true)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            for tmp in [&crt_tmp, &key_tmp] {
                match fs::remove_file(tmp) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

/// Reprise des remplacements interrompus des certificats du lien et du
/// panneau ([`OWN_CERTS`]), a faire avant de les lire. Un dossier sans
/// manifeste ni fichier prepare (installation d'avant ce mecanisme) est
/// laisse tel quel.
pub fn recover_pairs(config_dir: &Path) -> io::Result<()> {
    for pair in OWN_CERTS {
        if recover_pair(config_dir, pair)? {
            tracing::warn!(
                "certificat {} : remplacement interrompu termine a la reprise",
                pair.0
            );
        }
    }
    Ok(())
}

/// Renouvelle le certificat serveur `crt` (et sa cle `key`) s'il a passe
/// les deux tiers de sa validite, ou toujours avec `force`. Vrai si
/// renouvele. Certificat et cle sont remplaces ensemble ([`replace_pair`]).
pub fn renew_server(
    config_dir: &Path,
    ca: &pki::Ca,
    (crt, key): (&str, &str),
    now: i64,
    force: bool,
) -> io::Result<bool> {
    recover_pair(config_dir, (crt, key))?;
    let pem = fs::read_to_string(config_dir.join(crt))?;
    let due = pki::der_from_pem(&pem)
        .and_then(|der| pki::renewal_due(&der, now))
        .unwrap_or(true);
    if !due && !force {
        return Ok(false);
    }
    let mut bundle = ca.renew_server_cert(&pem).map_err(io::Error::other)?;
    let new_key = bundle.key_pem.take().unwrap_or_default();
    replace_pair(
        config_dir,
        (crt, key),
        bundle.cert_pem.as_bytes(),
        new_key.as_bytes(),
    )?;
    Ok(true)
}

/// Certificats du lien et du panneau a renouveler : renouveles. Renvoie les
/// certificats renouveles ; une erreur est journalisee et n'arrete rien.
pub fn renew_own(config_dir: &Path, ca: &pki::Ca, now: i64) -> Vec<&'static str> {
    let mut renewed = Vec::new();
    for pair in OWN_CERTS {
        match renew_server(config_dir, ca, pair, now, false) {
            Ok(true) => {
                tracing::info!("certificat {} renouvele", pair.0);
                renewed.push(pair.0);
            }
            Ok(false) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => tracing::error!("renouvellement de {} impossible : {e}", pair.0),
        }
    }
    renewed
}

/// Agent connecte avec le certificat `leaf` (empreinte `fingerprint`,
/// l'actuelle) : s'il a passe les deux tiers de sa validite, nouveau
/// certificat pour la meme cle, note comme suivant en base ; son PEM est a
/// envoyer a l'agent. `None` : pas encore du (ou deja note par une autre
/// connexion).
pub async fn renew_agent(
    db: &PgPool,
    ca: &pki::Ca,
    id: ServerId,
    fingerprint: &str,
    leaf: &[u8],
    now: i64,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    if pki::renewal_due(leaf, now) != Some(true) {
        return Ok(None);
    }
    let bundle = ca.renew_agent_cert(leaf)?;
    if !servers::set_next_cert(db, id, fingerprint, &bundle.fingerprint_sha256).await? {
        return Ok(None);
    }
    Ok(Some(bundle.cert_pem))
}

/// Resultat de `ciphera admin renew-certs`.
#[derive(Debug)]
pub struct RenewAll {
    pub server_certs: Vec<&'static str>,
    pub agent: Option<(ServerId, PathBuf)>,
}

/// Reprise manuelle (root) : certificats du lien et du panneau renouveles
/// quelle que soit leur date, et certificat de l'agent local (meme cle) ;
/// la nouvelle empreinte de l'agent est notee en base avant l'ecriture du
/// fichier, puis remplace l'ancienne a sa premiere connexion. Services a
/// redemarrer ensuite.
///
/// Le fichier de l'agent est modifiable par le compte de l'agent : son
/// certificat n'est signe que s'il est emis par la CA de l'instance et que
/// son empreinte est celle (actuelle ou suivante) du serveur local
/// enregistre et non revoque. Ce controle passe avant toute ecriture.
pub async fn renew_all(
    db: &PgPool,
    config_dir: &Path,
    agent_dir: &Path,
) -> Result<RenewAll, Box<dyn std::error::Error>> {
    let ca = load_ca(config_dir).map_err(|e| format!("CA de l'instance illisible : {e}"))?;
    recover_pairs(config_dir)
        .map_err(|e| format!("reprise d'un renouvellement interrompu : {e}"))?;
    let agent_crt = agent_dir.join(AGENT_CRT);
    let local_agent = match fs::read_to_string(&agent_crt) {
        Ok(pem) => Some(local_agent_cert(db, &ca, &pem).await?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{} : {e}", agent_crt.display()).into()),
    };
    let now = now_unix();
    let mut server_certs = Vec::new();
    for pair in OWN_CERTS {
        renew_server(config_dir, &ca, pair, now, true).map_err(|e| format!("{} : {e}", pair.0))?;
        server_certs.push(pair.0);
    }
    let agent = match local_agent {
        Some((id, fingerprint, der)) => {
            let bundle = ca.renew_agent_cert(&der)?;
            let noted = sqlx::query(
                "UPDATE servers SET next_cert_fingerprint = $3 \
                 WHERE id = $1 AND kind = 'local' AND revoked_at IS NULL \
                 AND (cert_fingerprint = $2 OR next_cert_fingerprint = $2)",
            )
            .bind(id)
            .bind(&fingerprint)
            .bind(&bundle.fingerprint_sha256)
            .execute(db)
            .await?
            .rows_affected();
            if noted == 0 {
                return Err(
                    format!("serveur {id} inconnu ou revoque : agent non renouvele").into(),
                );
            }
            replace_file(&agent_crt, bundle.cert_pem.as_bytes())
                .map_err(|e| format!("{} : {e}", agent_crt.display()))?;
            // Copie publique lue au demarrage du plan de controle.
            let local = config_dir.join(servers::LOCAL_AGENT_CERT);
            if local.exists() {
                replace_file(&local, bundle.cert_pem.as_bytes())
                    .map_err(|e| format!("{} : {e}", local.display()))?;
            }
            Some((id, agent_crt))
        }
        None => None,
    };
    Ok(RenewAll {
        server_certs,
        agent,
    })
}

/// Certificat PEM de l'agent local accepte pour `renew-certs` : emis par
/// la CA de l'instance, avec un identifiant d'agent, et d'empreinte
/// enregistree (actuelle ou suivante) pour le serveur local non revoque.
/// Renvoie l'identifiant, l'empreinte et le DER.
async fn local_agent_cert(
    db: &PgPool,
    ca: &pki::Ca,
    pem: &str,
) -> Result<(ServerId, String, Vec<u8>), Box<dyn std::error::Error>> {
    let der = pki::der_from_pem(pem).ok_or("certificat de l'agent illisible")?;
    let ca_der = pki::der_from_pem(ca.cert_pem()).ok_or("certificat de la CA illisible")?;
    if !pki::issued_by(&ca_der, &der) {
        return Err(
            "certificat de l'agent local non emis par la CA de l'instance : agent non renouvele"
                .into(),
        );
    }
    let id = pki::agent_id_from_cert(&der).ok_or("certificat de l'agent sans identifiant")?;
    let fingerprint = pki::fingerprint_sha256(&der);
    let revoked: Option<bool> = sqlx::query_scalar(
        "SELECT revoked_at IS NOT NULL FROM servers \
         WHERE id = $1 AND kind = 'local' \
         AND (cert_fingerprint = $2 OR next_cert_fingerprint = $2)",
    )
    .bind(id)
    .bind(&fingerprint)
    .fetch_optional(db)
    .await?;
    match revoked {
        None => Err(format!(
            "certificat de l'agent ({fingerprint}) different de celui enregistre pour le \
             serveur local : agent non renouvele"
        )
        .into()),
        Some(true) => Err(format!("serveur {id} revoque : agent non renouvele").into()),
        Some(false) => Ok((id, fingerprint, der)),
    }
}
