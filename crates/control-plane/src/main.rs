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
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Init(args)) => std::process::exit(run_init(args)),
        Some(Command::Admin(AdminCommand::ResetPassword { email })) => {
            std::process::exit(run_reset_password(&email).await)
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

/// Renouvelle les certificats de l'instance ; message sur la sortie d'erreur.
async fn run_renew_certs(agent_dir: &std::path::Path) -> i32 {
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

    // CA de l'instance : certificats du lien et du panneau renouveles aux
    // deux tiers de leur validite (ici puis toutes les 12 h), certificats
    // des agents a leur connexion (agents::certs).
    let ca = match agents::certs::load_ca(&cfg.config_dir) {
        Ok(ca) => {
            agents::certs::renew_own(&cfg.config_dir, &ca, agents::certs::now_unix());
            Some(Arc::new(ca))
        }
        Err(e) => {
            tracing::warn!(
                "CA de l'instance illisible ({e}) : certificats non renouveles automatiquement \
                 (`ciphera admin renew-certs`)"
            );
            None
        }
    };
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
    let agent_link = start_agent_link(&cfg, &db, &agents, ca.clone()).await?;
    if let Some(ca) = ca {
        tokio::spawn(renew_certs_periodically(
            cfg.clone(),
            ca,
            agent_link.clone(),
            tls_config.clone(),
        ));
    }

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
    // File de taches : deploiements et nettoyage des images ; les autres
    // genres (ACME...) arrivent avec leurs taches (sans gestionnaire, une
    // tache echoue de facon terminale).
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
    // Bascule vers le domaine du panneau restee sans certificat : abandonnee.
    let panel_expiry = tokio::spawn(control_plane::domain::domains::panel::run_expiry(
        state.clone(),
        stop_rx.clone(),
    ));
    let router = app::build_router(state);

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
    let https = serve_direct(https_addr, tls_config, router.clone(), direct_open, stop_rx);
    let local = listener::plain(
        local_addr,
        Hardening::LOCAL,
        ConnLimits::new(
            Hardening::LOCAL.max_connections,
            Hardening::LOCAL.max_per_ip,
        ),
    )
    .handle(handle.clone())
    // Adresse du pair TCP : boucle locale (le proxy pose X-Forwarded-For).
    .serve(router.into_make_service_with_connect_info::<SocketAddr>());
    // Redirection HTTP facultative (CIPHERA_HTTP_PORT) : le port 80 appartient
    // au proxy du noeud, qui redirige deja les hotes IP vers le panneau.
    let panel_port = cfg.panel_port;
    let http = async move {
        let Some(addr) = http_addr else {
            return Ok(());
        };
        tracing::info!("redirection HTTP sur {addr}");
        listener::plain(
            addr,
            Hardening::PUBLIC,
            ConnLimits::new(
                Hardening::PUBLIC.max_connections,
                Hardening::PUBLIC.max_per_ip,
            ),
        )
        .handle(handle)
        .serve(redirect::router(panel_port, panel).into_make_service())
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
                router
                    .clone()
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

/// Toutes les 12 h : certificats du lien et du panneau renouveles s'ils ont
/// passe les deux tiers de leur validite, puis recharges a chaud.
async fn renew_certs_periodically(
    cfg: Arc<Config>,
    ca: Arc<pki::Ca>,
    link: Option<quinn::Endpoint>,
    panel: axum_server::tls_rustls::RustlsConfig,
) {
    loop {
        tokio::time::sleep(agents::certs::CHECK_EVERY).await;
        let renewed = agents::certs::renew_own(&cfg.config_dir, &ca, agents::certs::now_unix());
        if renewed.contains(&agents::transport::LINK_CRT)
            && let Some(endpoint) = &link
        {
            match agents::transport::LinkTls::load(&cfg.config_dir)
                .map_err(|e| e.to_string())
                .and_then(|tls| agents::transport::server_config(&tls).map_err(|e| e.to_string()))
            {
                Ok(server) => endpoint.set_server_config(Some(server)),
                Err(e) => tracing::error!("lien agents : nouveau certificat non charge : {e}"),
            }
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
    ca: Option<Arc<pki::Ca>>,
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
        ca,
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
