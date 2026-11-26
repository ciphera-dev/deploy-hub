use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum_server::Handle;
use clap::{Args, Parser, Subcommand};
use control_plane::agents::{self, registry::AgentRegistry};
use control_plane::cli::admin;
use control_plane::connectors::github::app::GithubClient;
use control_plane::domain::deployments::orchestrator;
use control_plane::domain::domains::panel::PanelSwitch;
use control_plane::domain::logs::{self, hub::LogHub};
use control_plane::domain::metrics;
use control_plane::domain::variables::VariableSecrets;
use control_plane::http::listener::{self, ConnLimits, Hardening};
use control_plane::http::panel_host;
use control_plane::init::{self, InitOptions};
use control_plane::jobs::worker::{self, Handlers, WorkerConfig};
use control_plane::mail::Delivery;
use control_plane::secrets::SecretBox;
use control_plane::state::{AppState, Clock};
use control_plane::{app, http::redirect, tls};
use shared::config::Config;
use sqlx::postgres::PgPoolOptions;

#[derive(Parser)]
#[command(name = "ciphera", version, about = "Plan de controle Ciphera")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Lance le serveur (comportement par defaut sans sous-commande).
    Serve,
    /// Genere les secrets d'instance, la PKI et le jeton de setup.
    Init(InitArgs),
    /// Administration de l'instance (sur le serveur, configuration du service).
    #[command(subcommand)]
    Admin(AdminCommand),
}

#[derive(Subcommand)]
enum AdminCommand {
    /// Imprime un lien de reinitialisation du mot de passe (30 minutes, usage unique).
    ResetPassword {
        /// E-mail du compte.
        #[arg(long)]
        email: String,
    },
    /// Supprime le second facteur d'un compte (TOTP, passkeys, codes de
    /// secours) et revoque ses sessions.
    ResetMfa {
        /// E-mail du compte.
        email: String,
    },
    /// Retire le domaine du panneau (domaine qui ne repond plus) ; l'acces
    /// 8443 se rouvre au redemarrage du service.
    ResetPanelDomain,
    /// Vide la liste d'IP autorisees (adresse de l'administrateur changee) ;
    /// le service la relit en quelques secondes.
    ClearIpAllowlist,
    /// Renouvelle maintenant les certificats de l'instance (lien des
    /// agents, panneau, agent local), meme deja expires ; en root, puis
    /// `systemctl restart ciphera ciphera-agent`.
    RenewCerts {
        /// Dossier de l'identite de l'agent local.
        #[arg(long, env = "CIPHERA_AGENT_DIR", default_value = "/etc/ciphera-agent")]
        agent_dir: PathBuf,
    },
    /// Nouveau lien de l'assistant d'installation (valable 24 heures), tant
    /// qu'aucun compte n'existe ; en root. Le lien precedent ne vaut plus rien.
    SetupToken {
        /// Adresse IP du panneau dans le lien (par defaut : celle du
        /// certificat du panneau).
        #[arg(long)]
        public_ip: Option<IpAddr>,
        /// Compte systeme du service, proprietaire du fichier du jeton.
        #[arg(long, default_value = "ciphera")]
        owner: String,
    },
    /// Revoque un serveur (definitif) : son agent est deconnecte par le
    /// service en quelques secondes et ne peut plus se reconnecter.
    RevokeAgent {
        /// Identifiant du serveur (UUID de son agent).
        id: String,
    },
    /// Signataire root des certificats (ciphera-certsign.service) : cle de
    /// la CA remise a root (0600), puis demandes du service signees (meme
    /// cle publique, serveur enregistre et non revoque) ou refusees.
    SignRequests,
    /// Rotation de la CA de l'instance, en root : sans option, cree la CA
    /// suivante (les agents passent a elle d'eux-memes) ; `--status` suit
    /// leur passage ; `--finish` remplace l'ancienne CA quand tous les
    /// serveurs non revoques presentent un certificat de la suivante.
    RotateCa {
        /// Termine la rotation (ancienne CA retiree).
        #[arg(long, conflicts_with = "status")]
        finish: bool,
        /// Affiche l'etat de la rotation, sans rien changer.
        #[arg(long)]
        status: bool,
    },
}

#[derive(Args)]
struct InitArgs {
    /// Dossier de configuration de l'instance.
    #[arg(long, env = "CIPHERA_CONFIG_DIR", default_value = Config::DEFAULT_CONFIG_DIR)]
    config_dir: PathBuf,
    /// Adresse IP publique du serveur (SAN du certificat, URL de setup).
    #[arg(long)]
    public_ip: IpAddr,
    /// Port HTTPS du panneau.
    #[arg(long, default_value_t = Config::DEFAULT_PANEL_PORT)]
    panel_port: u16,
    /// Compte systeme proprietaire des fichiers (applique seulement en root).
    #[arg(long, default_value = "ciphera")]
    owner: String,
    /// Dossier de l'identite de l'agent local (cle privee, certificat, CA).
    #[arg(long, env = "CIPHERA_AGENT_DIR", default_value = "/etc/ciphera-agent")]
    agent_dir: PathBuf,
    /// Compte systeme de l'agent local, proprietaire de son dossier (en root).
    #[arg(long, default_value = "ciphera-agent")]
    agent_owner: String,
    /// Compte systeme du conteneur PostgreSQL, proprietaire de la copie de
    /// son mot de passe (en root).
    #[arg(long, default_value = "ciphera-postgres")]
    postgres_owner: String,
}

