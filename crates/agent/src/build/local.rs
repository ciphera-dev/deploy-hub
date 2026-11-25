//! `BuildBackend` de production : outils lances en sous-processus (sans
//! shell, voir `argv`), images chargees, etiquetees et telechargees par
//! l'API Docker (bollard). Chaque outil a un delai ; un processus est tue si
//! sa tache est annulee (`kill_on_drop`).

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bollard::Docker;
use bollard::errors::Error as BollardError;
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, DownloadFromContainerOptions, ImportImageOptions,
    LogsOptions, RemoveContainerOptions, RemoveImageOptions, TagImageOptions,
    UploadToContainerOptions,
};
use bytes::{Bytes, BytesMut};
use futures_util::stream::{self, Stream, StreamExt};
use proto::{ImageRef, LogStream};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

use shared::ids::ProjectId;

use super::argv::AllowedBin;
use super::sandbox::{self, PrepareContainer, PrepareOutput};
use super::{BuildBackend, BuildError, MAX_LOG_LINE, ProgressSink};
use crate::buildkit::{BuildkitLease, ProductionBuildkit};
use crate::docker::{assemble_logs, check_managed};

/// Lignes de journal de `railpack prepare` transmises au plus (les
/// dernieres).
const PREPARE_LOG_TAIL: &str = "2000";
/// Delai de chaque echange avec l'API d'archive (dossier de sortie, lecture
/// d'un fichier de sortie).
const ARCHIVE_TIMEOUT: Duration = Duration::from_secs(30);

/// Delai d'un clone ou d'une normalisation Compose.
pub const SHORT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Delai d'un build (buildctl + chargement).
pub const BUILD_TIMEOUT: Duration = Duration::from_secs(60 * 60);
/// Taille maximale de l'archive d'image que `buildctl` envoie a Docker : au
/// dela, `buildctl` est arrete et le build echoue.
pub const MAX_IMAGE_BYTES: u64 = 10 << 30;
/// Sortie standard capturee au plus (sortie de `docker compose config`).
pub const MAX_CAPTURE: usize = 16 << 20;

fn timeout_for(bin: AllowedBin) -> Duration {
    match bin {
        AllowedBin::Git | AllowedBin::DockerCompose => SHORT_TIMEOUT,
        AllowedBin::Buildctl => BUILD_TIMEOUT,
    }
}

fn tool(bin: AllowedBin, message: impl Into<String>) -> BuildError {
    BuildError::Tool {
        tool: bin.label(),
        message: message.into(),
    }
}

fn image_err(e: BollardError) -> BuildError {
    BuildError::Image(e.to_string())
}

fn prepare_err(message: impl Into<String>) -> BuildError {
    BuildError::Tool {
        tool: "railpack",
        message: message.into(),
    }
}

fn not_found(e: &BollardError) -> bool {
    matches!(
        e,
        BollardError::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

/// `depot:etiquette` (l'etiquette suit le dernier `:` apres le dernier `/`).
fn split_tag(image: &ImageRef) -> (&str, &str) {
    match image.0.rsplit_once(':') {
        Some((repo, tag)) if !tag.contains('/') => (repo, tag),
        _ => (image.0.as_str(), "latest"),
    }
}

/// Ajoute `bytes` a la ligne en cours, sans depasser `MAX_LOG_LINE` octets.
fn push_bounded(line: &mut Vec<u8>, bytes: &[u8]) {
    let room = MAX_LOG_LINE.saturating_sub(line.len());
    line.extend_from_slice(&bytes[..bytes.len().min(room)]);
}

/// Envoie chaque ligne de `reader` au journal du build. Lit tout jusqu'a la
/// fin (le processus n'est jamais bloque sur un tube plein) ; une ligne est
/// coupee a `MAX_LOG_LINE` octets, les octets non UTF-8 remplaces.
async fn forward_lines<R: AsyncRead + Unpin>(reader: R, stream: LogStream, sink: &ProgressSink) {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        let (consumed, complete, eof) = match reader.fill_buf().await {
            Err(_) => break,
            Ok([]) => (0, false, true),
            Ok(buf) => match buf.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    push_bounded(&mut line, &buf[..i]);
                    (i + 1, true, false)
                }
                None => {
                    push_bounded(&mut line, buf);
                    (buf.len(), false, false)
                }
            },
        };
        reader.consume(consumed);
        if complete || (eof && !line.is_empty()) {
            let text = String::from_utf8_lossy(&line);
            sink.log(stream, text.trim_end_matches('\r')).await;
            line.clear();
        }
        if eof {
            break;
        }
    }
}

