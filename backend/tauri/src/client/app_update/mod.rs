//! Backend-owned application update state and task lifecycle.

use std::{any::Any, sync::Arc};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use nyanpasu_config::application::{ReleaseChannel, UpdateSource};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, SupervisionEvent};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppUpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Cancelling,
    Cancelled,
    Verifying,
    Ready,
    Installing,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AppUpdateRelease {
    pub version: String,
    pub date: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AppUpdateSnapshot {
    pub revision: u64,
    pub phase: AppUpdatePhase,
    pub release: Option<AppUpdateRelease>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed: f64,
    pub source: Option<UpdateSource>,
    pub error: Option<String>,
    pub last_checked_at: Option<String>,
    pub supported: bool,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
pub struct AppUpdateStateChanged(pub AppUpdateSnapshot);

#[derive(Debug, Clone)]
pub struct AppUpdateSettings {
    pub channel: ReleaseChannel,
    pub sources: Vec<UpdateSource>,
    pub auto_check: bool,
    pub auto_download: bool,
}

/// Process-local updater context. This is intentionally not persisted; cached
/// package restoration can be added later behind a revalidation adapter.
#[derive(Clone)]
pub struct PreparedAppUpdate {
    pub release: AppUpdateRelease,
    /// Includes version, target, build identity, and signature for deduplication.
    pub identity: String,
    pub(crate) context: Arc<dyn Any + Send + Sync>,
}

#[derive(Debug, Clone)]
pub struct VerifiedAppUpdate {
    pub bytes: Arc<Vec<u8>>,
    pub source: UpdateSource,
}

#[derive(Debug, Clone, Copy)]
pub enum AppUpdateDownloadProgress {
    /// A download attempt started; progress and speed restart from zero.
    /// Sent exactly once per attempt.
    Attempt {
        source: UpdateSource,
    },
    /// Cumulative bytes of the current attempt.
    Chunk {
        downloaded: u64,
        total: Option<u64>,
    },
    Verifying,
}

#[async_trait]
pub trait AppUpdateBackend: Send + Sync + 'static {
    async fn check(
        &self,
        settings: &AppUpdateSettings,
        cancellation: CancellationToken,
    ) -> Result<Option<PreparedAppUpdate>>;
    async fn download(
        &self,
        update: PreparedAppUpdate,
        cancellation: CancellationToken,
        progress: Arc<dyn Fn(AppUpdateDownloadProgress) + Send + Sync>,
    ) -> Result<VerifiedAppUpdate>;
    async fn install(&self, update: PreparedAppUpdate, package: VerifiedAppUpdate) -> Result<()>;
}

pub type BackendFactory = dyn Fn(Arc<dyn crate::service::profile_file::SelfProxyPortSource>) -> Arc<dyn AppUpdateBackend>
    + Send
    + Sync;

pub trait AppUpdateEventSink: Send + Sync + 'static {
    fn publish(&self, snapshot: AppUpdateSnapshot);
}

pub(crate) mod adapters;
mod progress;

use progress::{ProgressSample, ProgressSampler, SAMPLE_INTERVAL};

pub struct NoopAppUpdateEventSink;
impl AppUpdateEventSink for NoopAppUpdateEventSink {
    fn publish(&self, _snapshot: AppUpdateSnapshot) {}
}

pub struct UnavailableAppUpdateBackend;
#[async_trait]
impl AppUpdateBackend for UnavailableAppUpdateBackend {
    async fn check(
        &self,
        _settings: &AppUpdateSettings,
        _cancellation: CancellationToken,
    ) -> Result<Option<PreparedAppUpdate>> {
        Err(anyhow!("application updater is unavailable"))
    }
    async fn download(
        &self,
        _update: PreparedAppUpdate,
        _cancellation: CancellationToken,
        _progress: Arc<dyn Fn(AppUpdateDownloadProgress) + Send + Sync>,
    ) -> Result<VerifiedAppUpdate> {
        Err(anyhow!("application updater is unavailable"))
    }
    async fn install(&self, _update: PreparedAppUpdate, _package: VerifiedAppUpdate) -> Result<()> {
        Err(anyhow!("application updater is unavailable"))
    }
}