#[tokio::main]
async fn main() {
    // Notre propre ligne de commande ; clippy.toml interdit Parser::parse pour que
    // l'Opt de Pingora ne soit jamais lu depuis argv.
    #[allow(clippy::disallowed_methods)]
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Init(args)) => std::process::exit(run_init(args)),
        Some(Command::Admin(AdminCommand::ResetPassword { email })) => {
            std::process::exit(run_reset_password(&email).await)
        }
        Some(Command::Admin(AdminCommand::ResetMfa { email })) => {
            std::process::exit(run_reset_mfa(&email).await)
        }
        Some(Command::Admin(AdminCommand::ResetPanelDomain)) => {
            std::process::exit(run_reset_panel_domain().await)
        }
        Some(Command::Admin(AdminCommand::ClearIpAllowlist)) => {
            std::process::exit(run_clear_ip_allowlist().await)
        }
        Some(Command::Admin(AdminCommand::RenewCerts { agent_dir })) => {
            std::process::exit(run_renew_certs(&agent_dir).await)
        }
        Some(Command::Admin(AdminCommand::SetupToken { public_ip, owner })) => {
            std::process::exit(run_setup_token(public_ip, &owner).await)
        }
        Some(Command::Admin(AdminCommand::RevokeAgent { id })) => {
            std::process::exit(run_revoke_agent(&id).await)
        }
        Some(Command::Admin(AdminCommand::SignRequests)) => {
            std::process::exit(run_sign_requests().await)
        }
        Some(Command::Admin(AdminCommand::RotateCa { finish, status })) => {
            std::process::exit(run_rotate_ca(finish, status).await)
        }
        Some(Command::Serve) | None => {}
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    if let Err(e) = run().await {
        tracing::error!("arret : {e}");
        std::process::exit(1);
    }
}

fn run_init(args: InitArgs) -> i32 {
    let opts = InitOptions {
        config_dir: args.config_dir,
        public_ip: args.public_ip,
        panel_port: args.panel_port,
        owner: Some(args.owner),
        agent_dir: args.agent_dir,
        agent_owner: Some(args.agent_owner),
        postgres_owner: Some(args.postgres_owner),
    };
    match init::run(&opts) {
        Ok(report) => {
            for w in &report.warnings {
                eprintln!("avertissement : {w}");
            }
            print!("{}", report.render());
            0
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            1
        }
    }
}

/// Lien de reinitialisation sur la sortie standard ; erreurs sans secret
/// (ni URL de base, ni jeton) sur la sortie d'erreur.
async fn run_reset_password(email: &str) -> i32 {
    let result = async {
        let cfg = control_plane::cli::env::admin_config()?;
        let db = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(10))
            .connect_with(cfg.database.connect_options()?)
            .await
            .map_err(|_| "connexion a la base impossible")?;
        let link = admin::reset_password(&db, &cfg, email).await?;
        let relative = !link.starts_with("http");
        Ok::<_, Box<dyn std::error::Error>>((link, relative))
    }
    .await;
    match result {
        Ok((link, relative)) => {
            println!("{link}");
            if relative {
                eprintln!(
                    "lien relatif : a ouvrir sur l'adresse du panneau (definir CIPHERA_PUBLIC_URL pour un lien complet)"
                );
            }
            eprintln!("valable 30 minutes, usage unique");
            0
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            1
        }
    }
}

/// Remise a zero du second facteur ; messages sans e-mail ni secret.
async fn run_reset_mfa(email: &str) -> i32 {
    let result = async {
        let cfg = control_plane::cli::env::admin_config()?;
        let db = admin_db(&cfg).await?;
        Ok::<_, Box<dyn std::error::Error>>(admin::reset_mfa(&db, email).await?)
    }
    .await;
    match result {
        Ok(r) => {
            println!(
                "second facteur supprime (TOTP : {}, passkeys : {}, codes de secours : {}) ; sessions revoquees : {}",
                r.totp, r.passkeys, r.recovery_codes, r.sessions_revoked
            );
            0
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            1
        }
    }
}

/// Renouvelle les certificats de l'instance ; message sur la sortie d'erreur.
async fn run_renew_certs(agent_dir: &std::path::Path) -> i32 {
    if !running_as_root() {
        eprintln!("erreur : ciphera admin renew-certs se lance en root");
        return 1;
    }
    let result = async {
        let cfg = control_plane::cli::env::admin_config()?;
        let db = admin_db(&cfg).await?;
        agents::certs::renew_all(&db, &cfg.config_dir, agent_dir).await
    }
    .await;
    match result {
        Ok(done) => {
            for crt in &done.server_certs {
                eprintln!("{crt} renouvele");
            }
            match &done.agent {
                Some((id, path)) => {
                    eprintln!("agent local {id} : {} renouvele (meme cle)", path.display())
                }
                None => eprintln!("aucun agent local dans {}", agent_dir.display()),
            }
            eprintln!("a faire maintenant : systemctl restart ciphera ciphera-agent");
            0
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            1
        }
    }
}

