//! Domaines des projets : validation des noms, hotes uniques sur l'instance
//! (et reserves au panneau), domaine genere `<projet>.<ip>.sslip.io`,
//! verification DNS, domaine principal et redirection 308, table des routes
//! du proxy de chaque noeud.
//!
//! Un domaine n'est en ligne (route envoyee au proxy) qu'une fois son DNS
//! verifie et, s'il sert un service, quand ce service a un port interne. Le
//! certificat (ACME HTTP-01) est emis et garde par le proxy du noeud ; seuls
//! son etat et sa fin de validite sont ici (`acme`).
//!
//! Le domaine du panneau (`panel`) est une ligne sans projet : jamais
//! listee ni modifiable par les routes des projets, son hote leur est reserve.

pub mod acme;
pub mod dns;
pub mod panel;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use api_types::domains::{CertStatus, DnsStatus, Domain, NewDomain, ProjectDomains, UpdateDomain};
use proto::{ControlToAgent, Route, TlsMode};
use shared::config::Config;
use shared::ids::ServiceId;
use sqlx::PgConnection;
use uuid::Uuid;

pub use dns::{DnsResolver, SystemResolver};

use crate::domain::projects::store::PRODUCTION;
use crate::state::AppState;

const TS: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'";

/// Longueur maximale d'un nom d'hote (sans point final).
pub const MAX_HOST: usize = 253;
const MAX_LABEL: usize = 63;
/// Longueur maximale d'un prefixe de chemin.
pub const MAX_PATH_PREFIX: usize = 255;
/// Suffixe des domaines generes (DNS joker public : `<x>.<ip>.sslip.io`
/// resout vers `<ip>`).
pub const GENERATED_SUFFIX: &str = "sslip.io";

/// Zones DNS jokers publiques (`<x>.<ip>.<zone>` resout vers `<ip>`) :
/// reservees au domaine genere, jamais acceptees comme domaine
/// personnalise. Leurs limites Let's Encrypt sont partagees par tous leurs
/// utilisateurs, et un nom d'un autre projet pourrait y etre capte.
pub const WILDCARD_ZONES: [&str; 2] = [GENERATED_SUFFIX, "nip.io"];

/// L'hote est-il une zone joker publique ou sous l'une d'elles ?
pub fn in_wildcard_zone(host: &str) -> bool {
    WILDCARD_ZONES
        .iter()
        .any(|z| host == *z || host.strip_suffix(z).is_some_and(|rest| rest.ends_with('.')))
}

#[derive(Debug)]
pub enum DomainError {
    /// Valeur refusee (422).
    Invalid(String),
    /// Port interne ou healthcheck du service a changer sans `Projects:Write` (403).
    Forbidden,
    /// Port interne du service manquant (422).
    PortRequired,
    /// IP publique du noeud inconnue (422).
    NoNodeIp,
    /// Hote deja utilise sur l'instance (409).
    HostTaken,
    /// Hote du panneau (409).
    Reserved,
    /// Operation impossible dans l'etat courant (409).
    Conflict(String),
    NotFound(&'static str),
    Db(sqlx::Error),
}

impl From<sqlx::Error> for DomainError {
    fn from(e: sqlx::Error) -> Self {
        DomainError::Db(e)
    }
}

pub type Result<T> = std::result::Result<T, DomainError>;

/// Message du port interne manquant.
pub const PORT_REQUIRED: &str = "Port interne du service requis avant de mettre un domaine en ligne : indiquez le port sur lequel l'application écoute dans son conteneur.";

/// Nom d'hote normalise (minuscules, sans point final) : FQDN d'au moins
/// deux etiquettes, lettres ASCII, chiffres et tirets (IDN en punycode
/// `xn--`), domaine de premier niveau non numerique. Refuse adresses IP,
/// jokers, ports, schemas et chemins.
pub fn normalize_host(input: &str) -> std::result::Result<String, &'static str> {
    const INVALID: &str = "Nom de domaine invalide : un nom complet comme app.exemple.com, sans adresse IP, joker, port ni chemin (IDN en punycode xn--).";
    let host = input.trim().to_ascii_lowercase();
    let host = host.strip_suffix('.').unwrap_or(&host).to_string();
    if host.is_empty() || host.len() > MAX_HOST || host.parse::<IpAddr>().is_ok() {
        return Err(INVALID);
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return Err(INVALID);
    }
    let labels_ok = labels.iter().all(|l| {
        !l.is_empty()
            && l.len() <= MAX_LABEL
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    });
    let tld = labels[labels.len() - 1];
    if !labels_ok || tld.bytes().all(|b| b.is_ascii_digit()) {
        return Err(INVALID);
    }
    Ok(host)
}

/// Prefixe de chemin accepte par le proxy : `/` puis lettres, chiffres et
/// `. _ ~ / -`, sans segment `.` ou `..` ni `//`.
pub fn valid_path_prefix(p: &str) -> bool {
    p.starts_with('/')
        && p.len() <= MAX_PATH_PREFIX
        && p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'~' | b'-'))
        && !p.contains("//")
        && !p.split('/').any(|s| s == "." || s == "..")
}

