use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ciphera_agent::build::local::LocalBackend;
use ciphera_agent::build::sandbox::PrepareConfig;
use ciphera_agent::build::{self, BuildConfig};
use ciphera_agent::buildkit::{BuildkitManager, DockerFs};
use ciphera_agent::config::{AgentConfig, Identity};
use ciphera_agent::deploy::compose::CliCompose;
use ciphera_agent::deploy::probe::TcpProbe;
use ciphera_agent::deploy::{DeployConfig, Deployer};
use ciphera_agent::docker::BollardDocker;
use ciphera_agent::infra::{self, InfraConfig};
use ciphera_agent::logs::LogFollows;
use ciphera_agent::state::StateDb;
use ciphera_agent::transport::Agent;
use ciphera_agent::{metrics, reconcile};
use proto::{AgentToControl, ControlToAgent};
use tokio::sync::mpsc;

/// Table de routes du plan de controle gardee (dossier de l'etat local).
const ROUTES_FILE: &str = "routes.json";
/// Envois de la table gardee apres le demarrage (secondes d'attente avant
/// chacun) : amonts recalcules a mesure que Docker relance les conteneurs.
const ROUTES_RESTORE_DELAYS: [u64; 5] = [0, 5, 15, 30, 60];
/// Balayage des conteneurs et images de deploiement absents de l'etat local
/// (`Deployer::sweep_orphans`) : au demarrage, puis a cet intervalle.
const ORPHAN_SWEEP_EVERY: Duration = Duration::from_secs(3600);

#[cfg(unix)]
type Proxy = ciphera_agent::proxy_client::ProxyHandle;
#[cfg(not(unix))]
type Proxy = ciphera_agent::routes::NoProxy;
type AgentDeployer = Deployer<BollardDocker, TcpProbe, CliCompose<LocalBackend>, Proxy>;

fn main() {
    // Mode GIT_ASKPASS (lance par `git` pendant un clone) : repond et sort,
    // avant toute initialisation.
    if let Some(code) = build::clone::askpass_main() {
        std::process::exit(code);
    }
    // Mode `egress load|check` (root, ExecStartPre de ciphera-agent.service
    // et ciphera-egress-check.service) : filtrage de sortie, puis sortie.
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = ciphera_agent::egress::cli_main(&args) {
        std::process::exit(code);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| {
            eprintln!("ciphera-agent : runtime tokio : {e}");
            std::process::exit(1);
        });
    runtime.block_on(agent_main());
}