/// Rotation de la CA ; messages sur la sortie d'erreur.
async fn run_rotate_ca(finish: bool, status: bool) -> i32 {
    use control_plane::agents::rotation::{self, Start};
    if !running_as_root() {
        eprintln!("erreur : ciphera admin rotate-ca se lance en root");
        return 1;
    }
    let result = async {
        let cfg = control_plane::cli::env::admin_config()?;
        if !finish && !status {
            let message = match rotation::start(&cfg.config_dir)? {
                Start::Started(fp) => {
                    // Journal d'audit au mieux : la CA suivante est deja
                    // ecrite, la rotation ne depend pas de la base.
                    match admin_db(&cfg).await {
                        Ok(db) => rotation::audit(&db, "ca.rotate.start", &fp).await,
                        Err(e) => eprintln!("attention : debut de rotation non journalise : {e}"),
                    }
                    let mut out = format!(
                        "CA suivante creee ({fp}) : les agents connectes passent a elle \
                         d'eux-memes en quelques minutes ; suivre avec ciphera admin \
                         rotate-ca --status, puis terminer avec ciphera admin rotate-ca \
                         --finish (une rotation commencee ne s'annule pas)"
                    );
                    // Redemarrage : la cle de la CA suivante, creee apres le
                    // demarrage du service, lui est masquee (InaccessiblePaths).
                    restart_notice(&mut out);
                    out
                }
                Start::InProgress(fp) => format!(
                    "rotation deja en cours (CA suivante {fp}) : ciphera admin rotate-ca --status"
                ),
            };
            return Ok::<_, Box<dyn std::error::Error>>(message);
        }
        let db = admin_db(&cfg).await?;
        if status {
            let st = rotation::status(&db, &cfg.config_dir).await?;
            let mut out = format!("CA courante : {}", st.current);
            match &st.next {
                None => out.push_str("\naucune rotation en cours"),
                Some(next) => {
                    out.push_str(&format!(
                        "\nCA suivante : {next}\n{} serveur(s) restant(s)",
                        st.remaining.len()
                    ));
                    for p in &st.remaining {
                        out.push_str(&format!("\n  {} ({})", p.name, p.id));
                    }
                    if st.remaining.is_empty() {
                        out.push_str("\npret : ciphera admin rotate-ca --finish");
                    }
                }
            }
            return Ok(out);
        }
        let done = rotation::finish(&db, &cfg.config_dir).await?;
        let mut out = if done.resumed {
            format!(
                "fin de rotation interrompue terminee : CA de l'instance {}",
                done.fingerprint
            )
        } else {
            format!(
                "rotation terminee : CA de l'instance {} ; {} re-signes",
                done.fingerprint,
                done.server_certs.join(", ")
            )
        };
        // Redemarrage du service : lien et panneau relus, agents reconnectes,
        // et masquage des cles de la CA (InaccessiblePaths) repose sur les
        // nouveaux fichiers.
        restart_notice(&mut out);
        Ok(out)
    }
    .await;
    match result {
        Ok(message) => {
            eprintln!("{message}");
            0
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            1
        }
    }
}

/// Redemarre `ciphera.service` et complete `out` (ou dit quoi lancer).
fn restart_notice(out: &mut String) {
    match restart_service("ciphera.service") {
        Ok(()) => out.push_str(" ; service ciphera redemarre"),
        Err(e) => out.push_str(&format!(
            " ; redemarrage du service impossible ({e}) : lancer \
             systemctl restart ciphera"
        )),
    }
}

/// `systemctl restart <unit>` (chemin absolu : la commande peut etre lancee
/// sans PATH, `env -i`).
fn restart_service(unit: &str) -> Result<(), String> {
    let systemctl = ["/usr/bin/systemctl", "/bin/systemctl"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .ok_or("systemctl introuvable")?;
    let status = std::process::Command::new(systemctl)
        .args(["restart", unit])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("systemctl : {status}"))
    }
}