/// Chemin de healthcheck (meme regle que la base : `/` puis ASCII visible).
pub fn valid_healthcheck_path(p: &str) -> bool {
    p.starts_with('/') && p.len() <= 1024 && p.bytes().all(|b| b.is_ascii_graphic())
}

/// Domaine genere du projet : `<projet>.<ip>.sslip.io` (IPv6 : `:` -> `-`).
pub fn generated_host(project: &str, ip: IpAddr) -> Option<String> {
    let label = project.trim_end_matches('-');
    let ip = match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => v6.to_string().replace(':', "-"),
    };
    normalize_host(&format!("{label}.{ip}.{GENERATED_SUFFIX}")).ok()
}

/// Hote reserve au panneau (URL publique, RP WebAuthn, alias).
pub fn reserved(cfg: &Config, host: &str) -> bool {
    cfg.reserved_hosts().iter().any(|h| h == host)
}

/// Hote refuse a un projet : reserve au panneau, ou nom par lequel la
/// requete courante joint le panneau (`current_host`, normalise).
pub fn refused(cfg: &Config, current_host: Option<&str>, host: &str) -> bool {
    reserved(cfg, host) || current_host == Some(host)
}

fn dns_status(s: &str) -> DnsStatus {
    match s {
        "ok" => DnsStatus::Ok,
        "error" => DnsStatus::Error,
        _ => DnsStatus::Pending,
    }
}

fn cert_status(s: &str) -> CertStatus {
    match s {
        "issuing" => CertStatus::Issuing,
        "active" => CertStatus::Active,
        "error" => CertStatus::Error,
        _ => CertStatus::None,
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    host: String,
    path_prefix: String,
    generated: bool,
    is_primary: bool,
    redirect: bool,
    service_id: Option<Uuid>,
    service_name: Option<String>,
    internal_port: Option<i32>,
    dns_status: String,
    dns_error: Option<String>,
    cert_status: String,
    cert_expires_at: Option<String>,
    cert_issuer: Option<String>,
    cert_error: Option<String>,
    created_at: String,
    primary_host: Option<String>,
}

/// Erreur affichee quand la chaine d'emission d'un domaine en ligne s'est
/// arretee (tentatives epuisees) : jamais de renouvellement perdu en silence.
const INTERRUPTED: &str =
    "'Émission du certificat interrompue : cliquez sur « Vérifier » pour la relancer.'";

fn select() -> String {
    format!(
        "SELECT d.id, d.project_id, d.environment_id, d.host, d.path_prefix, d.generated, \
         d.is_primary, d.redirect, d.service_id, s.name AS service_name, s.internal_port, \
         d.dns_status, d.dns_error, d.cert_status, \
         to_char(d.cert_expires_at AT TIME ZONE 'UTC', {TS}) AS cert_expires_at, \
         d.cert_issuer, \
         CASE WHEN NOT d.generated AND d.dns_status = 'ok' AND NOT EXISTS \
           (SELECT 1 FROM jobs j WHERE j.dedupe_key = 'acme:' || d.id::text \
            AND j.status IN ('queued', 'running')) \
         THEN {INTERRUPTED} ELSE d.cert_error END AS cert_error, \
         to_char(d.created_at AT TIME ZONE 'UTC', {TS}) AS created_at, \
         (SELECT p.host FROM domains p WHERE p.environment_id = d.environment_id \
            AND p.is_primary) AS primary_host \
         FROM domains d LEFT JOIN services s ON s.id = d.service_id"
    )
}

/// Domaine et ce qui le rattache.
pub struct Found {
    pub domain: Domain,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub service_id: Option<Uuid>,
}

impl From<Row> for Found {
    fn from(r: Row) -> Self {
        let redirect_to = if r.redirect { r.primary_host } else { None };
        Found {
            project_id: r.project_id,
            environment_id: r.environment_id,
            service_id: r.service_id,
            domain: Domain {
                id: r.id.to_string(),
                host: r.host,
                path_prefix: r.path_prefix,
                generated: r.generated,
                primary: r.is_primary,
                redirect: r.redirect,
                redirect_to,
                service_id: r.service_id.map(|s| s.to_string()),
                service_name: r.service_name,
                internal_port: r.internal_port.and_then(|p| u16::try_from(p).ok()),
                dns_status: dns_status(&r.dns_status),
                dns_error: r.dns_error,
                cert_status: cert_status(&r.cert_status),
                cert_expires_at: r.cert_expires_at,
                cert_issuer: r.cert_issuer,
                cert_error: r.cert_error,
                created_at: r.created_at,
            },
        }
    }
}

pub async fn get(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<Found>> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} WHERE d.id = $1 AND NOT d.panel",
        select()
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(Found::from))
}