#[derive(Clone)]
pub struct AppUpdateArgs {
    pub backend: Arc<dyn AppUpdateBackend>,
    pub events: Arc<dyn AppUpdateEventSink>,
    pub settings: AppUpdateSettings,
    pub supported: bool,
    pub endpoints: Vec<String>,
    pub shutdown: CancellationToken,
}

enum OperationResult {
    Checked(Result<Option<PreparedAppUpdate>>),
    Downloaded(Result<VerifiedAppUpdate>),
    Installed(Result<()>),
}

enum OperationMessage {
    Check(AppUpdateSettings, RpcReplyPort<OperationResult>),
    Download(PreparedAppUpdate, RpcReplyPort<OperationResult>),
    Install(
        PreparedAppUpdate,
        VerifiedAppUpdate,
        RpcReplyPort<OperationResult>,
    ),
}

struct OperationArgs {
    backend: Arc<dyn AppUpdateBackend>,
    cancellation: CancellationToken,
    parent: ActorRef<Message>,
    operation_id: u64,
}

struct OperationActor;

impl Actor for OperationActor {
    type Msg = OperationMessage;
    type State = OperationArgs;
    type Arguments = OperationArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<OperationMessage>,
        args: OperationArgs,
    ) -> Result<OperationArgs, ActorProcessingErr> {
        Ok(args)
    }

    async fn handle(
        &self,
        _myself: ActorRef<OperationMessage>,
        message: OperationMessage,
        state: &mut OperationArgs,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            OperationMessage::Check(settings, reply) => {
                let result = state
                    .backend
                    .check(&settings, state.cancellation.clone())
                    .await;
                let _ = reply.send(OperationResult::Checked(result));
            }
            OperationMessage::Download(update, reply) => {
                let parent = state.parent.clone();
                let operation_id = state.operation_id;
                // Guards only this download's sampler: the adapter records into it
                // from its callbacks while the tick below reads it.
                let sampler = Arc::new(std::sync::Mutex::new(ProgressSampler::new(
                    tokio::time::Instant::now(),
                )));
                let progress = Arc::new({
                    let sampler = sampler.clone();
                    let parent = parent.clone();
                    move |event| match event {
                        AppUpdateDownloadProgress::Attempt { source } => {
                            *sampler.lock().expect("progress sampler lock poisoned") =
                                ProgressSampler::new(tokio::time::Instant::now());
                            let _ = parent.cast(Message::Attempt(operation_id, source));
                        }
                        AppUpdateDownloadProgress::Chunk { downloaded, total } => sampler
                            .lock()
                            .expect("progress sampler lock poisoned")
                            .record(downloaded, total),
                        AppUpdateDownloadProgress::Verifying => {
                            let _ = parent.cast(Message::Verifying(operation_id));
                        }
                    }
                });
                let download = state
                    .backend
                    .download(update, state.cancellation.clone(), progress);
                tokio::pin!(download);
                // Progress is sampled on this task, so every message to the parent
                // leaves in order and an attempt reset cannot race a sample.
                let mut ticks = tokio::time::interval_at(
                    tokio::time::Instant::now() + SAMPLE_INTERVAL,
                    SAMPLE_INTERVAL,
                );
                ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                let result = loop {
                    tokio::select! {
                        result = &mut download => break result,
                        now = ticks.tick() => {
                            let sample = sampler
                                .lock()
                                .expect("progress sampler lock poisoned")
                                .sample(now);
                            let _ = parent.cast(Message::Progress(operation_id, sample));
                        }
                    }
                };
                let _ = reply.send(OperationResult::Downloaded(result));
            }
            OperationMessage::Install(update, package, reply) => {
                let result = state.backend.install(update, package).await;
                let _ = reply.send(OperationResult::Installed(result));
            }
        }
        Ok(())
    }
}

