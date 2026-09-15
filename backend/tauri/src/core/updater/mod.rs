use crate::config::nyanpasu::ClashCore;
use anyhow::{Result, anyhow};
use futures_util::FutureExt;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use serde::{Deserialize, Serialize};
use shared::{CoreTypeMeta, get_arch};
use specta::Type;
use std::{
    collections::HashMap,
    panic::AssertUnwindSafe,
    sync::Arc,
    time::{Duration, Instant},
};

mod instance;
pub(crate) mod ports;
mod shared;
pub(crate) use instance::HttpUpdaterBackend;
pub use instance::{UpdaterState, UpdaterSummary};
use ports::{CoreUpdateInstaller, UpdaterBackend, UpdaterProgress};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ManifestVersion {
    manifest_version: u64,
    latest: ManifestVersionLatest,
    arch_template: ArchTemplate,
    updated_at: String,
}

// TODO: manifest v2 should be kebad-case
#[derive(Deserialize, Serialize, Clone, Debug, Type)]
pub struct ManifestVersionLatest {
    mihomo: String,
    mihomo_alpha: String,
    clash_rs: String,
    clash_rs_alpha: String,
    clash_premium: String,
    meow: String,
}

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
pub struct ArchTemplate {
    mihomo: HashMap<String, String>,
    mihomo_alpha: HashMap<String, String>,
    clash_rs: HashMap<String, String>,
    clash_rs_alpha: HashMap<String, String>,
    clash_premium: HashMap<String, String>,
    meow: HashMap<String, String>,
}

impl Default for ManifestVersion {
    fn default() -> Self {
        Self {
            manifest_version: 0,
            latest: ManifestVersionLatest::default(),
            arch_template: ArchTemplate::default(),
            updated_at: "".to_string(),
        }
    }
}

impl Default for ManifestVersionLatest {
    fn default() -> Self {
        Self {
            mihomo: "".to_string(),
            mihomo_alpha: "".to_string(),
            clash_rs: "".to_string(),
            clash_rs_alpha: "".to_string(),
            clash_premium: "".to_string(),
            meow: "".to_string(),
        }
    }
}

impl ManifestVersion {
    pub(self) fn get_matches(&self, core_type: &ClashCore) -> Option<(String, CoreTypeMeta)> {
        let arch = get_arch().ok()?;
        match core_type {
            ClashCore::ClashPremium => Some((
                self.arch_template
                    .clash_premium
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.clash_premium),
                CoreTypeMeta::ClashPremium(self.latest.clash_premium.clone()),
            )),
            ClashCore::Mihomo => Some((
                self.arch_template
                    .mihomo
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.mihomo),
                CoreTypeMeta::Mihomo(self.latest.mihomo.clone()),
            )),
            ClashCore::MihomoAlpha => Some((
                self.arch_template
                    .mihomo_alpha
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.mihomo_alpha),
                CoreTypeMeta::MihomoAlpha,
            )),
            ClashCore::ClashRs => Some((
                self.arch_template
                    .clash_rs
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.clash_rs),
                CoreTypeMeta::ClashRs(self.latest.clash_rs.clone()),
            )),
            ClashCore::ClashRsAlpha => Some((
                self.arch_template
                    .clash_rs_alpha
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.clash_rs_alpha),
                CoreTypeMeta::ClashRsAlpha,
            )),
            ClashCore::Meow => Some((
                self.arch_template
                    .meow
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.meow),
                CoreTypeMeta::Meow(self.latest.meow.clone()),
            )),
        }
    }
}

const RETENTION: Duration = Duration::from_secs(300);
const MAX_TASKS: usize = 64;

enum Message {
    Fetch(RpcReplyPort<Result<ManifestVersionLatest>>),
    Fetched(Box<Result<(ManifestVersion, (String, Instant))>>),
    Start(ClashCore, RpcReplyPort<Result<usize>>),
    Inspect(usize, RpcReplyPort<Result<UpdaterSummary>>),
    Progress(
        usize,
        UpdaterState,
        Option<crate::core::download::DownloadStatus>,
    ),
    Finished(usize, Result<()>),
    Prune(Instant),
    Shutdown(RpcReplyPort<Result<()>>),
}