/// Domaines du projet (`None` : projet introuvable).
pub async fn list(
    conn: &mut PgConnection,
    cfg: &Config,
    project: Uuid,
) -> sqlx::Result<Option<ProjectDomains>> {
    let generated: Option<bool> =
        sqlx::query_scalar("SELECT generated_domain FROM projects WHERE id = $1")
            .bind(project)
            .fetch_optional(&mut *conn)
            .await?;
    let Some(generated_domain) = generated else {
        return Ok(None);
    };
    // Requete fixe : `select()` ne contient aucune donnee.
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{} WHERE d.project_id = $1 ORDER BY d.generated DESC, d.created_at, d.host",
        select()
    )))
    .bind(project)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Some(ProjectDomains {
        node_ip: cfg.public_ip.map(|ip| ip.to_string()),
        // Seconde cible (AAAA) quand `node_ip` est une IPv4.
        node_ip6: cfg
            .public_ip
            .filter(IpAddr::is_ipv4)
            .and(cfg.public_ip6)
            .map(|ip| ip.to_string()),
        generated_domain,
        domains: rows.into_iter().map(|r| Found::from(r).domain).collect(),
    }))
}

/// Domaine principal de la production du projet (sinon le domaine genere
/// en ligne), pour l'apercu.
pub async fn primary_host(conn: &mut PgConnection, project: Uuid) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar(
        "SELECT d.host FROM domains d JOIN environments e ON e.id = d.environment_id \
         WHERE d.project_id = $1 AND e.name = $2 AND (d.is_primary OR (d.generated AND d.dns_status = 'ok')) \
         ORDER BY d.is_primary DESC LIMIT 1",
    )
    .bind(project)
    .bind(PRODUCTION)
    .fetch_optional(conn)
    .await
}

/// Environnement de production du projet, verrouille (ecritures de domaines
/// serialisees par environnement : domaine principal unique).
async fn production(conn: &mut PgConnection, project: Uuid) -> Result<Uuid> {
    sqlx::query_scalar("SELECT id FROM environments WHERE project_id = $1 AND name = $2 FOR UPDATE")
        .bind(project)
        .bind(PRODUCTION)
        .fetch_optional(conn)
        .await?
        .ok_or(DomainError::NotFound("Projet introuvable."))
}

async fn lock_environment(conn: &mut PgConnection, environment: Uuid) -> sqlx::Result<()> {
    sqlx::query("SELECT 1 FROM environments WHERE id = $1 FOR UPDATE")
        .bind(environment)
        .execute(conn)
        .await
        .map(|_| ())
}

/// Service servi : celui demande (de l'environnement), sinon le plus ancien
/// de l'environnement.
async fn service(
    conn: &mut PgConnection,
    environment: Uuid,
    wanted: Option<&str>,
) -> Result<(Uuid, Option<i32>)> {
    let found: Option<(Uuid, Option<i32>)> = match wanted {
        Some(id) => {
            let id =
                Uuid::parse_str(id).map_err(|_| DomainError::NotFound("Service introuvable."))?;
            sqlx::query_as(
                "SELECT id, internal_port FROM services WHERE id = $1 AND environment_id = $2",
            )
            .bind(id)
            .bind(environment)
            .fetch_optional(conn)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT id, internal_port FROM services WHERE environment_id = $1 \
                 ORDER BY created_at, name LIMIT 1",
            )
            .bind(environment)
            .fetch_optional(conn)
            .await?
        }
    };
    found.ok_or(DomainError::NotFound("Service introuvable."))
}

/// Fixe le port interne (et le healthcheck) du service. Le port vaut pour
/// les prochains deploiements.
async fn set_service_port(
    conn: &mut PgConnection,
    service: Uuid,
    port: Option<u16>,
    healthcheck: Option<&str>,
    may_edit: bool,
) -> Result<()> {
    if !may_edit {
        let (current_port, current_check): (Option<i32>, Option<String>) =
            sqlx::query_as("SELECT internal_port, healthcheck_path FROM services WHERE id = $1")
                .bind(service)
                .fetch_one(&mut *conn)
                .await?;
        let port_changes = port.is_some_and(|p| current_port != Some(i32::from(p)));
        let check_changes = healthcheck.is_some_and(|h| current_check.as_deref() != Some(h));
        if port_changes || check_changes {
            return Err(DomainError::Forbidden);
        }
    }
    if let Some(p) = healthcheck
        && !valid_healthcheck_path(p)
    {
        return Err(DomainError::Invalid(
            "Chemin de healthcheck invalide : il commence par / (1024 caractères au plus).".into(),
        ));
    }
    if port == Some(0) {
        return Err(DomainError::Invalid(
            "Port interne invalide (1 à 65535).".into(),
        ));
    }
    sqlx::query(
        "UPDATE services SET internal_port = COALESCE($2, internal_port), \
         healthcheck_path = COALESCE($3, healthcheck_path) WHERE id = $1",
    )
    .bind(service)
    .bind(port.map(i32::from))
    .bind(healthcheck)
    .execute(conn)
    .await?;
    Ok(())
}