/// Signataire root : messages sur la sortie d'erreur (journal de
/// ciphera-certsign.service). Sans demande, la base n'est pas ouverte.
async fn run_sign_requests() -> i32 {
    use control_plane::agents::signer;
    if !running_as_root() {
        eprintln!(
            "erreur : ciphera admin sign-requests se lance en root (ciphera-certsign.service)"
        );
        return 1;
    }
    let cfg = match control_plane::cli::env::admin_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("erreur : {e}");
            return 1;
        }
    };
    // Dossier de configuration au compte du service (installation
    // anterieure) : remis a root par install.sh, renew-certs ou rotate-ca ;
    // ici en lecture seule (ProtectSystem=strict), simple avertissement.
    if let Err(e) = signer::harden_config_dir(&cfg.config_dir) {
        eprintln!(
            "attention : {} pas a root en 0750 ({e}) : relancer install.sh ou ciphera admin renew-certs",
            cfg.config_dir.display()
        );
    }
    // Echec : la cle de la CA n'est pas protegee (ou plus lisible par le
    // signataire) ; seul cas, avec une CA ou des dossiers inutilisables, ou
    // la commande echoue (une demande refusee n'est pas une panne).
    match signer::harden_ca(&cfg.config_dir) {
        Ok(true) => {
            eprintln!("dossier de la CA remis a root (cle 0600, certificat 0640, dossier 0750)")
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!("erreur : dossier de la CA non protege : {e}");
            // Rien n'est signe ; les demandes sont retirees quand meme, sinon
            // le `.path` relancerait le signataire sans fin (limites de
            // demarrage de systemd, puis plus aucun renouvellement).
            match signer::discard_requests(&cfg.data_dir) {
                Ok((n, report)) => {
                    print_set_aside(&report);
                    if n > 0 {
                        eprintln!("{n} demande(s) abandonnee(s) : dossier de la CA a corriger");
                    }
                }
                Err(e) => eprintln!("erreur : demandes non supprimees : {e}"),
            }
            return 1;
        }
    }
    match signer::has_requests(&cfg.data_dir) {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("aucune demande en attente : rien a signer");
            return 0;
        }
        Err(e) => {
            eprintln!("erreur : demandes illisibles : {e}");
            return 1;
        }
    }
    // La base (conteneur de l'agent) peut demarrer apres nous : attente
    // bornee, puis demandes abandonnees (le service redemande plus tard).
    let mut db = None;
    for attempt in 0..6 {
        match admin_db(&cfg).await {
            Ok(pool) => {
                db = Some(pool);
                break;
            }
            Err(e) if attempt == 5 => eprintln!("attention : base indisponible : {e}"),
            Err(_) => tokio::time::sleep(Duration::from_secs(5)).await,
        }
    }
    let Some(db) = db else {
        return match signer::discard_requests(&cfg.data_dir) {
            Ok((n, report)) => {
                print_set_aside(&report);
                eprintln!(
                    "{n} demande(s) abandonnee(s) : base indisponible, le service redemandera"
                );
                0
            }
            Err(e) => {
                eprintln!("erreur : demandes non supprimees : {e}");
                1
            }
        };
    };
    match signer::sign_requests(&db, &cfg.config_dir, &cfg.data_dir).await {
        Ok(report) => {
            print_set_aside(&report);
            for name in &report.signed {
                eprintln!("certificat {name} signe");
            }
            for (name, why) in &report.refused {
                eprintln!("demande {name} refusee : {why}");
            }
            0
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            1
        }
    }
}

/// Entrees du dossier des demandes impossibles a supprimer (une fois
/// chacune : elles ne sont plus dans le dossier).
fn print_set_aside(report: &control_plane::agents::signer::SignReport) {
    for (name, outcome) in &report.set_aside {
        eprintln!("attention : entree {name:?} du dossier des demandes : {outcome}");
    }
}