/// Lit `reader` jusqu'au bout (au plus `MAX_CAPTURE` octets).
async fn capture<R: AsyncRead + Unpin>(reader: R) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    reader
        .take(MAX_CAPTURE as u64 + 1)
        .read_to_end(&mut out)
        .await
        .map_err(|e| e.to_string())?;
    if out.len() > MAX_CAPTURE {
        return Err("sortie trop longue".into());
    }
    Ok(out)
}

/// Groupe de processus d'un outil (l'outil en est le chef). Tuer le groupe
/// arrete aussi ses descendants (aides de `git`, processus de `buildctl`),
/// que `kill_on_drop` laisserait vivre.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(unix), allow(dead_code))]
struct ProcessGroup(Option<u32>);

impl ProcessGroup {
    fn of(child: &tokio::process::Child) -> Self {
        Self(child.id())
    }

    /// SIGKILL au groupe ; groupe deja vide : sans effet.
    fn kill(self) {
        #[cfg(unix)]
        if let Some(pid) = self
            .0
            .and_then(|p| i32::try_from(p).ok())
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
}

/// Tue le groupe de processus a la liberation, sauf si l'outil s'est
/// termine normalement (`finished`) : delai depasse, tache annulee ou echec.
struct GroupGuard {
    group: ProcessGroup,
    armed: bool,
}

impl GroupGuard {
    fn new(child: &tokio::process::Child) -> Self {
        Self {
            group: ProcessGroup::of(child),
            armed: true,
        }
    }

    fn finished(&mut self) {
        self.armed = false;
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        if self.armed {
            self.group.kill();
        }
    }
}

/// Archive lue sur `reader`, coupee en erreur au-dela de `cap` octets :
/// `exceeded` est alors positionne et `on_exceed` appele (arret de l'outil
/// qui produit l'archive).
fn capped_archive<R>(
    reader: R,
    cap: u64,
    exceeded: Arc<AtomicBool>,
    on_exceed: impl Fn() + Send + Sync + 'static,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let on_exceed = Arc::new(on_exceed);
    stream::unfold(Some((reader, 0u64)), move |state| {
        let exceeded = exceeded.clone();
        let on_exceed = on_exceed.clone();
        async move {
            let (mut reader, total) = state?;
            let mut buf = BytesMut::with_capacity(64 * 1024);
            match reader.read_buf(&mut buf).await {
                Ok(0) => None,
                Ok(n) => {
                    let total = total.saturating_add(n as u64);
                    if total > cap {
                        exceeded.store(true, Ordering::SeqCst);
                        on_exceed();
                        let e = std::io::Error::other("image trop volumineuse");
                        return Some((Err(e), None));
                    }
                    Some((Ok(buf.freeze()), Some((reader, total))))
                }
                Err(e) => Some((Err(e), None)),
            }
        }
    })
}

/// Processus et API Docker locaux.
#[derive(Clone)]
pub struct LocalBackend {
    docker: Docker,
    buildkit_socket: PathBuf,
    /// buildkitd par projet ; `None` : le socket partage `buildkit_socket`.
    buildkit: Option<Arc<ProductionBuildkit>>,
}

impl std::fmt::Debug for LocalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalBackend")
            .field("buildkit_socket", &self.buildkit_socket)
            .finish_non_exhaustive()
    }
}