struct Insert<'a> {
    project: Uuid,
    environment: Uuid,
    service: Option<Uuid>,
    host: &'a str,
    path_prefix: &'a str,
    generated: bool,
    primary: bool,
    redirect: bool,
    dns_ok: bool,
    actor: Option<Uuid>,
}

/// Insere un domaine ; `None` si l'hote est deja pris (aucune erreur : la
/// transaction reste utilisable).
async fn insert(conn: &mut PgConnection, d: Insert<'_>) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar(
        "INSERT INTO domains (id, project_id, environment_id, service_id, host, path_prefix, \
         generated, is_primary, redirect, dns_status, dns_checked_at, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, CASE WHEN $11 THEN now() END, $12) \
         ON CONFLICT DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(d.project)
    .bind(d.environment)
    .bind(d.service)
    .bind(d.host)
    .bind(d.path_prefix)
    .bind(d.generated)
    .bind(d.primary)
    .bind(d.redirect)
    .bind(if d.dns_ok { "ok" } else { "pending" })
    .bind(d.dns_ok)
    .bind(d.actor)
    .fetch_optional(conn)
    .await
}

async fn has_primary(conn: &mut PgConnection, environment: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM domains WHERE environment_id = $1 AND is_primary)",
    )
    .bind(environment)
    .fetch_one(conn)
    .await
}

/// Domaines ajoutes par l'utilisateur au plus par projet (hors domaine genere).
pub const MAX_PER_PROJECT: i64 = 20;
/// Domaines ajoutes par l'utilisateur au plus sur l'instance.
pub const MAX_PER_INSTANCE: i64 = 100;

/// Refuse l'ajout de `adding` hotes au-dela des plafonds. Un verrou de
/// transaction serialise les ajouts : deux requetes simultanees ne depassent
/// pas le plafond de l'instance.
async fn check_quota(conn: &mut PgConnection, project: Uuid, adding: i64) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('ciphera.domain_quota'))")
        .execute(&mut *conn)
        .await?;
    let (in_project, in_instance): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE project_id = $1), count(*) \
         FROM domains WHERE NOT generated AND NOT panel",
    )
    .bind(project)
    .fetch_one(conn)
    .await?;
    if in_project + adding > MAX_PER_PROJECT {
        return Err(DomainError::Invalid(format!(
            "{MAX_PER_PROJECT} domaines au plus par projet : supprimez-en un avant d'en ajouter."
        )));
    }
    if in_instance + adding > MAX_PER_INSTANCE {
        return Err(DomainError::Invalid(format!(
            "{MAX_PER_INSTANCE} domaines au plus sur l'instance."
        )));
    }
    Ok(())
}

/// Ajoute un domaine (DNS a verifier). Le premier domaine servant un
/// service devient principal ; `www` ajoute `www.<hote>` en redirection.
/// `current_host` : nom par lequel la requete joint le panneau, refuse
/// comme les hotes reserves.
pub async fn create(
    conn: &mut PgConnection,
    cfg: &Config,
    current_host: Option<&str>,
    project: Uuid,
    input: &NewDomain,
    actor: Uuid,
    may_edit_service: bool,
) -> Result<Uuid> {
    let host = normalize_host(&input.host).map_err(|m| DomainError::Invalid(m.into()))?;
    if in_wildcard_zone(&host) {
        return Err(DomainError::Invalid(
            "Les noms sslip.io et nip.io sont réservés au domaine généré : utilisez un domaine que vous possédez.".into(),
        ));
    }
    let path_prefix = input.path_prefix.as_deref().unwrap_or("/");
    if !valid_path_prefix(path_prefix) {
        return Err(DomainError::Invalid(
            "Préfixe de chemin invalide : / puis lettres, chiffres, « . », « _ », « ~ », « - » et « / ».".into(),
        ));
    }
    let www = if input.www {
        if host.starts_with("www.") {
            return Err(DomainError::Invalid(
                "L'option www s'applique à un domaine apex (sans www.).".into(),
            ));
        }
        Some(normalize_host(&format!("www.{host}")).map_err(|m| DomainError::Invalid(m.into()))?)
    } else {
        None
    };
    let hosts: Vec<String> = std::iter::once(&host)
        .chain(www.as_ref())
        .cloned()
        .collect();
    if hosts.iter().any(|h| refused(cfg, current_host, h)) || panel_host(conn, &hosts).await? {
        return Err(DomainError::Reserved);
    }
    check_quota(conn, project, hosts.len() as i64).await?;
    let environment = production(conn, project).await?;
    let (service, current_port) = service(conn, environment, input.service_id.as_deref()).await?;
    if !input.redirect {
        if input.internal_port.is_none() && current_port.is_none() {
            return Err(DomainError::PortRequired);
        }
        set_service_port(
            conn,
            service,
            input.internal_port,
            input.healthcheck_path.as_deref(),
            may_edit_service,
        )
        .await?;
    }
    let primary_exists = has_primary(conn, environment).await?;
    if input.redirect && !primary_exists {
        return Err(DomainError::Conflict(
            "Aucun domaine principal vers lequel rediriger : ajoutez d'abord le domaine principal."
                .into(),
        ));
    }
    let id = insert(
        conn,
        Insert {
            project,
            environment,
            service: Some(service),
            host: &host,
            path_prefix,
            generated: false,
            primary: !input.redirect && !primary_exists,
            redirect: input.redirect,
            dns_ok: false,
            actor: Some(actor),
        },
    )
    .await?
    .ok_or(DomainError::HostTaken)?;
    if let Some(www) = www {
        insert(
            conn,
            Insert {
                project,
                environment,
                service: Some(service),
                host: &www,
                path_prefix: "/",
                generated: false,
                primary: false,
                redirect: true,
                dns_ok: false,
                actor: Some(actor),
            },
        )
        .await?
        .ok_or(DomainError::HostTaken)?;
    }
    Ok(id)
}

