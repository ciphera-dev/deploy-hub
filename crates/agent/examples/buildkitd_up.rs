//! Outil du test d'installation (`install/tests/run-docker-host.sh`) : demarre
//! le buildkitd d'un projet, comme le ferait son premier build, puis affiche
//! son socket. Jamais livre dans le binaire de production ni installe.
//!
//!   buildkitd_up <uuid du projet>
//!
//! A lancer sous le compte `ciphera-agent` (refuse root : il creerait des
//! fichiers que l'agent ne pourrait plus ecrire) et sans build en cours : ce
//! processus a son propre gestionnaire, qui arrete les buildkitd deja en
//! marche au demarrage.

#[cfg(unix)]
mod tool {
    use std::path::Path;
    use std::sync::Arc;

    use ciphera_agent::buildkit::{BuildkitManager, DockerFs};
    use ciphera_agent::docker::BollardDocker;
    use ciphera_agent::infra::{self, InfraConfig};
    use shared::ids::ProjectId;

    pub async fn run(project: Option<String>) -> Result<(), String> {
        if rustix::process::geteuid().as_raw() == 0 {
            return Err("refuse sous root : lancer sous le compte ciphera-agent".into());
        }
        let project: ProjectId = project
            .ok_or("UUID du projet attendu")?
            .parse()
            .map_err(|e| format!("UUID du projet : {e}"))?;
        let cfg = InfraConfig::resolve(Path::new("/etc")).map_err(|e| e.to_string())?;
        let account = cfg
            .buildkit_account
            .ok_or("compte ciphera-build absent (relancer install.sh)")?;
        let image = infra::pinned_image(infra::BUILDKIT_IMAGE_FILE, "buildkit")
            .map_err(|e| e.to_string())?;
        let docker = Arc::new(BollardDocker::connect().map_err(|e| e.to_string())?);
        let fs = DockerFs::connect(&cfg, image, account.user).map_err(|e| e.to_string())?;
        let manager = BuildkitManager::new(docker, fs, cfg);
        let lease = manager.ensure(project).await.map_err(|e| e.to_string())?;
        if let Some(socket) = lease.socket() {
            println!("{}", socket.display());
        }
        Ok(())
    }
}

#[cfg(unix)]
fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime tokio");
    if let Err(e) = runtime.block_on(tool::run(std::env::args().nth(1))) {
        eprintln!("buildkitd_up : {e}");
        std::process::exit(1);
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("buildkitd_up : Linux seulement");
    std::process::exit(1);
}