enum Message {
    Get(RpcReplyPort<Result<AppUpdateSnapshot>>),
    Configure(AppUpdateSettings),
    Check {
        automatic: bool,
        reply: Option<RpcReplyPort<Result<AppUpdateSnapshot>>>,
    },
    OperationFinished(u64, OperationResult),
    OperationPanicked(u64),
    OperationFailed(u64, ActorProcessingErr),
    Attempt(u64, UpdateSource),
    Progress(u64, ProgressSample),
    Verifying(u64),
    Download {
        automatic: bool,
        reply: Option<RpcReplyPort<Result<AppUpdateSnapshot>>>,
    },
    Cancel(RpcReplyPort<Result<AppUpdateSnapshot>>),
    Install(RpcReplyPort<Result<AppUpdateSnapshot>>),
    Discard(RpcReplyPort<Result<AppUpdateSnapshot>>),
}

struct Operation {
    id: u64,
    cancellation: CancellationToken,
    child: ActorRef<OperationMessage>,
    monitor: Option<tokio::task::JoinHandle<()>>,
    installing: bool,
    settings_revision: u64,
}

async fn supervise_operation(
    id: u64,
    child: ActorRef<OperationMessage>,
    child_task: tokio::task::JoinHandle<()>,
    parent: ActorRef<Message>,
    command: impl FnOnce(RpcReplyPort<OperationResult>) -> OperationMessage,
    failed: impl FnOnce(anyhow::Error) -> OperationResult,
) {
    let call_result = child.call(command, None).await;
    child.stop(None);
    match child_task.await {
        Ok(()) => match call_result {
            Ok(ractor::rpc::CallResult::Success(result)) => {
                let _ = parent.cast(Message::OperationFinished(id, result));
            }
            Ok(_) => {
                let _ = parent.cast(Message::OperationFinished(
                    id,
                    failed(anyhow!("application update operation owner stopped")),
                ));
            }
            Err(error) => {
                let _ = parent.cast(Message::OperationFinished(id, failed(anyhow!("{error}"))));
            }
        },
        Err(error) if error.is_panic() => {
            let _ = parent.cast(Message::OperationPanicked(id));
            std::panic::resume_unwind(error.into_panic());
        }
        Err(error) => {
            let _ = parent.cast(Message::OperationFailed(id, error.into()));
        }
    }
}

struct State {
    args: AppUpdateArgs,
    settings: AppUpdateSettings,
    settings_revision: u64,
    snapshot: AppUpdateSnapshot,
    offer: Option<PreparedAppUpdate>,
    package: Option<VerifiedAppUpdate>,
    operation: Option<Operation>,
    next_id: u64,
    cancelled_release: Option<String>,
    pending_auto_check: bool,
}

struct AppUpdateActor;

impl AppUpdateActor {
    fn update_snapshot(state: &mut State, change: impl FnOnce(&mut AppUpdateSnapshot)) {
        change(&mut state.snapshot);
        state.snapshot.revision = state.snapshot.revision.saturating_add(1);
        state.args.events.publish(state.snapshot.clone());
    }

    fn set_phase(state: &mut State, phase: AppUpdatePhase) {
        Self::update_snapshot(state, |snapshot| {
            snapshot.phase = phase;
            snapshot.error = None;
        });
    }

    /// Progress of an operation that ended or is being cancelled is stale.
    fn is_current_download(state: &State, id: u64) -> bool {
        state
            .operation
            .as_ref()
            .is_some_and(|operation| operation.id == id)
            && state.snapshot.phase != AppUpdatePhase::Cancelling
    }

    async fn start_operation(
        myself: ActorRef<Message>,
        state: &mut State,
        id: u64,
        cancellation: CancellationToken,
    ) -> Result<(ActorRef<OperationMessage>, tokio::task::JoinHandle<()>)> {
        let supervisor = myself.get_cell();
        let (child, task) = Actor::spawn_linked(
            None,
            OperationActor,
            OperationArgs {
                backend: state.args.backend.clone(),
                cancellation,
                parent: myself,
                operation_id: id,
            },
            supervisor,
        )
        .await?;
        Ok((child, task))
    }