/// Un de ces hotes est-il le domaine du panneau ?
async fn panel_host(conn: &mut PgConnection, hosts: &[String]) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM domains WHERE panel AND host = ANY($1))")
        .bind(hosts)
        .fetch_one(conn)
        .await
}

/// Domaine genere du projet (si active et l'IP du noeud connue). Rien si
/// l'hote est deja pris.
///
/// `immediate` : creation du projet, premiere emission tout de suite. Une
/// recreation (reactivation, renommage) reporte la premiere tentative d'un
/// jour : desactiver puis reactiver ne contourne pas la limite d'une
/// tentative par jour face aux limites de debit de l'autorite.
pub async fn ensure_generated(
    conn: &mut PgConnection,
    cfg: &Config,
    project: Uuid,
    immediate: bool,
) -> sqlx::Result<()> {
    let Some(ip) = cfg.public_ip else {
        return Ok(());
    };
    let row: Option<(String, bool, Uuid)> = sqlx::query_as(
        "SELECT p.name, p.generated_domain, e.id FROM projects p \
         JOIN environments e ON e.project_id = p.id AND e.name = $2 WHERE p.id = $1",
    )
    .bind(project)
    .bind(PRODUCTION)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((name, enabled, environment)) = row else {
        return Ok(());
    };
    let Some(host) = generated_host(&name, ip) else {
        return Ok(());
    };
    if reserved(cfg, &host) {
        tracing::warn!(%project, "domaine genere reserve au panneau : non cree");
        return Ok(());
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM domains WHERE project_id = $1 AND generated)",
    )
    .bind(project)
    .fetch_one(&mut *conn)
    .await?;
    if !enabled || exists {
        return Ok(());
    }
    let service: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM services WHERE environment_id = $1 ORDER BY created_at, name LIMIT 1",
    )
    .bind(environment)
    .fetch_optional(&mut *conn)
    .await?;
    let inserted = insert(
        conn,
        Insert {
            project,
            environment,
            service,
            host: &host,
            path_prefix: "/",
            generated: true,
            primary: false,
            redirect: false,
            // Resout vers l'IP du noeud par construction.
            dns_ok: true,
            actor: None,
        },
    )
    .await?;
    match inserted {
        // Une tentative d'emission des la creation : en cas d'echec (limite
        // de debit, proxy injoignable), le domaine reste servi en HTTP et la
        // tache reessaie une fois par jour (`acme`). Aucun certificat
        // existant : l'heure n'entre pas en compte.
        Some(id) => {
            acme::request(conn, id, 0).await?;
            if !immediate {
                acme::postpone(conn, id, acme::GENERATED_RETRY_SECS).await?;
            }
        }
        None => tracing::warn!(%project, "domaine genere deja pris : non cree"),
    }
    Ok(())
}

/// Supprime le domaine genere du projet ; renvoie son hote, dont le
/// certificat est a retirer du proxy une fois la transaction validee
/// ([`remove_certificates`]).
async fn delete_generated(conn: &mut PgConnection, project: Uuid) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar("DELETE FROM domains WHERE project_id = $1 AND generated RETURNING host")
        .bind(project)
        .fetch_all(conn)
        .await
}

/// Recree le domaine genere (projet renomme) ; renvoie les hotes supprimes,
/// dont les certificats sont a retirer.
pub async fn regenerate(
    conn: &mut PgConnection,
    cfg: &Config,
    project: Uuid,
) -> sqlx::Result<Vec<String>> {
    let mut removed = delete_generated(conn, project).await?;
    ensure_generated(conn, cfg, project, false).await?;
    // Meme hote recree (nom inchange a la casse pres) : son certificat reste
    // sur le proxy, que la nouvelle ligne reprendra.
    let kept: Vec<String> =
        sqlx::query_scalar("SELECT host FROM domains WHERE project_id = $1 AND generated")
            .bind(project)
            .fetch_all(&mut *conn)
            .await?;
    removed.retain(|h| !kept.contains(h));
    Ok(removed)
}