async fn agent_main() {
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

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = AgentConfig::from_env()?;
    let docker = Arc::new(BollardDocker::connect()?);

    // Infrastructure d'abord, sans attendre le lien : le plan de controle a
    // besoin de sa base pour demarrer. Sans configuration (compte manquant),
    // arret en erreur : systemd l'affiche et relance l'agent.
    let infra_cfg = InfraConfig::resolve(Path::new("/etc"))
        .map_err(|e| format!("conteneurs d'infrastructure : {e}"))?;
    // Bac a sable de `railpack prepare` : image epinglee de buildkitd, compte
    // ciphera-build ; sans ce compte, les builds Railpack echouent.
    let prepare = match (
        infra_cfg.buildkit_account,
        infra::pinned_image(infra::BUILDKIT_IMAGE_FILE, "buildkit"),
    ) {
        (Some(account), Ok(image)) => Some(PrepareConfig::production(image, account.user)),
        _ => None,
    };
    // Un buildkitd par projet, demarre a la demande par les builds (meme
    // compte `ciphera-build`, meme image epinglee) ; arret apres 10 min
    // d'inactivite, etat supprime avec le projet ou apres 30 jours.
    let buildkit = match (
        infra_cfg.buildkit_account,
        infra::pinned_image(infra::BUILDKIT_IMAGE_FILE, "buildkit"),
    ) {
        (Some(account), Ok(image)) => match DockerFs::connect(&infra_cfg, image, account.user) {
            Ok(fs) => {
                let manager = Arc::new(BuildkitManager::new(docker.clone(), fs, infra_cfg.clone()));
                tokio::spawn(manager.clone().maintain());
                Some(manager)
            }
            Err(e) => {
                tracing::error!("builds indisponibles, API Docker : {e}");
                None
            }
        },
        _ => None,
    };
    {
        let docker = docker.clone();
        tokio::spawn(async move {
            infra::ensure_infra_with_retry(docker.as_ref(), &infra_cfg).await;
        });
    }

    let db =
        StateDb::open(&cfg.state_db).map_err(|e| format!("{e} ({})", cfg.state_db.display()))?;
    let identity =
        Identity::load(&cfg.dir).map_err(|e| format!("{e} (lancer `ciphera init` d'abord ?)"))?;
    tracing::info!(
        "agent {} : plan de controle {}",
        identity.agent_id,
        cfg.control_addr
    );
    // Proxy du noeud : joint des le demarrage, derniere table renvoyee a
    // chaque reconnexion.
    #[cfg(unix)]
    let proxy = ciphera_agent::proxy_client::spawn(cfg.proxy_socket.clone());
    #[cfg(not(unix))]
    let proxy = ciphera_agent::routes::NoProxy;
    // `docker compose` des deploiements : meme executeur que les builds.
    let compose = LocalBackend::connect(build::DEFAULT_BUILDKIT_SOCKET)
        .map_err(|e| format!("API Docker : {e}"))?;
    let routes_file = cfg
        .state_db
        .parent()
        .unwrap_or(Path::new("."))
        .join(ROUTES_FILE);
    let deployer: Arc<AgentDeployer> = Arc::new(
        Deployer::new(
            docker.clone(),
            db.clone(),
            proxy,
            TcpProbe::default(),
            CliCompose(Arc::new(compose)),
            DeployConfig {
                panel_upstream: Some(cfg.panel_upstream),
                ..DeployConfig::production()
            },
        )
        .with_routes_file(routes_file),
    );
    // Restes d'un arret brutal (bac a sable, dossiers), avant tout ordre ;
    // puis, toutes les heures, conteneurs et images de deploiements absents
    // de l'etat local (suppression perdue ou en echec).
    deployer.sweep().await;
    deployer.sweep_orphans().await;
    {
        let deployer = deployer.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(ORPHAN_SWEEP_EVERY).await;
                deployer.sweep_orphans().await;
            }
        });
    }
    // Table de routes gardee renvoyee au proxy sans attendre le plan de
    // controle (redemarrage de la machine), puis recalculee quelques fois
    // le temps que Docker relance les conteneurs servis.
    {
        let deployer = deployer.clone();
        tokio::spawn(async move {
            for delay in ROUTES_RESTORE_DELAYS {
                tokio::time::sleep(Duration::from_secs(delay)).await;
                if !deployer.restore_routes().await {
                    break;
                }
            }
        });
    }
    let (metrics_slot, latest_metrics) = metrics::slot();
    let state_fn = reconcile::state_fn(docker.clone(), db.clone(), deployer.ops().clone());
    let agent = Agent::new(&cfg, &identity, state_fn.clone())?.with_metrics(latest_metrics);
    let (inbox_tx, mut inbox_rx) = mpsc::channel(64);
    // File des messages de l'agent (DeployProgress, LogLine) ; les metriques
    // passent par leur emplacement.
    let (outbox_tx, outbox_rx) = mpsc::channel::<AgentToControl>(64);
    // Retrait d'un projet : etat de son buildkitd supprime apres l'arret de
    // ses builds.
    let project_buildkit = buildkit.clone();
    // Reseaux de projet vides (projets supprimes pendant une absence de
    // l'agent, restes) : balayage horaire.
    {
        let deployer = deployer.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(ciphera_agent::deploy::NETWORK_SWEEP_EVERY);
            loop {
                every.tick().await;
                deployer.sweep_networks().await;
            }
        });
    }
    // Builds : socket de buildkitd ; sans buildkitd (compte ciphera-build
    // absent), chaque build echoue avec un message clair.
    let builder = BuildConfig::production(prepare)
        .map_err(|e| format!("binaire de l'agent introuvable : {e}"))
        .and_then(|cfg| {
            LocalBackend::connect(cfg.buildkit_socket.clone())
                .map(|backend| match buildkit {
                    Some(manager) => backend.with_buildkit(manager),
                    None => backend,
                })
                .map(|backend| (cfg, Arc::new(backend)))
                .map_err(|e| e.to_string())
        });
    tokio::spawn(metrics::run(
        docker.clone(),
        db.clone(),
        metrics_slot.clone(),
    ));
    let log_follows = LogFollows::new(docker.clone(), db.clone());
    let state_requests = ciphera_agent::reconcile::StateRequests::default();
    let metrics_db = db.clone();
    #[cfg(unix)]
    let proxy_socket = cfg.proxy_socket.clone();
    tokio::spawn(async move {
        while let Some(msg) = inbox_rx.recv().await {
            match msg {
                // Le plan de controle a perdu des messages : meme etat
                // complet qu'a la connexion. Une seule lecture d'etat a la
                // fois ; les demandes recues pendant ce temps en relancent
                // une seule apres elle (`StateRequests`).
                ControlToAgent::RequestState => {
                    if !state_requests.request() {
                        continue;
                    }
                    let state_fn = state_fn.clone();
                    let outbox = outbox_tx.clone();
                    let requests = state_requests.clone();
                    tokio::spawn(async move {
                        requests
                            .serve(
                                || state_fn(),
                                |state| {
                                    let outbox = outbox.clone();
                                    async move {
                                        let _ = outbox.send(AgentToControl::State(state)).await;
                                    }
                                },
                            )
                            .await;
                    });
                }
                ControlToAgent::RequestMetrics => {
                    metrics::sample_once(docker.as_ref(), &metrics_db, &metrics_slot).await;
                }
                ControlToAgent::ApplyRoutes(routes) => {
                    if let Err(e) = deployer.apply_routes(routes).await {
                        tracing::warn!("table de routes non envoyee au proxy : {e}");
                    }
                }
                ControlToAgent::Deploy(spec) => deployer.spawn_deploy(spec, outbox_tx.clone()),
                // Sans attendre la boucle : l'annulation attend la fin de
                // l'operation interrompue.
                ControlToAgent::Stop { deployment_id } => {
                    let deployer = deployer.clone();
                    let outbox = outbox_tx.clone();
                    tokio::spawn(async move { deployer.stop(deployment_id, &outbox).await });
                }
                // Accuse `DeploymentRemoved` seulement si tout a ete supprime.
                ControlToAgent::RemoveDeployment { deployment_id } => {
                    let deployer = deployer.clone();
                    let outbox = outbox_tx.clone();
                    tokio::spawn(
                        async move { deployer.remove_acked(deployment_id, &outbox).await },
                    );
                }
                // Projet supprime : builds et deploiements en cours annules,
                // conteneurs, reseau, volumes (sur demande) et etat de son
                // buildkitd supprimes ; accuse seulement si tout est parti
                // (sinon le plan de controle renvoie l'ordre plus tard).
                ControlToAgent::RemoveProject {
                    project_id,
                    purge_volumes,
                } => {
                    let deployer = deployer.clone();
                    let buildkit = project_buildkit.clone();
                    let outbox = outbox_tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = deployer.remove_project(project_id, purge_volumes).await {
                            tracing::warn!("projet {project_id} : suppression incomplete : {e}");
                            return;
                        }
                        if let Some(manager) = buildkit
                            && let Err(e) = manager.remove_project(project_id).await
                        {
                            tracing::warn!("projet {project_id} : etat de buildkitd : {e}");
                            return;
                        }
                        let _ = outbox
                            .send(AgentToControl::ProjectRemoved { project_id })
                            .await;
                    });
                }
                ControlToAgent::Logs {
                    target,
                    tail,
                    follow,
                } => log_follows.start(target, tail, follow, outbox_tx.clone()),
                ControlToAgent::StopLogs { target } => log_follows.stop(target),
                ControlToAgent::Restart { deployment_id } => {
                    let deployer = deployer.clone();
                    tokio::spawn(async move { deployer.restart(deployment_id).await });
                }
                ControlToAgent::StopContainers { deployment_id } => {
                    let deployer = deployer.clone();
                    tokio::spawn(async move { deployer.stop_containers(deployment_id).await });
                }
                ControlToAgent::Build(spec) => match &builder {
                    Ok((cfg, backend)) => {
                        deployer.spawn_build(spec, cfg.clone(), backend.clone(), outbox_tx.clone())
                    }
                    Err(e) => {
                        tracing::warn!("build {} impossible : {e}", spec.deployment_id);
                        let _ = outbox_tx.try_send(AgentToControl::DeployProgress {
                            deployment_id: spec.deployment_id,
                            stage: proto::Stage::Build,
                            status: proto::StageStatus::Failed { message: e.clone() },
                        });
                    }
                },
                msg @ (ControlToAgent::IssueCertificate { .. }
                | ControlToAgent::CheckCertificate { .. }
                | ControlToAgent::RemoveCertificate { .. }) => {
                    let host = match &msg {
                        ControlToAgent::IssueCertificate { host }
                        | ControlToAgent::CheckCertificate { host }
                        | ControlToAgent::RemoveCertificate { host } => host.clone(),
                        _ => unreachable!("ordre de certificat"),
                    };
                    let outbox = outbox_tx.clone();
                    #[cfg(unix)]
                    let socket = proxy_socket.clone();
                    tokio::spawn(async move {
                        #[cfg(unix)]
                        let outcome =
                            ciphera_agent::proxy_client::certificate_request(&socket, msg).await;
                        #[cfg(not(unix))]
                        let _ = msg;
                        #[cfg(not(unix))]
                        let outcome = proto::CertificateOutcome::Failed {
                            reason: "pas de proxy du noeud sur cette plateforme".into(),
                        };
                        let _ = outbox
                            .send(AgentToControl::Certificate { host, outcome })
                            .await;
                    });
                }
                other => {
                    tracing::info!("ordre recu : {} (pas encore pris en charge)", kind(&other))
                }
            }
        }
    });
    let result = tokio::select! {
        r = agent.run(inbox_tx, outbox_rx) => r.map_err(Into::into),
        _ = tokio::signal::ctrl_c() => Ok(()),
    };
    agent.endpoint.close(0u32.into(), b"arret de l'agent");
    result
}