impl LocalBackend {
    /// Socket Docker par defaut ; `buildkit_socket` : socket de buildkitd.
    pub fn connect(buildkit_socket: impl Into<PathBuf>) -> Result<Self, BuildError> {
        Ok(Self {
            docker: Docker::connect_with_socket_defaults().map_err(image_err)?,
            buildkit_socket: buildkit_socket.into(),
            buildkit: None,
        })
    }

    /// Un buildkitd par projet, demarre a la demande.
    #[must_use]
    pub fn with_buildkit(mut self, buildkit: Arc<ProductionBuildkit>) -> Self {
        self.buildkit = Some(buildkit);
        self
    }

    /// Lance l'outil comme chef de son propre groupe de processus.
    fn spawn(
        bin: AllowedBin,
        #[cfg_attr(not(unix), allow(unused_mut))] mut cmd: Command,
    ) -> Result<(tokio::process::Child, GroupGuard), BuildError> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut cmd = tokio::process::Command::from(cmd);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = cmd
            .spawn()
            .map_err(|e| tool(bin, format!("lancement impossible : {e}")))?;
        let guard = GroupGuard::new(&child);
        Ok((child, guard))
    }

    async fn exec_inner(
        bin: AllowedBin,
        cmd: Command,
        sink: &ProgressSink,
        want_stdout: bool,
    ) -> Result<Vec<u8>, BuildError> {
        let (mut child, mut guard) = Self::spawn(bin, cmd)?;
        let stdout = child.stdout.take().expect("stdout pipe");
        let stderr = child.stderr.take().expect("stderr pipe");
        let out = async {
            if want_stdout {
                capture(stdout).await
            } else {
                forward_lines(stdout, LogStream::Stdout, sink).await;
                Ok(Vec::new())
            }
        };
        let (out, (), status) = tokio::join!(
            out,
            forward_lines(stderr, LogStream::Stderr, sink),
            child.wait()
        );
        let status = status.map_err(|e| tool(bin, e.to_string()))?;
        if !status.success() {
            return Err(tool(bin, format!("echec ({status})")));
        }
        guard.finished();
        out.map_err(|e| tool(bin, e))
    }

    async fn load(
        &self,
        cmd: Command,
        staging: &ImageRef,
        sink: &ProgressSink,
    ) -> Result<(), BuildError> {
        let bin = AllowedBin::Buildctl;
        let (mut child, mut guard) = Self::spawn(bin, cmd)?;
        let stdout = child.stdout.take().expect("stdout pipe");
        let stderr = child.stderr.take().expect("stderr pipe");
        // Archive `type=docker` de buildctl, envoyee a Docker au fil de l'eau,
        // plafonnee : au dela, buildctl et ses descendants sont tues.
        let exceeded = Arc::new(AtomicBool::new(false));
        let group = guard.group;
        let archive = capped_archive(stdout, MAX_IMAGE_BYTES, exceeded.clone(), move || {
            group.kill()
        });
        let options = ImportImageOptions {
            quiet: true,
            ..Default::default()
        };
        let import = async {
            let mut progress = self.docker.import_image_stream(options, archive, None);
            let mut result = Ok(());
            while let Some(step) = progress.next().await {
                if let Err(e) = step {
                    result = Err(BuildError::Image(format!("chargement dans Docker : {e}")));
                }
            }
            result
        };
        let (imported, (), status) = tokio::join!(
            import,
            forward_lines(stderr, LogStream::Stderr, sink),
            child.wait()
        );
        if exceeded.load(Ordering::SeqCst) {
            return Err(BuildError::Image(format!(
                "image de plus de {} Gio : build arrete",
                MAX_IMAGE_BYTES >> 30
            )));
        }
        let status = status.map_err(|e| tool(bin, e.to_string()))?;
        if !status.success() {
            return Err(tool(bin, format!("echec du build ({status})")));
        }
        guard.finished();
        imported?;
        self.docker
            .inspect_image(&staging.0)
            .await
            .map_err(|_| BuildError::Image("image absente apres le chargement".into()))?;
        Ok(())
    }
}