/// Active ou desactive le domaine genere du projet ; renvoie les hotes
/// supprimes, dont les certificats sont a retirer.
pub async fn set_generated(
    conn: &mut PgConnection,
    cfg: &Config,
    project: Uuid,
    enabled: bool,
) -> Result<Vec<String>> {
    production(conn, project).await?;
    if enabled && cfg.public_ip.is_none() {
        return Err(DomainError::NoNodeIp);
    }
    sqlx::query("UPDATE projects SET generated_domain = $2 WHERE id = $1")
        .bind(project)
        .bind(enabled)
        .execute(&mut *conn)
        .await?;
    if enabled {
        ensure_generated(conn, cfg, project, false).await?;
        Ok(Vec::new())
    } else {
        Ok(delete_generated(conn, project).await?)
    }
}

/// Le domaine (verrouille avec son environnement).
async fn locked(conn: &mut PgConnection, id: Uuid) -> Result<Found> {
    let environment: Uuid =
        sqlx::query_scalar("SELECT environment_id FROM domains WHERE id = $1 AND NOT panel")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?
            .ok_or(DomainError::NotFound("Domaine introuvable."))?;
    lock_environment(conn, environment).await?;
    get(conn, id)
        .await?
        .ok_or(DomainError::NotFound("Domaine introuvable."))
}