/// Nature d'un ordre, pour le journal (jamais son contenu).
fn kind(msg: &ControlToAgent) -> &'static str {
    match msg {
        ControlToAgent::Hello { .. } => "hello",
        ControlToAgent::Deploy(_) => "deploy",
        ControlToAgent::Stop { .. } => "stop",
        ControlToAgent::Logs { .. } => "logs",
        ControlToAgent::Pull { .. } => "pull",
        ControlToAgent::ApplyRoutes(_) => "apply-routes",
        ControlToAgent::RequestMetrics => "request-metrics",
        ControlToAgent::RemoveDeployment { .. } => "remove-deployment",
        ControlToAgent::Build(_) => "build",
        ControlToAgent::StopLogs { .. } => "stop-logs",
        ControlToAgent::Restart { .. } => "restart",
        ControlToAgent::StopContainers { .. } => "stop-containers",
        ControlToAgent::IssueCertificate { .. } => "issue-certificate",
        ControlToAgent::CheckCertificate { .. } => "check-certificate",
        ControlToAgent::RemoveCertificate { .. } => "remove-certificate",
        ControlToAgent::RequestState => "request-state",
        ControlToAgent::RenewedCertificate { .. } => "renewed-certificate",
        ControlToAgent::RemoveProject { .. } => "remove-project",
        ControlToAgent::TrustedCas { .. } => "trusted-cas",
    }
}