impl LocalBackend {
    /// Pont `ciphera-prepare` : cree s'il manque ; existant, il doit etre a
    /// Ciphera, non interne et sans communication entre conteneurs (sinon
    /// refuse, jamais adopte). Celui d'une version precedente, sans nom
    /// d'interface (filtrage de sortie), est supprime puis recree.
    async fn ensure_prepare_network(&self) -> Result<(), BuildError> {
        let bridge_ok = |n: &bollard::models::NetworkInspect| {
            crate::docker::bridge_name_of(n) == Some(sandbox::PREPARE_NETWORK)
        };
        let check = |n: bollard::models::NetworkInspect| {
            check_managed(sandbox::PREPARE_NETWORK, n.labels.as_ref())
                .map_err(|e| prepare_err(e.to_string()))?;
            let icc_off = n
                .options
                .as_ref()
                .and_then(|o| o.get(sandbox::ICC_OPTION))
                .is_some_and(|v| v == "false");
            if n.internal.unwrap_or(false)
                || n.driver.as_deref() != Some("bridge")
                || !icc_off
                || !bridge_ok(&n)
            {
                return Err(prepare_err(format!(
                    "reseau {} inattendu",
                    sandbox::PREPARE_NETWORK
                )));
            }
            Ok(())
        };
        match self
            .docker
            .inspect_network(sandbox::PREPARE_NETWORK, None)
            .await
        {
            Ok(n) if bridge_ok(&n) => return check(n),
            Ok(n) => {
                // Version precedente : seulement s'il est a Ciphera, sans
                // conteneur rattache (la suppression echoue sinon).
                check_managed(sandbox::PREPARE_NETWORK, n.labels.as_ref())
                    .map_err(|e| prepare_err(e.to_string()))?;
                self.docker
                    .remove_network(sandbox::PREPARE_NETWORK)
                    .await
                    .map_err(|e| prepare_err(e.to_string()))?;
            }
            Err(e) if not_found(&e) => {}
            Err(e) => return Err(prepare_err(e.to_string())),
        }
        match self
            .docker
            .create_network(sandbox::prepare_network_request())
            .await
        {
            Ok(_) => Ok(()),
            // Cree entre-temps : on le reverifie.
            Err(BollardError::DockerResponseServerError {
                status_code: 409, ..
            }) => check(
                self.docker
                    .inspect_network(sandbox::PREPARE_NETWORK, None)
                    .await
                    .map_err(|e| prepare_err(e.to_string()))?,
            ),
            Err(e) => Err(prepare_err(e.to_string())),
        }
    }

    /// Image epinglee du bac a sable, telechargee (par digest) si absente.
    async fn ensure_image(&self, image: &str) -> Result<(), BuildError> {
        match self.docker.inspect_image(image).await {
            Ok(_) => return Ok(()),
            Err(e) if not_found(&e) => {}
            Err(e) => return Err(image_err(e)),
        }
        let options = CreateImageOptions {
            from_image: Some(image.to_string()),
            ..Default::default()
        };
        let mut steps = self.docker.create_image(Some(options), None, None);
        while let Some(step) = steps.next().await {
            step.map_err(|e| BuildError::Image(format!("image du bac a sable : {e}")))?;
        }
        Ok(())
    }

    /// Supprime le conteneur `name` (force : tue d'abord) s'il existe et
    /// est a Ciphera.
    async fn remove_managed_container(&self, name: &str) -> Result<(), BuildError> {
        match self.docker.inspect_container(name, None).await {
            Ok(info) => {
                check_managed(name, info.config.as_ref().and_then(|c| c.labels.as_ref()))
                    .map_err(|e| prepare_err(e.to_string()))?;
            }
            Err(e) if not_found(&e) => return Ok(()),
            Err(e) => return Err(prepare_err(e.to_string())),
        }
        // Avec son volume anonyme de sortie.
        let options = RemoveContainerOptions {
            force: true,
            v: true,
            ..Default::default()
        };
        match self.docker.remove_container(name, Some(options)).await {
            Ok(()) => Ok(()),
            Err(e) if not_found(&e) => Ok(()),
            Err(e) => Err(prepare_err(e.to_string())),
        }
    }