/// Modifie le prefixe, le service servi et son port ou son healthcheck.
pub async fn update(
    conn: &mut PgConnection,
    id: Uuid,
    input: &UpdateDomain,
    may_edit_service: bool,
) -> Result<()> {
    let found = locked(conn, id).await?;
    if let Some(p) = &input.path_prefix
        && !valid_path_prefix(p)
    {
        return Err(DomainError::Invalid(
            "Préfixe de chemin invalide : / puis lettres, chiffres, « . », « _ », « ~ », « - » et « / ».".into(),
        ));
    }
    let (service, current_port) = match (&input.service_id, found.service_id) {
        (Some(wanted), _) => service(conn, found.environment_id, Some(wanted)).await?,
        (None, Some(current)) => {
            service(conn, found.environment_id, Some(&current.to_string())).await?
        }
        (None, None) => service(conn, found.environment_id, None).await?,
    };
    if !found.domain.redirect && input.internal_port.is_none() && current_port.is_none() {
        return Err(DomainError::PortRequired);
    }
    set_service_port(
        conn,
        service,
        input.internal_port,
        input.healthcheck_path.as_deref(),
        may_edit_service,
    )
    .await?;
    sqlx::query(
        "UPDATE domains SET path_prefix = COALESCE($2, path_prefix), service_id = $3, \
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(input.path_prefix.as_deref())
    .bind(service)
    .execute(conn)
    .await?;
    Ok(())
}

/// Supprime un domaine personnalise. Le principal supprime, le plus ancien
/// domaine servant un service le remplace.
pub async fn delete(conn: &mut PgConnection, id: Uuid) -> Result<Found> {
    let found = locked(conn, id).await?;
    if found.domain.generated {
        return Err(DomainError::Conflict(
            "Le domaine généré ne se supprime pas : désactivez-le.".into(),
        ));
    }
    sqlx::query("DELETE FROM domains WHERE id = $1")
        .bind(id)
        .execute(&mut *conn)
        .await?;
    if found.domain.primary {
        sqlx::query(
            "UPDATE domains SET is_primary = true, updated_at = now() WHERE id = ( \
               SELECT id FROM domains WHERE environment_id = $1 AND NOT redirect AND NOT generated \
               ORDER BY created_at LIMIT 1)",
        )
        .bind(found.environment_id)
        .execute(conn)
        .await?;
    }
    Ok(found)
}

/// Fait du domaine le principal de son environnement.
pub async fn set_primary(conn: &mut PgConnection, id: Uuid) -> Result<Found> {
    let found = locked(conn, id).await?;
    if found.domain.redirect {
        return Err(DomainError::Conflict(
            "Un domaine redirigé ne peut pas être principal : retirez d'abord la redirection."
                .into(),
        ));
    }
    if found.domain.generated {
        return Err(DomainError::Conflict(
            "Le domaine généré ne peut pas être principal : utilisez un domaine à vous.".into(),
        ));
    }
    sqlx::query("UPDATE domains SET is_primary = false, updated_at = now() WHERE environment_id = $1 AND is_primary AND id <> $2")
        .bind(found.environment_id)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE domains SET is_primary = true, updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(found)
}

/// Redirection 308 vers le principal (`true`) ou service de nouveau servi.
pub async fn set_redirect(conn: &mut PgConnection, id: Uuid, enabled: bool) -> Result<()> {
    let found = locked(conn, id).await?;
    if enabled {
        if found.domain.primary {
            return Err(DomainError::Conflict(
                "Le domaine principal ne peut pas se rediriger : choisissez d'abord un autre domaine principal."
                    .into(),
            ));
        }
        if !has_primary(conn, found.environment_id).await? {
            return Err(DomainError::Conflict(
                "Aucun domaine principal vers lequel rediriger.".into(),
            ));
        }
    } else {
        let port: Option<i32> = match found.service_id {
            Some(s) => sqlx::query_scalar("SELECT internal_port FROM services WHERE id = $1")
                .bind(s)
                .fetch_optional(&mut *conn)
                .await?
                .flatten(),
            None => None,
        };
        if port.is_none() {
            return Err(DomainError::PortRequired);
        }
    }
    sqlx::query("UPDATE domains SET redirect = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(enabled)
        .execute(conn)
        .await?;
    Ok(())
}

/// Le domaine peut-il etre mis en ligne (port interne pour un service) ?
pub async fn ready(conn: &mut PgConnection, found: &Found) -> sqlx::Result<bool> {
    if found.domain.redirect {
        return Ok(true);
    }
    let Some(service) = found.service_id else {
        return Ok(false);
    };
    let port: Option<i32> = sqlx::query_scalar("SELECT internal_port FROM services WHERE id = $1")
        .bind(service)
        .fetch_optional(conn)
        .await?
        .flatten();
    Ok(port.is_some())
}

/// Note le resultat de la verification DNS (`error` : raison).
pub async fn record_dns(
    conn: &mut PgConnection,
    id: Uuid,
    error: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE domains SET dns_status = CASE WHEN $2::text IS NULL THEN 'ok' ELSE 'error' END, \
         dns_error = $2, dns_checked_at = now(), updated_at = now(),          dns_failures = CASE WHEN $2::text IS NULL THEN 0 ELSE dns_failures END WHERE id = $1",
    )
    .bind(id)
    .bind(error)
    .execute(conn)
    .await
    .map(|_| ())
}

#[derive(sqlx::FromRow)]
struct RouteRow {
    host: String,
    path_prefix: String,
    redirect: bool,
    service_id: Option<Uuid>,
    internal_port: Option<i32>,
    primary_host: Option<String>,
}

/// Table complete des routes du proxy du serveur `server` : domaines au DNS
/// verifie des environnements deployes sur ce serveur (ou pas encore
/// deployes). Un domaine servant un service exige son port interne ; une
/// redirection exige un principal en ligne (DNS verifie). Tous les domaines sont en HTTPS (ACME) :
/// le domaine genere reste servi en HTTP tant que son certificat manque. Le domaine du panneau, sur le serveur
/// local seulement, mene a l'ecoute HTTP locale du panneau une fois son
/// certificat actif (DNS verifie, ou deja confirme : une erreur DNS
/// passagere ne coupe pas le panneau).
pub async fn route_table(
    conn: &mut PgConnection,
    cfg: &Config,
    server: Uuid,
) -> sqlx::Result<Vec<Route>> {
    let rows: Vec<RouteRow> = sqlx::query_as(
        "SELECT d.host, d.path_prefix, d.redirect, d.service_id, s.internal_port, \
           (SELECT p.host FROM domains p WHERE p.environment_id = d.environment_id \
              AND p.is_primary AND p.id <> d.id \
              AND p.dns_status = 'ok' AND NOT p.generated) AS primary_host \
         FROM domains d \
         LEFT JOIN services s ON s.id = d.service_id \
         LEFT JOIN LATERAL (SELECT x.server_id FROM deployments x \
              WHERE x.environment_id = d.environment_id \
              ORDER BY x.created_at DESC LIMIT 1) last ON true \
         WHERE NOT d.panel AND d.dns_status = 'ok' AND (last.server_id = $1 OR last.server_id IS NULL) \
         ORDER BY d.host",
    )
    .bind(server)
    .fetch_all(&mut *conn)
    .await?;
    let panel: Option<(String, bool, bool)> = if local_server(conn).await? == Some(server) {
        sqlx::query_as(
            "SELECT host, cert_status = 'active' OR panel_confirmed_at IS NOT NULL, \
             panel_confirmed_at IS NOT NULL FROM domains \
             WHERE panel AND (dns_status = 'ok' OR panel_confirmed_at IS NOT NULL)",
        )
        .fetch_optional(&mut *conn)
        .await?
    } else {
        None
    };
    // Avant le certificat, la route n'a pas d'amont : elle permet l'emission
    // (le proxy n'emet que pour une route HTTPS, le defi HTTP-01 passe avant
    // la table) sans jamais servir le panneau en HTTP clair. Confirme : le
    // proxy ne renvoie plus les hotes IP vers l'acces direct.
    let panel = panel.map(|(host, served, confirmed)| Route {
        host,
        path_prefix: "/".into(),
        upstreams: if served {
            vec![SocketAddr::from((
                Ipv4Addr::LOCALHOST,
                cfg.panel_local_port,
            ))]
        } else {
            Vec::new()
        },
        redirect_to: None,
        tls: TlsMode::Acme,
        service: None,
        panel_confirmed: confirmed,
    });
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            // Nom declare pour le panneau apres coup (alias, URL publique) :
            // il n'est plus servi au projet.
            if reserved(cfg, &r.host) {
                tracing::warn!(host = %r.host, "domaine de projet reserve au panneau : route retiree");
                return None;
            }
            // Domaine genere compris : HTTPS des que le certificat existe,
            // HTTP d'ici la (le proxy ne redirige qu'avec un certificat).
            let tls = TlsMode::Acme;
            if r.redirect {
                let primary = r.primary_host?;
                return Some(Route {
                    host: r.host,
                    path_prefix: "/".into(),
                    upstreams: Vec::new(),
                    redirect_to: Some(format!("https://{primary}")),
                    tls,
                    service: None,
                    panel_confirmed: false,
                });
            }
            r.internal_port?;
            Some(Route {
                host: r.host,
                path_prefix: r.path_prefix,
                upstreams: Vec::new(),
                redirect_to: None,
                tls,
                service: Some(ServiceId(r.service_id?)),
                panel_confirmed: false,
            })
        })
        .chain(panel)
        .collect())
}