/// Processus root (UID effectif 0) ; toujours vrai hors Unix (tests).
fn running_as_root() -> bool {
    #[cfg(unix)]
    {
        rustix::process::geteuid().is_root()
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Connexion a la base de l'instance pour une commande d'administration.
async fn admin_db(cfg: &Config) -> Result<sqlx::PgPool, Box<dyn std::error::Error>> {
    Ok(PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(cfg.database.connect_options()?)
        .await
        // Cause affichee (authentification refusee, socket absent...) : les
        // erreurs de sqlx ne contiennent jamais le mot de passe.
        .map_err(|e| format!("connexion a la base impossible : {e}"))?)
}

/// Retire le domaine du panneau ; message sur la sortie d'erreur.
async fn run_reset_panel_domain() -> i32 {
    let result = async {
        let cfg = control_plane::cli::env::admin_config()?;
        let db = admin_db(&cfg).await?;
        // Commande de secours : sans cle ni client GitHub, le domaine est
        // retire quand meme (webhook a deplacer a la main).
        let secrets = SecretBox::load(&cfg.config_dir.join("age.key"))
            .inspect_err(|e| eprintln!("attention : cle de chiffrement illisible ({e})"))
            .ok();
        let github = GithubClient::new()
            .inspect_err(|e| eprintln!("attention : client GitHub indisponible ({e})"))
            .ok();
        Ok::<_, Box<dyn std::error::Error>>(
            admin::reset_panel_domain(&db, &cfg, secrets.as_ref(), github.as_ref()).await?,
        )
    }
    .await;
    match result {
        Ok(reset) => {
            match reset.github {
                admin::WebhookMove::Moved(url) => {
                    eprintln!("webhook de la GitHub App ramene sur {url}");
                }
                admin::WebhookMove::Failed(url) => {
                    eprintln!(
                        "attention : webhook de la GitHub App NON deplace ; sur GitHub, dans les reglages de l'App, remplacez l'URL du webhook par {} et l'URL de configuration (Setup URL) par l'adresse 8443 du panneau suivie de /connectors",
                        url.as_deref()
                            .unwrap_or("l'adresse 8443 du panneau suivie de /v1/webhooks/github")
                    );
                }
                admin::WebhookMove::Unchanged => {}
            }
            match reset.removed {
                Some(host) => eprintln!(
                    "domaine du panneau {host} retire : redemarrer le service pour rouvrir le port HTTPS du panneau (CIPHERA_PANEL_PORT)"
                ),
                None => eprintln!("aucun domaine du panneau"),
            }
            0
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            1
        }
    }
}

/// Vide la liste d'IP autorisees ; message sur la sortie d'erreur.
async fn run_clear_ip_allowlist() -> i32 {
    let result = async {
        let cfg = control_plane::cli::env::admin_config()?;
        let db = admin_db(&cfg).await?;
        Ok::<_, Box<dyn std::error::Error>>(admin::clear_ip_allowlist(&db).await?)
    }
    .await;
    match result {
        Ok(0) => {
            eprintln!("liste d'IP autorisees deja vide");
            0
        }
        Ok(n) => {
            eprintln!(
                "liste d'IP autorisees videe ({n} entree(s)) : acces rouvert a toutes les adresses d'ici quelques secondes"
            );
            0
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            1
        }
    }
}

/// Nouveau lien de l'assistant d'installation sur la sortie standard (il
/// porte le jeton) ; messages sans secret sur la sortie d'erreur.
async fn run_setup_token(public_ip: Option<IpAddr>, owner: &str) -> i32 {
    use control_plane::setup_token;
    if !running_as_root() {
        eprintln!("erreur : ciphera admin setup-token se lance en root");
        return 1;
    }
    let result = async {
        let cfg = control_plane::cli::env::admin_config()?;
        let account = init::account_ids(owner);
        if cfg!(unix) && account.is_none() {
            return Err(format!("compte systeme {owner} introuvable").into());
        }
        let cert = std::fs::read_to_string(cfg.config_dir.join("tls/panel.crt")).ok();
        let ip = public_ip
            .or_else(|| cert.as_deref().and_then(pki::ip_from_cert_pem))
            .ok_or("adresse IP du panneau introuvable : relancer avec --public-ip")?;
        let fingerprint = cert
            .as_deref()
            .and_then(pki::der_from_pem)
            .map(|der| pki::fingerprint_sha256(&der));
        let db = admin_db(&cfg).await?;
        let token = admin::setup_token(&db, &cfg.config_dir, account).await?;
        let url = setup_token::url(SocketAddr::new(ip, cfg.panel_port), &token);
        Ok::<_, Box<dyn std::error::Error>>((url, fingerprint))
    }
    .await;
    match result {
        Ok((url, fingerprint)) => {
            println!("{url}");
            if let Some(fp) = fingerprint {
                eprintln!("empreinte SHA-256 du certificat du panneau : {fp}");
            }
            eprintln!("valable 24 heures, usage unique ; le lien precedent ne vaut plus rien");
            0
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            1
        }
    }
}

/// Revoque un serveur ; message sur la sortie d'erreur.
async fn run_revoke_agent(id: &str) -> i32 {
    let result = async {
        let id: shared::ids::ServerId = id
            .trim()
            .parse()
            .map_err(|_| format!("identifiant de serveur invalide : {id}"))?;
        let cfg = control_plane::cli::env::admin_config()?;
        let db = admin_db(&cfg).await?;
        Ok::<_, Box<dyn std::error::Error>>((id, admin::revoke_agent(&db, id).await?))
    }
    .await;
    match result {
        Ok((id, admin::AgentRevoke::Revoked)) => {
            eprintln!(
                "serveur {id} revoque : son agent sera deconnecte d'ici quelques secondes et ne pourra plus se reconnecter"
            );
            0
        }
        Ok((id, admin::AgentRevoke::AlreadyRevoked)) => {
            eprintln!("serveur {id} deja revoque");
            0
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            1
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let env_ip = std::env::var_os("CIPHERA_PUBLIC_IP").is_some_and(|v| !v.is_empty());
    let (cfg, ip_source) = tls::with_panel_ip(Config::from_env()?, env_ip);
    let cfg = Arc::new(cfg);
    tls::log_ip_source(&cfg, &ip_source);
    let secrets = Arc::new(
        SecretBox::load(&cfg.config_dir.join("age.key"))
            .map_err(|e| format!("{e} (lancer `ciphera init` d'abord ?)"))?,
    );

    // Les options de connexion ne sont jamais journalisees (mot de passe).
    // La base (conteneur de l'agent) peut demarrer apres le service : attente.
    let connect = cfg.database.connect_options()?;
    let db = control_plane::db::retry_until_ready(
        || {
            PgPoolOptions::new()
                .max_connections(10)
                .acquire_timeout(Duration::from_secs(10))
                .connect_with(connect.clone())
        },
        control_plane::db::sanitized_cause,
    )
    .await;
    sqlx::migrate!().run(&db).await?;
    tracing::info!("base de donnees prete, migrations appliquees");
    // Secrets chiffres avant la liaison a leur ligne : rechiffres une fois.
    control_plane::domain::variables::rebind_all(&db, &secrets).await?;

    // Remplacement d'une paire certificat + cle interrompu (coupure pendant
    // un renouvellement) : termine ou annule avant toute lecture.
    if let Err(e) = agents::certs::recover_pairs(&cfg.config_dir) {
        tracing::error!("reprise d'un renouvellement de certificat interrompu impossible : {e}");
    }
    // Certificats du lien et du panneau demandes au signataire root aux
    // deux tiers de leur validite (ici puis toutes les 12 h), certificats
    // des agents a leur connexion (agents::certs::Renewals) : le service n'a
    // pas la cle de la CA.
    let renewals = Arc::new(agents::certs::Renewals::new(&cfg.config_dir, &cfg.data_dir));
    renewals.request_own(agents::certs::now_unix());
    let tls_config = tls::load(&cfg).await.map_err(|e| {
        let (crt, key) = tls::panel_paths(&cfg);
        format!(
            "certificat du panneau illisible ({} / {}) : {e}",
            crt.display(),
            key.display()
        )
    })?;

    let agents = AgentRegistry::new();
    // Abonne avant l'ouverture du lien : aucun `State` d'agent n'est manque.
    // Lignes de journal sur leur propre canal (`subscribe_logs`).
    let agent_events = agents.subscribe();
    let agent_logs = agents.subscribe_logs();
    let log_events = agents.subscribe();
    let log_lines = agents.subscribe_logs();
    let log_store_lines = agents.subscribe_logs();
    let agent_link = start_agent_link(&cfg, &db, &agents, renewals.clone()).await?;
    tokio::spawn(renew_certs_periodically(
        cfg.clone(),
        renewals,
        agent_link.clone(),
        agents.clone(),
        tls_config.clone(),
    ));

    let https_addr = SocketAddr::from(([0, 0, 0, 0], cfg.panel_port));
    let http_addr = cfg
        .http_port
        .map(|port| SocketAddr::from(([0, 0, 0, 0], port)));
    let github = Arc::new(
        control_plane::connectors::github::app::GithubClient::new()
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?,
    );
    // Masquage des journaux : valeurs secretes des variables du projet.
    let redaction = Arc::new(VariableSecrets::new(db.clone(), secrets.clone()));
    let state = AppState {
        db: db.clone(),
        cfg: cfg.clone(),
        secrets,
        clock: Clock::system(),
        mail: Delivery::smtp(),
        logs: LogHub::new(agents.clone(), redaction),
        agents,
        github,
        dns: Arc::new(control_plane::domain::domains::SystemResolver),
        panel: PanelSwitch::load(&db).await?,
        ip_allowlist: Default::default(),
        access: Default::default(),
        releases: Default::default(),
        update_request_dir: shared::update::REQUEST_DIR.into(),
        public_limits: Default::default(),
        login_flights: Default::default(),
    };
    let direct_open = state.panel.subscribe();
    let panel = state.panel.clone();
    let direct_hosts = state.panel.clone();
    // File de taches : deploiements, nettoyage des images, ACME, metriques
    // et notifications (sans gestionnaire, une tache echoue de facon
    // terminale). Priorite par genre (deploiements d'abord) ; ACME et
    // notifications plafonnes a une tache en cours chacun, pour que les
    // deploiements gardent des creneaux (voir `jobs`).
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let stop_jobs = Arc::new(stop_tx);
    let stop_jobs_on_exit = stop_jobs.clone();
    let job_worker = tokio::spawn(worker::run(
        db.clone(),
        Clock::system(),
        control_plane::domain::notifications::register(
            metrics::register(
                control_plane::domain::domains::acme::register(
                    orchestrator::register(Handlers::new(), state.clone()),
                    state.clone(),
                ),
                state.clone(),
            ),
            state.clone(),
        ),
        WorkerConfig::new(format!("cp-{}", uuid::Uuid::new_v4())),
        stop_rx.clone(),
    ));
    // Suivi des messages des agents et chien de garde des deploiements.
    let deployments = tokio::spawn(orchestrator::run(
        state.clone(),
        agent_events,
        agent_logs,
        stop_rx.clone(),
    ));
    // Logs en direct : diffusion aux clients et enregistrement des lignes
    // des deploiements.
    let log_hub = tokio::spawn(logs::hub::run(
        state.logs.clone(),
        log_events,
        log_lines,
        stop_rx.clone(),
    ));
    let log_store = tokio::spawn(logs::store::run(
        state.clone(),
        log_store_lines,
        stop_rx.clone(),
    ));
    // Agregation horaire des metriques (sur la file de taches).
    let metrics_rollup = tokio::spawn(metrics::run(db.clone(), stop_rx.clone()));
    // Purge quotidienne des echecs de connexion anciens du journal d'audit.
    let audit_purge = tokio::spawn(control_plane::auth::audit_purge::run(
        db.clone(),
        stop_rx.clone(),
    ));
    // Bascule vers le domaine du panneau restee sans certificat : abandonnee.
    let panel_expiry = tokio::spawn(control_plane::domain::domains::panel::run_expiry(
        state.clone(),
        stop_rx.clone(),
    ));
    // Lecture du corps bornee dans le temps (http::listener), sur les deux ecoutes.
    let router =
        listener::with_body_deadline(app::build_router(state), Hardening::PUBLIC.body_read);

    // Hachage factice des connexions a un e-mail inconnu, calcule d'avance.
    tokio::task::spawn_blocking(control_plane::http::auth::warm_up).await?;

    let handle = Handle::new();
    let shutdown = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!("arret demande, fermeture propre");
        let _ = stop_jobs.send(true);
        shutdown.graceful_shutdown(Some(Duration::from_secs(10)));
    });

    // Ecoute HTTP en boucle locale : le proxy du noeud y sert le domaine du
    // panneau (route `panel`). Jamais exposee hors de la machine.
    let local_addr = SocketAddr::from(([127, 0, 0, 1], cfg.panel_local_port));
    tracing::info!(
        "HTTPS sur {https_addr} (jusqu'a la confirmation d'un domaine du panneau), ecoute locale du proxy sur {local_addr}"
    );
    // Ecouteurs bornes (delais, plafonds de connexions) : voir http::listener.
    // Acces direct : 421 pour un nom non declare (http::panel_host).
    let direct_router = panel_host::direct(router.clone(), cfg.clone(), direct_hosts);
    let https = serve_direct(https_addr, tls_config, direct_router, direct_open, stop_rx);
    let local_limits = ConnLimits::new(
        Hardening::LOCAL.max_connections,
        Hardening::LOCAL.max_per_ip,
    );
    let local = listener::plain(local_addr, Hardening::LOCAL, local_limits)
        .handle(handle.clone())
        // Adresse du pair TCP : boucle locale ; requetes marquees, seules a
        // pouvoir faire croire X-Forwarded-For (pose par le proxy).
        .serve(
            listener::with_idle_tracking(panel_host::local(router))
                .into_make_service_with_connect_info::<SocketAddr>(),
        );
    // Redirection HTTP facultative (CIPHERA_HTTP_PORT) : le port 80 appartient
    // au proxy du noeud, qui redirige deja les hotes IP vers le panneau.
    let panel_port = cfg.panel_port;
    let reserved = cfg.reserved_hosts();
    let http = async move {
        let Some(addr) = http_addr else {
            return Ok(());
        };
        tracing::info!("redirection HTTP sur {addr}");
        let limits = ConnLimits::new(
            Hardening::PUBLIC.max_connections,
            Hardening::PUBLIC.max_per_ip,
        );
        listener::plain(addr, Hardening::PUBLIC, limits)
            .handle(handle)
            .serve(
                listener::with_idle_tracking(redirect::router(panel_port, panel, reserved))
                    .into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
    };
    let served = tokio::try_join!(https, local, http);
    if let Some(endpoint) = &agent_link {
        endpoint.close(0u32.into(), b"arret du plan de controle");
    }
    // Le serveur est arrete (signal ou erreur) : arret du travailleur aussi.
    let _ = stop_jobs_on_exit.send(true);
    let _ = job_worker.await;
    let _ = deployments.await;
    let _ = log_hub.await;
    let _ = log_store.await;
    let _ = metrics_rollup.await;
    let _ = audit_purge.await;
    let _ = panel_expiry.await;
    served?;
    Ok(())
}

/// Acces HTTPS direct au panneau (8443, certificat auto-signe) : ouvert tant
/// qu'aucun domaine du panneau n'est confirme (`open`), ferme proprement a la
/// confirmation, rouvert si le domaine est retire ; jusqu'a l'arret (`stop`).
async fn serve_direct(
    addr: SocketAddr,
    tls: axum_server::tls_rustls::RustlsConfig,
    router: axum::Router,
    mut open: tokio::sync::watch::Receiver<bool>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) -> std::io::Result<()> {
    const GRACE: Duration = Duration::from_secs(10);
    // Plafonds communs aux reouvertures successives de l'acces direct.
    let limits = ConnLimits::new(
        Hardening::PUBLIC.max_connections,
        Hardening::PUBLIC.max_per_ip,
    );
    loop {
        if *stop.borrow() {
            return Ok(());
        }
        if !*open.borrow_and_update() {
            tracing::info!("acces direct {addr} ferme : panneau servi par son domaine");
            tokio::select! {
                changed = open.changed() => {
                    if changed.is_err() {
                        return Ok(());
                    }
                    continue;
                }
                _ = stop.wait_for(|s| *s) => return Ok(()),
            }
        }
        let handle = Handle::new();
        let server = listener::tls(addr, tls.clone(), Hardening::PUBLIC, limits.clone())
            .handle(handle.clone())
            // Adresse du pair TCP : IP cliente (limitation de debit, audit).
            .serve(
                listener::with_idle_tracking(router.clone())
                    .into_make_service_with_connect_info::<SocketAddr>(),
            );
        tokio::pin!(server);
        loop {
            tokio::select! {
                served = &mut server => return served,
                _ = stop.wait_for(|s| *s) => {
                    handle.graceful_shutdown(Some(GRACE));
                    return (&mut server).await;
                }
                changed = open.changed() => {
                    if changed.is_err() || !*open.borrow() {
                        handle.graceful_shutdown(Some(GRACE));
                        (&mut server).await?;
                        break;
                    }
                }
            }
        }
    }
}

/// Toutes les 12 h : certificats du lien et du panneau demandes au
/// signataire s'ils ont passe les deux tiers de leur validite ; toutes les
/// 2 s : certificats signes installes, puis recharges a chaud, et CA de
/// confiance relues (rotation de la CA : lien et panneau recharges, agents
/// reconnectes, certificats d'une CA retiree redemandes).
async fn renew_certs_periodically(
    cfg: Arc<Config>,
    renewals: Arc<agents::certs::Renewals>,
    link: Option<quinn::Endpoint>,
    registry: AgentRegistry,
    panel: axum_server::tls_rustls::RustlsConfig,
) {
    let mut checked = std::time::Instant::now();
    let mut trust = agents::transport::TrustWatch::new(&cfg.config_dir);
    loop {
        tokio::time::sleep(agents::certs::RESPONSE_POLL).await;
        let trust_changed = trust.changed();
        if trust_changed || checked.elapsed() >= agents::certs::CHECK_EVERY {
            checked = std::time::Instant::now();
            renewals.request_own(agents::certs::now_unix());
        }
        let mut renewed = renewals.install_own_responses(agents::certs::now_unix());
        if trust_changed {
            tracing::warn!("CA de l'instance changees (rotation) : lien et panneau recharges");
            if let Some(endpoint) = &link {
                match agents::transport::apply_trust_change(endpoint, &cfg.config_dir, &registry) {
                    Ok(ids) => tracing::info!(
                        "lien agents : {} agent(s) reconnecte(s) pour les nouvelles CA",
                        ids.len()
                    ),
                    Err(e) => tracing::error!("lien agents : nouvelles CA non chargees : {e}"),
                }
            }
            renewed.retain(|crt| *crt != agents::transport::LINK_CRT);
            renewed.push(agents::certs::PANEL_CRT);
        }
        if renewed.contains(&agents::transport::LINK_CRT)
            && let Some(endpoint) = &link
            && let Err(e) = agents::transport::reload_link(endpoint, &cfg.config_dir)
        {
            tracing::error!("lien agents : nouveau certificat non charge : {e}");
        }
        if renewed.contains(&agents::certs::PANEL_CRT) {
            let (crt, key) = tls::panel_paths(&cfg);
            if let Err(e) = panel.reload_from_pem_file(crt, key).await {
                tracing::error!("panneau : nouveau certificat non charge : {e}");
            }
        }
    }
}

/// Enregistre le serveur local et ouvre le lien QUIC des agents, qui
/// inscrit les agents connectes dans `registry` (celui de `AppState`). Sans
/// les fichiers de `ciphera init` (instance initialisee avant le lien), le
/// panneau demarre sans lien, avec un avertissement.
async fn start_agent_link(
    cfg: &Config,
    db: &sqlx::PgPool,
    registry: &AgentRegistry,
    renewals: Arc<agents::certs::Renewals>,
) -> Result<Option<quinn::Endpoint>, Box<dyn std::error::Error>> {
    match agents::servers::register_local_from_dir(db, &cfg.config_dir).await? {
        Some(id) => tracing::info!("serveur local {id} enregistre"),
        None => tracing::warn!(
            "certificat de l'agent local absent ({}) : agent local non enregistre",
            cfg.config_dir
                .join(agents::servers::LOCAL_AGENT_CERT)
                .display()
        ),
    }
    let tls = match agents::transport::LinkTls::load(&cfg.config_dir) {
        Ok(tls) => tls,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!("certificat du lien agents absent : lien QUIC non demarre");
            return Ok(None);
        }
        Err(e) => return Err(format!("certificat du lien agents illisible : {e}").into()),
    };
    let addr = SocketAddr::new(cfg.agent_link_bind, cfg.agent_link_port);
    let endpoint = agents::transport::bind(addr, agents::transport::server_config(&tls)?)
        .map_err(|e| format!("ecoute UDP {addr} du lien agents impossible : {e}"))?;
    tokio::spawn(agents::transport::serve_with_renewal(
        endpoint.clone(),
        registry.clone(),
        db.clone(),
        Some(renewals),
    ));
    // Revocations posees hors du service (`ciphera admin revoke-agent`).
    tokio::spawn(agents::transport::watch_revocations(
        db.clone(),
        registry.clone(),
    ));
    tracing::info!("lien agents (QUIC + mTLS) sur {addr}");
    Ok(Some(endpoint))
}

/// Ctrl+C, et SIGTERM sous Unix (arret par systemd).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