    /// Demarre le conteneur et attend son code de sortie.
    async fn start_and_wait(&self, id: &str) -> Result<i64, BuildError> {
        self.docker
            .start_container(id, None)
            .await
            .map_err(|e| prepare_err(e.to_string()))?;
        let mut wait = self.docker.wait_container(id, None);
        match wait.next().await {
            Some(Ok(resp)) => Ok(resp.status_code),
            Some(Err(BollardError::DockerContainerWaitError { code, .. })) => Ok(code),
            Some(Err(e)) => Err(prepare_err(e.to_string())),
            None => Err(prepare_err("attente du conteneur interrompue")),
        }
    }

    /// Cree `sandbox::OUTPUT_DIR` dans le volume de sortie du conteneur
    /// (cree, pas encore demarre) par l'API d'archive.
    async fn create_output_dir(&self, id: &str) -> Result<(), BuildError> {
        let options = UploadToContainerOptions {
            path: sandbox::PLAN_TARGET.to_string(),
            no_overwrite_dir_non_dir: Some("true".to_string()),
            copy_uidgid: None,
        };
        self.docker
            .upload_to_container(
                id,
                Some(options),
                bollard::body_full(Bytes::from(sandbox::output_dir_archive())),
            )
            .await
            .map_err(|e| prepare_err(format!("dossier de sortie : {e}")))
    }

    /// Fichier `name` de `sandbox::OUTPUT_DIR`, lu par l'API d'archive
    /// (`sandbox::MAX_OUTPUT_ARCHIVE` octets au plus) puis verifie.
    async fn read_output_file(&self, id: &str, name: &str) -> Result<Vec<u8>, BuildError> {
        let options = DownloadFromContainerOptions {
            path: format!("{}/{name}", sandbox::OUTPUT_DIR),
        };
        let mut stream = std::pin::pin!(self.docker.download_from_container(id, Some(options)));
        let mut archive = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                if not_found(&e) {
                    prepare_err(format!("{name} absent de la sortie"))
                } else {
                    prepare_err(format!("lecture de {name} : {e}"))
                }
            })?;
            if archive.len() + chunk.len() > sandbox::MAX_OUTPUT_ARCHIVE {
                return Err(sandbox::output_too_big(name));
            }
            archive.extend_from_slice(&chunk);
        }
        sandbox::read_output_file(&archive, name)
    }

    /// Les deux fichiers de sortie du conteneur termine.
    async fn read_output(&self, id: &str) -> Result<PrepareOutput, BuildError> {
        Ok(PrepareOutput {
            plan: self.read_output_file(id, sandbox::PLAN_FILE).await?,
            info: self.read_output_file(id, sandbox::INFO_FILE).await?,
        })
    }

    /// Journaux du conteneur (dernieres lignes) vers le journal du build.
    /// `exited` : conteneur sorti (son attente a abouti) ; sinon (delai
    /// depasse, attente en echec) il peut ecrire encore et la ligne en
    /// attente est ecartee (`docker::assemble_logs`).
    async fn forward_container_logs(&self, id: &str, exited: bool, sink: &ProgressSink) {
        let options = LogsOptions {
            stdout: true,
            stderr: true,
            // L'assembleur retire l'horodatage de chaque message partiel.
            timestamps: true,
            tail: PREPARE_LOG_TAIL.to_string(),
            ..Default::default()
        };
        // Les messages partiels de Docker (16 Kio) sont rassembles avant
        // l'envoi : le plan de controle masque la ligne entiere.
        let mut lines = assemble_logs(
            self.docker.logs(id, Some(options)),
            std::future::ready(exited),
        );
        while let Some(line) = lines.next().await {
            sink.log(line.stream, &line.line).await;
        }
    }
}