    async fn start_check(
        myself: ActorRef<Message>,
        state: &mut State,
        automatic: bool,
        reply: Option<RpcReplyPort<Result<AppUpdateSnapshot>>>,
    ) -> Result<(), ActorProcessingErr> {
        if !state.snapshot.supported || state.args.shutdown.is_cancelled() {
            if let Some(reply) = reply {
                let _ = reply.send(Err(anyhow!(
                    "application updater is unsupported or shutting down"
                )));
            }
            return Ok(());
        }
        if state.operation.is_some() {
            if let Some(reply) = reply {
                let _ = reply.send(Err(anyhow!("application update operation is busy")));
            }
            return Ok(());
        }
        if !automatic {
            state.cancelled_release = None;
        }
        let id = state.next_id.saturating_add(1);
        state.next_id = id;
        let cancellation = state.args.shutdown.child_token();
        let (child, child_task) =
            Self::start_operation(myself.clone(), state, id, cancellation.clone()).await?;
        let settings = state.settings.clone();
        let monitor_actor = myself.clone();
        let monitor = tokio::spawn(supervise_operation(
            id,
            child.clone(),
            child_task,
            monitor_actor,
            move |reply| OperationMessage::Check(settings, reply),
            |error| OperationResult::Checked(Err(error)),
        ));
        state.operation = Some(Operation {
            id,
            cancellation,
            child,
            monitor: Some(monitor),
            installing: false,
            settings_revision: state.settings_revision,
        });
        Self::set_phase(state, AppUpdatePhase::Checking);
        if let Some(reply) = reply {
            let _ = reply.send(Ok(state.snapshot.clone()));
        }
        Ok(())
    }

    async fn start_download(
        myself: ActorRef<Message>,
        state: &mut State,
        automatic: bool,
        reply: Option<RpcReplyPort<Result<AppUpdateSnapshot>>>,
    ) -> Result<(), ActorProcessingErr> {
        if !state.snapshot.supported || state.args.shutdown.is_cancelled() {
            if let Some(reply) = reply {
                let _ = reply.send(Err(anyhow!(
                    "application updater is unsupported or shutting down"
                )));
            }
            return Ok(());
        }
        if state.operation.is_some() {
            if let Some(reply) = reply {
                let _ = reply.send(Err(anyhow!("application update operation is busy")));
            }
            return Ok(());
        }
        let Some(offer) = state.offer.clone() else {
            if let Some(reply) = reply {
                let _ = reply.send(Err(anyhow!("check for an application update first")));
            }
            return Ok(());
        };
        if state.package.is_some() {
            if let Some(reply) = reply {
                let _ = reply.send(Ok(state.snapshot.clone()));
            }
            return Ok(());
        }
        if automatic && state.cancelled_release.as_deref() == Some(&offer.identity) {
            if let Some(reply) = reply {
                let _ = reply.send(Ok(state.snapshot.clone()));
            }
            return Ok(());
        }
        if !automatic {
            state.cancelled_release = None;
        }
        let id = state.next_id.saturating_add(1);
        state.next_id = id;
        let cancellation = state.args.shutdown.child_token();
        let (child, child_task) =
            Self::start_operation(myself.clone(), state, id, cancellation.clone()).await?;
        let monitor = tokio::spawn(supervise_operation(
            id,
            child.clone(),
            child_task,
            myself.clone(),
            move |reply| OperationMessage::Download(offer, reply),
            |error| OperationResult::Downloaded(Err(error)),
        ));
        state.operation = Some(Operation {
            id,
            cancellation,
            child,
            monitor: Some(monitor),
            installing: false,
            settings_revision: state.settings_revision,
        });
        Self::update_snapshot(state, |snapshot| {
            snapshot.phase = AppUpdatePhase::Downloading;
            snapshot.downloaded = 0;
            snapshot.total = None;
            snapshot.speed = 0.0;
            snapshot.error = None;
        });
        if let Some(reply) = reply {
            let _ = reply.send(Ok(state.snapshot.clone()));
        }
        Ok(())
    }
}

impl Actor for AppUpdateActor {
    type Msg = Message;
    type State = State;
    type Arguments = AppUpdateArgs;