/// Envoie la table des routes a chaque agent connecte (apres une
/// modification des domaines). Un agent absent la recoit a sa reconnexion.
pub async fn push_routes(state: &AppState) {
    for server in state.agents.connected() {
        let routes = match state.db.acquire().await {
            Ok(mut conn) => route_table(&mut conn, &state.cfg, server.0).await,
            Err(e) => Err(e),
        };
        match routes {
            Ok(routes) => {
                if let Err(e) = state
                    .agents
                    .send(server, ControlToAgent::ApplyRoutes(routes))
                {
                    tracing::warn!(server = %server.0, "table des routes non envoyee : {e}");
                }
            }
            Err(e) => tracing::error!(error = %e, "table des routes illisible"),
        }
    }
}

/// Domaines supprimes : certificat et cle retires du proxy de chaque serveur
/// connecte (au mieux : un agent absent garde le fichier chiffre, sans route
/// qui le serve). Une route simplement retiree garde son certificat.
pub fn remove_certificates(state: &AppState, hosts: &[String]) {
    for server in state.agents.connected() {
        for host in hosts {
            let order = ControlToAgent::RemoveCertificate { host: host.clone() };
            if let Err(e) = state.agents.send(server, order) {
                tracing::warn!(server = %server.0, %host, "certificat non retire : {e}");
            }
        }
    }
}

/// Serveur qui sert l'environnement (dernier deploiement), sinon le serveur
/// local.
pub async fn server_of(conn: &mut PgConnection, environment: Uuid) -> sqlx::Result<Option<Uuid>> {
    let last: Option<Option<Uuid>> = sqlx::query_scalar(
        "SELECT server_id FROM deployments WHERE environment_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(environment)
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(Some(server)) = last {
        return Ok(Some(server));
    }
    local_server(conn).await
}

/// Serveur local (sinon le plus ancien serveur non revoque) : celui du
/// panneau.
pub async fn local_server(conn: &mut PgConnection) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM servers WHERE revoked_at IS NULL ORDER BY (kind = 'local') DESC, created_at LIMIT 1",
    )
    .fetch_optional(conn)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_normalized_fqdns() {
        assert_eq!(
            normalize_host("App.Example.COM.").unwrap(),
            "app.example.com"
        );
        assert_eq!(
            normalize_host("xn--bcher-kva.example").unwrap(),
            "xn--bcher-kva.example"
        );
        for bad in [
            "",
            "localhost",
            "1.2.3.4",
            "::1",
            "[::1]",
            "a..b",
            "-a.b",
            "a-.b",
            "a_b.c",
            "*.a.b",
            "a.b:80",
            "http://a.b",
            "a.b/c",
            "a.123",
            "ex\u{e9}.com",
        ] {
            assert!(normalize_host(bad).is_err(), "{bad:?}");
        }
    }

    /// sslip.io n'est PAS dans la Public Suffix List (psl 2.1.263, sections
    /// privees comprises) : son domaine enregistrable est `sslip.io` lui-meme,
    /// donc tous les noms generes partagent la limite de certificats par
    /// domaine enregistrable de l'autorite (decision R50). Si ce test casse,
    /// la liste a ete mise a jour : reevaluer la limite.
    #[test]
    fn sslip_io_is_not_a_public_suffix() {
        assert_eq!(
            psl::domain_str("shop.203.0.113.7.sslip.io"),
            Some("sslip.io")
        );
    }

    #[test]
    fn generated_hosts_use_sslip_io() {
        assert_eq!(
            generated_host("shop", "203.0.113.7".parse().unwrap()).as_deref(),
            Some("shop.203.0.113.7.sslip.io")
        );
        assert_eq!(
            generated_host("shop-", "2001:db8::1".parse().unwrap()).as_deref(),
            Some("shop.2001-db8--1.sslip.io")
        );
    }

    #[test]
    fn path_prefixes_stay_unambiguous() {
        for ok in ["/", "/api", "/api/v1", "/a.b_c~d-e/"] {
            assert!(valid_path_prefix(ok), "{ok}");
        }
        for bad in [
            "", "api", "/a//b", "/a/../b", "/./", "/a?b", "/a%2fb", "/a b",
        ] {
            assert!(!valid_path_prefix(bad), "{bad}");
        }
    }
}