impl BuildBackend for LocalBackend {
    fn buildkit_available(&self) -> bool {
        match &self.buildkit {
            Some(manager) => manager.available(),
            None => self.buildkit_socket.exists(),
        }
    }

    async fn buildkit_for(&self, project: ProjectId) -> Result<BuildkitLease, BuildError> {
        let Some(manager) = &self.buildkit else {
            return Ok(BuildkitLease::shared());
        };
        manager
            .ensure(project)
            .await
            .map_err(|e| tool(AllowedBin::Buildctl, format!("buildkitd du projet : {e}")))
    }

    async fn exec(
        &self,
        bin: AllowedBin,
        cmd: Command,
        sink: &ProgressSink,
        capture: bool,
    ) -> Result<Vec<u8>, BuildError> {
        tokio::time::timeout(timeout_for(bin), Self::exec_inner(bin, cmd, sink, capture))
            .await
            .map_err(|_| tool(bin, "delai depasse"))?
    }

    async fn build_and_load(
        &self,
        cmd: Command,
        staging: &ImageRef,
        sink: &ProgressSink,
    ) -> Result<(), BuildError> {
        let result = tokio::time::timeout(BUILD_TIMEOUT, self.load(cmd, staging, sink))
            .await
            .unwrap_or_else(|_| Err(tool(AllowedBin::Buildctl, "delai depasse")));
        if result.is_err() {
            // Rien ne reste sous le nom provisoire (archive chargee avant
            // l'echec de buildctl, par exemple).
            let _ = self.remove_image(staging).await;
        }
        result
    }

    /// Jamais une image interne ; delai et taille annoncee bornes
    /// (`docker::bounded_pull`).
    async fn pull(&self, image: &ImageRef, sink: &ProgressSink) -> Result<(), BuildError> {
        let failed =
            |e: crate::docker::PullError| BuildError::Image(format!("telechargement : {e}"));
        crate::docker::check_pullable(&image.0).map_err(failed)?;
        let options = CreateImageOptions {
            from_image: Some(image.0.clone()),
            ..Default::default()
        };
        let steps = self.docker.create_image(Some(options), None, None);
        crate::docker::bounded_pull(
            steps,
            crate::docker::PULL_TIMEOUT,
            crate::docker::MAX_PULL_BYTES,
            move |status| async move { sink.log(LogStream::Stdout, &status).await },
        )
        .await
        .map_err(failed)
    }

    async fn tag(&self, source: &ImageRef, target: &ImageRef) -> Result<(), BuildError> {
        let (repo, tag) = split_tag(target);
        let options = TagImageOptions {
            repo: Some(repo.to_string()),
            tag: Some(tag.to_string()),
        };
        self.docker
            .tag_image(&source.0, Some(options))
            .await
            .map_err(image_err)
    }

    async fn prepare(
        &self,
        container: &PrepareContainer,
        sink: &ProgressSink,
    ) -> Result<PrepareOutput, BuildError> {
        self.ensure_prepare_network().await?;
        self.ensure_image(&container.image).await?;
        // Reste d'un build interrompu du meme deploiement.
        self.remove_managed_container(&container.name).await?;
        let options = CreateContainerOptions {
            name: Some(container.name.clone()),
            ..Default::default()
        };
        let created = self
            .docker
            .create_container(Some(options), container.body.clone())
            .await
            .map_err(|e| prepare_err(format!("creation du bac a sable : {e}")))?;
        let outcome = async {
            tokio::time::timeout(ARCHIVE_TIMEOUT, self.create_output_dir(&created.id))
                .await
                .map_err(|_| prepare_err("dossier de sortie : delai depasse"))??;
            tokio::time::timeout(sandbox::PREPARE_TIMEOUT, self.start_and_wait(&created.id))
                .await
                .map_err(|_| prepare_err("delai depasse"))?
        }
        .await;
        // `Ok` : l'attente a abouti, le conteneur est sorti.
        self.forward_container_logs(&created.id, outcome.is_ok(), sink)
            .await;
        let output = match outcome {
            Ok(0) => tokio::time::timeout(ARCHIVE_TIMEOUT, self.read_output(&created.id))
                .await
                .unwrap_or_else(|_| Err(prepare_err("lecture de la sortie : delai depasse"))),
            Ok(code) => Err(prepare_err(format!("echec (code {code})"))),
            Err(e) => Err(e),
        };
        // Supprime dans tous les cas avec son volume (tue d'abord s'il
        // tourne encore).
        let removed = self.remove_managed_container(&created.id).await;
        let output = output?;
        removed?;
        Ok(output)
    }