    async fn pre_start(
        &self,
        myself: ActorRef<Message>,
        args: AppUpdateArgs,
    ) -> Result<State, ActorProcessingErr> {
        let settings = args.settings.clone();
        let state = State {
            snapshot: AppUpdateSnapshot {
                revision: 0,
                phase: AppUpdatePhase::Idle,
                release: None,
                downloaded: 0,
                total: None,
                speed: 0.0,
                source: None,
                error: None,
                last_checked_at: None,
                supported: args.supported,
                endpoints: args.endpoints.clone(),
            },
            args,
            settings,
            settings_revision: 0,
            offer: None,
            package: None,
            operation: None,
            next_id: 0,
            cancelled_release: None,
            pending_auto_check: false,
        };
        state.args.events.publish(state.snapshot.clone());
        if state.settings.auto_check {
            let _ = myself.cast(Message::Check {
                automatic: true,
                reply: None,
            });
        }
        Ok(state)
    }

    async fn handle(
        &self,
        myself: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Get(reply) => {
                let _ = reply.send(Ok(state.snapshot.clone()));
            }
            Message::Configure(settings) => {
                let identity_changed = state.settings.channel != settings.channel
                    || state.settings.sources != settings.sources;
                let changed = identity_changed
                    || state.settings.auto_check != settings.auto_check
                    || state.settings.auto_download != settings.auto_download;
                if !changed {
                    return Ok(());
                }
                let auto_check = settings.auto_check;
                let installing = state
                    .operation
                    .as_ref()
                    .is_some_and(|operation| operation.installing);
                state.settings = settings;
                let channel = state.settings.channel;
                if identity_changed && !installing {
                    state.settings_revision = state.settings_revision.saturating_add(1);
                }
                Self::update_snapshot(state, |snapshot| {
                    snapshot.endpoints = crate::bundle::update_endpoints(channel);
                });
                if identity_changed && !installing {
                    state.offer = None;
                    state.package = None;
                    Self::update_snapshot(state, |snapshot| {
                        snapshot.release = None;
                        snapshot.source = None;
                        snapshot.downloaded = 0;
                        snapshot.total = None;
                        snapshot.speed = 0.0;
                        if snapshot.phase != AppUpdatePhase::Installing {
                            snapshot.phase = AppUpdatePhase::Idle;
                        }
                    });
                    if let Some(operation) = state.operation.as_ref() {
                        operation.cancellation.cancel();
                        state.pending_auto_check = auto_check;
                        Self::set_phase(state, AppUpdatePhase::Cancelling);
                    } else if auto_check {
                        Self::start_check(myself.clone(), state, true, None).await?;
                    }
                } else if auto_check && state.operation.is_none() {
                    Self::start_check(myself.clone(), state, true, None).await?;
                }
            }
            Message::Check { automatic, reply } => {
                Self::start_check(myself.clone(), state, automatic, reply).await?;
            }
            Message::OperationFinished(id, result) => {
                let Some(operation) = state.operation.as_ref() else {
                    return Ok(());
                };
                if operation.id != id {
                    return Ok(());
                }
                let stale = operation.settings_revision != state.settings_revision;
                let cancelled = operation.cancellation.is_cancelled();
                let installing = operation.installing;
                state.operation = None;
                match result {
                    OperationResult::Checked(result) => {
                        if stale {
                            Self::set_phase(state, AppUpdatePhase::Idle);
                        } else {
                            match result {
                                Ok(Some(offer)) => {
                                    let same_release =
                                        state.offer.as_ref().is_some_and(|previous| {
                                            previous.identity == offer.identity
                                        });
                                    if !same_release {
                                        state.package = None;
                                    }
                                    let release = offer.release.clone();
                                    state.offer = Some(offer);
                                    let package_ready = same_release && state.package.is_some();
                                    let package_size =
                                        state.package.as_ref().map(|p| p.bytes.len() as u64);
                                    let package_source = state.package.as_ref().map(|p| p.source);
                                    Self::update_snapshot(state, |snapshot| {
                                        snapshot.release = Some(release);
                                        snapshot.phase = if package_ready {
                                            AppUpdatePhase::Ready
                                        } else {
                                            AppUpdatePhase::Available
                                        };
                                        snapshot.downloaded = package_size.unwrap_or(0);
                                        snapshot.total = package_size;
                                        snapshot.source = package_source;
                                        snapshot.error = None;
                                        snapshot.last_checked_at = Some(now_rfc3339());
                                    });
                                    if state.settings.auto_download
                                        && state.settings.auto_check
                                        && state.cancelled_release.as_deref()
                                            != state
                                                .offer
                                                .as_ref()
                                                .map(|offer| offer.identity.as_str())
                                        && state.package.is_none()
                                    {
                                        Self::start_download(myself.clone(), state, true, None)
                                            .await?;
                                    }
                                }
                                Ok(None) => {
                                    state.offer = None;
                                    state.package = None;
                                    Self::update_snapshot(state, |snapshot| {
                                        snapshot.release = None;
                                        snapshot.phase = AppUpdatePhase::UpToDate;
                                        snapshot.downloaded = 0;
                                        snapshot.total = None;
                                        snapshot.speed = 0.0;
                                        snapshot.source = None;
                                        snapshot.error = None;
                                        snapshot.last_checked_at = Some(now_rfc3339());
                                    });
                                }
                                Err(error) => {
                                    if state.package.is_some() {
                                        Self::update_snapshot(state, |snapshot| {
                                            snapshot.phase = AppUpdatePhase::Ready;
                                            snapshot.error = Some(format!("{error:#}"));
                                            snapshot.last_checked_at = Some(now_rfc3339());
                                        });
                                    } else {
                                        state.offer = None;
                                        Self::update_snapshot(state, |snapshot| {
                                            snapshot.phase = AppUpdatePhase::Failed;
                                            snapshot.release = None;
                                            snapshot.source = None;
                                            snapshot.downloaded = 0;
                                            snapshot.total = None;
                                            snapshot.speed = 0.0;
                                            snapshot.error = Some(format!("{error:#}"));
                                            snapshot.last_checked_at = Some(now_rfc3339());
                                        });
                                    }
                                }
                            }
                        }
                        if state.pending_auto_check {
                            state.pending_auto_check = false;
                            if state.settings.auto_check {
                                Self::start_check(myself.clone(), state, true, None).await?;
                            }
                        }
                    }
                    OperationResult::Downloaded(result) => {
                        if stale || cancelled {
                            if !stale
                                && !installing
                                && let Some(offer) = &state.offer
                            {
                                state.cancelled_release = Some(offer.identity.clone());
                            }
                            Self::update_snapshot(state, |snapshot| {
                                snapshot.phase = if stale {
                                    AppUpdatePhase::Idle
                                } else {
                                    AppUpdatePhase::Cancelled
                                };
                                snapshot.downloaded = 0;
                                snapshot.total = None;
                                snapshot.speed = 0.0;
                                snapshot.error = None;
                            });
                        } else {
                            match result {
                                Ok(package) => {
                                    let source = package.source;
                                    let size = package.bytes.len() as u64;
                                    state.package = Some(package);
                                    Self::update_snapshot(state, |snapshot| {
                                        snapshot.phase = AppUpdatePhase::Ready;
                                        snapshot.source = Some(source);
                                        snapshot.downloaded = size;
                                        snapshot.total = Some(size);
                                        snapshot.speed = 0.0;
                                        snapshot.error = None;
                                    });
                                }
                                Err(error) => Self::update_snapshot(state, |snapshot| {
                                    snapshot.phase = AppUpdatePhase::Failed;
                                    snapshot.error = Some(format!("{error:#}"));
                                }),
                            }
                        }
                        if state.pending_auto_check {
                            state.pending_auto_check = false;
                            if state.settings.auto_check {
                                Self::start_check(myself.clone(), state, true, None).await?;
                            }
                        }
                    }
                    OperationResult::Installed(result) => {
                        if !installing {
                            return Ok(());
                        }
                        match result {
                            Ok(()) => Self::set_phase(state, AppUpdatePhase::Idle),
                            Err(error) => Self::update_snapshot(state, |snapshot| {
                                snapshot.phase = AppUpdatePhase::Ready;
                                snapshot.error = Some(format!("{error:#}"));
                            }),
                        }
                    }
                }
            }
            Message::OperationPanicked(id) => {
                if state
                    .operation
                    .as_ref()
                    .is_some_and(|operation| operation.id == id)
                {
                    panic!("application update operation child {id} panicked");
                }
            }
            Message::OperationFailed(id, error) => {
                if state
                    .operation
                    .as_ref()
                    .is_some_and(|operation| operation.id == id)
                {
                    return Err(error);
                }
            }
            Message::Attempt(id, source) => {
                if !Self::is_current_download(state, id) {
                    return Ok(());
                }
                Self::update_snapshot(state, |snapshot| {
                    if matches!(
                        snapshot.phase,
                        AppUpdatePhase::Downloading | AppUpdatePhase::Verifying
                    ) {
                        snapshot.phase = AppUpdatePhase::Downloading;
                        snapshot.source = Some(source);
                        snapshot.downloaded = 0;
                        snapshot.total = None;
                        snapshot.speed = 0.0;
                    }
                });
            }
            Message::Progress(id, sample) => {
                if !Self::is_current_download(state, id)
                    || state.snapshot.phase != AppUpdatePhase::Downloading
                {
                    return Ok(());
                }
                Self::update_snapshot(state, |snapshot| {
                    snapshot.downloaded = sample.downloaded;
                    snapshot.total = sample.total;
                    snapshot.speed = sample.speed;
                });
            }
            Message::Verifying(id) => {
                if !Self::is_current_download(state, id)
                    || state.snapshot.phase != AppUpdatePhase::Downloading
                {
                    return Ok(());
                }
                Self::update_snapshot(state, |snapshot| {
                    snapshot.phase = AppUpdatePhase::Verifying;
                    snapshot.speed = 0.0;
                    snapshot.error = None;
                });
            }
            Message::Download { automatic, reply } => {
                Self::start_download(myself.clone(), state, automatic, reply).await?;
            }
            Message::Cancel(reply) => match state.operation.as_ref() {
                Some(operation)
                    if !operation.installing
                        && matches!(
                            state.snapshot.phase,
                            AppUpdatePhase::Downloading
                                | AppUpdatePhase::Cancelling
                                | AppUpdatePhase::Verifying
                        ) =>
                {
                    operation.cancellation.cancel();
                    Self::set_phase(state, AppUpdatePhase::Cancelling);
                    let _ = reply.send(Ok(state.snapshot.clone()));
                }
                _ => {
                    let _ = reply.send(Err(anyhow!("no cancellable application update download")));
                }
            },
            Message::Install(reply) => {
                if state.args.shutdown.is_cancelled() {
                    let _ = reply.send(Err(anyhow!("application is shutting down")));
                    return Ok(());
                }
                if state.operation.is_some() {
                    let _ = reply.send(Err(anyhow!("application update operation is busy")));
                    return Ok(());
                }
                if state.snapshot.phase != AppUpdatePhase::Ready {
                    let _ = reply.send(Err(anyhow!(
                        "application update package is not ready to install"
                    )));
                    return Ok(());
                }
                let (Some(offer), Some(package)) = (state.offer.clone(), state.package.clone())
                else {
                    let _ = reply.send(Err(anyhow!(
                        "no verified application update package is ready"
                    )));
                    return Ok(());
                };
                let id = state.next_id.saturating_add(1);
                state.next_id = id;
                let cancellation = state.args.shutdown.child_token();
                let (child, child_task) =
                    Self::start_operation(myself.clone(), state, id, cancellation.clone()).await?;
                let monitor = tokio::spawn(supervise_operation(
                    id,
                    child.clone(),
                    child_task,
                    myself.clone(),
                    move |reply| OperationMessage::Install(offer, package, reply),
                    |error| OperationResult::Installed(Err(error)),
                ));
                state.operation = Some(Operation {
                    id,
                    cancellation,
                    child,
                    monitor: Some(monitor),
                    installing: true,
                    settings_revision: state.settings_revision,
                });
                Self::set_phase(state, AppUpdatePhase::Installing);
                let _ = reply.send(Ok(state.snapshot.clone()));
            }
            Message::Discard(reply) => {
                if state.operation.is_some() || state.args.shutdown.is_cancelled() {
                    let _ = reply.send(Err(anyhow!(
                        "application update operation is busy or shutting down"
                    )));
                } else {
                    state.offer = None;
                    state.package = None;
                    Self::update_snapshot(state, |snapshot| {
                        snapshot.phase = AppUpdatePhase::Idle;
                        snapshot.release = None;
                        snapshot.source = None;
                        snapshot.downloaded = 0;
                        snapshot.total = None;
                        snapshot.speed = 0.0;
                        snapshot.error = None;
                    });
                    let _ = reply.send(Ok(state.snapshot.clone()));
                }
            }
        }
        Ok(())
    }

    async fn handle_supervisor_evt(
        &self,
        _myself: ActorRef<Message>,
        event: SupervisionEvent,
        _state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        // Ordinary operation failures are returned through typed replies. An
        // actor failure is a lifecycle/invariant failure and stops this owner.
        if let SupervisionEvent::ActorFailed(_, error) = event {
            return Err(error);
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        actor: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(operation) = state.operation.take() {
            if operation.installing {
                // The platform installer can shut down this owner from its exit
                // hook. Its handoff must finish independently: keeping it linked
                // would let parent termination kill the install future before
                // the adapter's restart/recovery code runs.
                operation.child.unlink(actor.get_cell());
                drop(operation.monitor);
            } else {
                operation.cancellation.cancel();
                if let Some(monitor) = operation.monitor
                    && let Err(error) = monitor.await
                    && let Ok(panic) = error.try_into_panic()
                {
                    std::panic::resume_unwind(panic);
                }
            }
            operation.child.stop(None);
        }
        Ok(())
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

struct Inner(ActorRef<Message>);
impl Drop for Inner {
    fn drop(&mut self) {
        self.0.stop(None);
    }
}

#[derive(Clone)]
pub struct AppUpdateClient(Arc<Inner>);

impl AppUpdateClient {
    pub async fn spawn(args: AppUpdateArgs, tasks: &TaskTracker) -> Result<Self> {
        let (actor, _) = Actor::spawn(None, AppUpdateActor, args.clone()).await?;
        crate::client::drain_on_shutdown(tasks, args.shutdown, actor.get_cell());
        Ok(Self(Arc::new(Inner(actor))))
    }

    async fn call(
        &self,
        make: impl FnOnce(RpcReplyPort<Result<AppUpdateSnapshot>>) -> Message,
    ) -> Result<AppUpdateSnapshot> {
        match self.0.0.call(make, None).await? {
            ractor::rpc::CallResult::Success(result) => result,
            _ => Err(anyhow!("application updater is unavailable")),
        }
    }

    pub async fn state(&self) -> Result<AppUpdateSnapshot> {
        self.call(Message::Get).await
    }
    pub async fn check(&self) -> Result<AppUpdateSnapshot> {
        self.call(|reply| Message::Check {
            automatic: false,
            reply: Some(reply),
        })
        .await
    }
    pub async fn download(&self) -> Result<AppUpdateSnapshot> {
        self.call(|reply| Message::Download {
            automatic: false,
            reply: Some(reply),
        })
        .await
    }
    pub async fn cancel_download(&self) -> Result<AppUpdateSnapshot> {
        self.call(Message::Cancel).await
    }
    pub async fn install(&self) -> Result<AppUpdateSnapshot> {
        self.call(Message::Install).await
    }
    pub async fn discard(&self) -> Result<AppUpdateSnapshot> {
        self.call(Message::Discard).await
    }
    pub fn configure(&self, settings: AppUpdateSettings) {
        let _ = self.0.0.cast(Message::Configure(settings));
    }
}

#[cfg(test)]
mod tests;