struct Task {
    core: ClashCore,
    summary: UpdaterSummary,
    worker: Option<tokio::task::JoinHandle<()>>,
    finished: Option<Instant>,
}
struct Args {
    backend: Arc<dyn UpdaterBackend>,
    installer: Arc<dyn CoreUpdateInstaller>,
}
struct State {
    args: Args,
    manifest: ManifestVersion,
    mirror: Option<(String, Instant)>,
    tasks: HashMap<usize, Task>,
    next_id: usize,
    fetch: Option<tokio::task::JoinHandle<()>>,
    fetch_waiters: Vec<RpcReplyPort<Result<ManifestVersionLatest>>>,
    timer: tokio::task::JoinHandle<()>,
    closing: bool,
}
impl Drop for State {
    fn drop(&mut self) {
        self.timer.abort();
        if let Some(task) = self.fetch.take() {
            task.abort();
        }
        for task in self.tasks.values_mut() {
            if let Some(worker) = task.worker.take() {
                worker.abort();
            }
        }
    }
}
struct UpdaterActor;
impl Actor for UpdaterActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;
    async fn pre_start(
        &self,
        actor: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        let timer = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                if actor.cast(Message::Prune(Instant::now())).is_err() {
                    break;
                }
            }
        });
        Ok(State {
            args,
            manifest: ManifestVersion::default(),
            mirror: None,
            tasks: HashMap::new(),
            next_id: 0,
            fetch: None,
            fetch_waiters: Vec::new(),
            timer,
            closing: false,
        })
    }
    async fn handle(
        &self,
        actor: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Fetch(reply) => {
                if reply.is_closed() {
                    return Ok(());
                }
                if state.closing || state.fetch_waiters.len() >= MAX_TASKS {
                    let _ = reply.send(Err(anyhow!("updater is shutting down or busy")));
                    return Ok(());
                }
                state.fetch_waiters.push(reply);
                if state.fetch.is_none() {
                    let backend = state.args.backend.clone();
                    let mirror = state.mirror.clone();
                    state.fetch = Some(tokio::spawn(async move {
                        let result = AssertUnwindSafe(backend.fetch_manifest(mirror))
                            .catch_unwind()
                            .await
                            .unwrap_or_else(|_| Err(anyhow!("updater manifest worker panicked")));
                        let _ = actor.cast(Message::Fetched(Box::new(result)));
                    }));
                }
            }
            Message::Fetched(result) => {
                state.fetch.take();
                let result = (*result).map(|(manifest, mirror)| {
                    state.manifest = manifest;
                    state.mirror = Some(mirror);
                    state.manifest.latest.clone()
                });
                for reply in state.fetch_waiters.drain(..) {
                    let _ = reply.send(
                        result
                            .as_ref()
                            .cloned()
                            .map_err(|error| anyhow!("{error:#}")),
                    );
                }
            }
            Message::Start(core, reply) => {
                if reply.is_closed() {
                    return Ok(());
                }
                if state.closing {
                    let _ = reply.send(Err(anyhow!("updater is shutting down")));
                    return Ok(());
                }
                // A repeated request observes the admitted operation instead of downloading/installing twice.
                if let Some(task) = state
                    .tasks
                    .values()
                    .find(|task| task.core == core && task.finished.is_none())
                {
                    let _ = reply.send(Ok(task.summary.id));
                    return Ok(());
                }
                if state.tasks.len() >= MAX_TASKS {
                    let _ = reply.send(Err(anyhow!("too many retained updater tasks")));
                    return Ok(());
                }
                let Some((artifact, tag)) = state.manifest.get_matches(&core) else {
                    let _ = reply.send(Err(anyhow!(
                        "fetch latest versions before updating {core:?}"
                    )));
                    return Ok(());
                };
                let Some((mirror, _)) = state.mirror.clone() else {
                    let _ = reply.send(Err(anyhow!("updater mirror is unavailable")));
                    return Ok(());
                };
                state.next_id += 1;
                let id = state.next_id;
                let backend = state.args.backend.clone();
                let installer = state.args.installer.clone();
                let progress_actor = actor.clone();
                let progress = UpdaterProgress::new(move |status, download| {
                    let _ = progress_actor.cast(Message::Progress(id, status, download));
                });
                let worker = tokio::spawn(async move {
                    let result = AssertUnwindSafe(async {
                        let prepared = backend
                            .prepare(core, mirror, artifact, tag, progress)
                            .await?;
                        installer.install(prepared).await
                    })
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| Err(anyhow!("updater worker panicked")));
                    let _ = actor.cast(Message::Finished(id, result));
                });
                state.tasks.insert(
                    id,
                    Task {
                        core,
                        summary: UpdaterSummary {
                            id,
                            state: UpdaterState::Idle,
                            downloader: crate::core::download::DownloadStatus {
                                state: Default::default(),
                                downloaded: 0,
                                total: 0,
                                speed: 0.0,
                            },
                        },
                        worker: Some(worker),
                        finished: None,
                    },
                );
                let _ = reply.send(Ok(id));
            }
            Message::Inspect(id, reply) => {
                let _ = reply.send(
                    state
                        .tasks
                        .get(&id)
                        .map(|task| task.summary.clone())
                        .ok_or_else(|| anyhow!("updater does not exist")),
                );
            }
            Message::Progress(id, progress, download) => {
                if let Some(task) = state.tasks.get_mut(&id) {
                    // Terminal install notifications remain authoritative after an RPC timeout.
                    if matches!(task.summary.state, UpdaterState::Done) {
                        return Ok(());
                    }
                    if matches!(progress, UpdaterState::Done | UpdaterState::Failed(_)) {
                        task.finished = Some(Instant::now());
                    }
                    task.summary.state = progress;
                    if let Some(download) = download {
                        task.summary.downloader = download;
                    }
                }
            }
            Message::Finished(id, result) => {
                if let Some(task) = state.tasks.get_mut(&id) {
                    task.worker.take();
                    let pending = result
                        .as_ref()
                        .err()
                        .is_some_and(|error| error.is::<ports::InstallPending>())
                        && task.finished.is_none();
                    if !matches!(task.summary.state, UpdaterState::Done) {
                        task.summary.state = match result {
                            Ok(()) => UpdaterState::Done,
                            Err(error) if pending => UpdaterState::Pending(format!("{error:#}")),
                            Err(error) => UpdaterState::Failed(format!("{error:#}")),
                        };
                    }
                    if !pending {
                        task.finished = Some(Instant::now());
                    }
                }
            }
            Message::Prune(now) => {
                state.tasks.retain(|_, task| {
                    task.worker.is_some()
                        || task.finished.is_none_or(|finished| {
                            now.saturating_duration_since(finished) < RETENTION
                        })
                });
            }
            Message::Shutdown(reply) => {
                state.closing = true;
                state.timer.abort();
                if let Some(fetch) = state.fetch.take() {
                    fetch.abort();
                    let _ = fetch.await;
                }
                for waiter in state.fetch_waiters.drain(..) {
                    let _ = waiter.send(Err(anyhow!("updater is shutting down")));
                }
                for task in state.tasks.values_mut() {
                    if let Some(worker) = task.worker.take() {
                        worker.abort();
                        let _ = worker.await;
                    }
                    if task.finished.is_none() {
                        task.summary.state = UpdaterState::Failed("updater shut down".into());
                        task.finished = Some(Instant::now());
                    }
                }
                let _ = reply.send(Ok(()));
            }
        }
        Ok(())
    }
}
struct ClientInner(ActorRef<Message>);
impl Drop for ClientInner {
    fn drop(&mut self) {
        self.0.stop(None);
    }
}
#[derive(Clone)]
pub(crate) struct UpdaterClient(Arc<ClientInner>);
impl UpdaterClient {
    pub async fn spawn(
        backend: Arc<dyn UpdaterBackend>,
        installer: Arc<dyn CoreUpdateInstaller>,
    ) -> Result<Self> {
        let (actor, _) = Actor::spawn(None, UpdaterActor, Args { backend, installer }).await?;
        Ok(Self(Arc::new(ClientInner(actor))))
    }
    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<Result<T>>) -> Message,
    ) -> Result<T> {
        match self
            .0
            .0
            .call(message, Some(Duration::from_secs(120)))
            .await?
        {
            ractor::rpc::CallResult::Success(result) => result,
            ractor::rpc::CallResult::Timeout => Err(anyhow!(
                "updater request timed out; inspect admitted tasks before retrying"
            )),
            ractor::rpc::CallResult::SenderError => Err(anyhow!("updater is unavailable")),
        }
    }
    pub async fn fetch_latest(&self) -> Result<ManifestVersionLatest> {
        self.call(Message::Fetch).await
    }
    pub async fn update(&self, core: ClashCore) -> Result<usize> {
        self.call(|reply| Message::Start(core, reply)).await
    }
    pub async fn inspect(&self, id: usize) -> Result<UpdaterSummary> {
        self.call(|reply| Message::Inspect(id, reply)).await
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.call(Message::Shutdown).await
    }
}

#[cfg(test)]
mod tests;