    async fn remove_image(&self, image: &ImageRef) -> Result<(), BuildError> {
        let options = RemoveImageOptions {
            force: false,
            noprune: false,
            ..Default::default()
        };
        match self
            .docker
            .remove_image(&image.0, Some(options), None)
            .await
        {
            Ok(_) => Ok(()),
            Err(BollardError::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(()),
            Err(e) => Err(image_err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn output_lines_are_bounded_and_lossy() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let sink = ProgressSink::new(shared::ids::DeploymentId::new(), tx);
        let mut input = b"un\r\ndeux\xff\n".to_vec();
        input.extend(std::iter::repeat_n(b'x', MAX_LOG_LINE + 10));
        input.extend_from_slice(b"\nfin");
        forward_lines(input.as_slice(), LogStream::Stderr, &sink).await;
        drop(sink);
        let mut lines = Vec::new();
        while let Some(proto::AgentToControl::LogLine(l)) = rx.recv().await {
            lines.push(l.line);
        }
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "un");
        assert_eq!(lines[1], "deux\u{fffd}");
        assert_eq!(lines[2].len(), MAX_LOG_LINE);
        assert_eq!(lines[3], "fin");
    }

    #[tokio::test]
    async fn archive_is_cut_above_the_cap() {
        let data = vec![7u8; 100_000];
        let exceeded = Arc::new(AtomicBool::new(false));
        let called = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = called.clone();
        let archive = capped_archive(
            std::io::Cursor::new(data.clone()),
            64 * 1024,
            exceeded.clone(),
            move || {
                c.fetch_add(1, Ordering::SeqCst);
            },
        );
        let items: Vec<_> = archive.collect().await;
        assert!(items.last().is_some_and(Result::is_err));
        assert!(exceeded.load(Ordering::SeqCst));
        assert_eq!(called.load(Ordering::SeqCst), 1);

        // Au plafond exactement : tout passe.
        let exceeded = Arc::new(AtomicBool::new(false));
        let archive = capped_archive(
            std::io::Cursor::new(data.clone()),
            data.len() as u64,
            exceeded.clone(),
            || panic!("sous le plafond"),
        );
        let items: Vec<_> = archive.collect().await;
        let total: usize = items.iter().map(|i| i.as_ref().unwrap().len()).sum();
        assert_eq!(total, data.len());
        assert!(!exceeded.load(Ordering::SeqCst));
    }

    #[test]
    fn tags_split_after_the_last_slash() {
        let r = |s: &str| ImageRef(s.to_string());
        assert_eq!(
            split_tag(&r("ciphera.invalid/web-prod:abc")),
            ("ciphera.invalid/web-prod", "abc")
        );
        assert_eq!(
            split_tag(&r("ciphera.invalid/staging/abc-web:build")),
            ("ciphera.invalid/staging/abc-web", "build")
        );
        assert_eq!(
            split_tag(&r("localhost:5000/app")),
            ("localhost:5000/app", "latest")
        );
        assert_eq!(split_tag(&r("nginx")), ("nginx", "latest"));
    }
}
