//! ProfilesActor: single owner of the profiles document.
//! Tauri-free; every filesystem/network effect goes through the ports.

use std::{collections::HashMap, sync::Arc, time::Duration};

use nyanpasu_config::profile::{
    ConfigDefinition, ExternalMode, ExternalProfilePath, FileConfig, LocalBinding,
    ManagedProfilePath, MaterializedFile, ProfileDefinition, ProfileDependencyIndex, ProfileId,
    ProfileItem, ProfileMetadata, ProfileMetadataPatch, ProfileRevisionError, ProfileSource,
    ProfileValidationError, Profiles, RemoteProfileOptions, RemoteProfileOptionsPatch,
    ScriptRuntime, SubscriptionInfo, TransformDefinition,
};
use nyanpasu_core::state::{PersistentStateManager, ReplaceIfVersionResult, Version};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use crate::{
    client::application_workflow::{
        impact::{
            ActivationIntent, ContentDigest, MutationHints, RequestedRuntimeFields,
            StagedResourceToken, TouchedContent,
        },
        policy::CommandClass,
    },
    core::migration::modules::profiles::ProfilesFormat,
    state::mutation::MutationCoordinator,
};
use futures::FutureExt as _;
use nyanpasu_core_manager::OperationId;
use tokio::{sync::watch, task::JoinHandle};

use super::{
    ports::{
        MaterializationReconcileReport, MaterializationResource, PreparedCleanup,
        ProfileDegradation, ProfileDegradationCode, ProfileDegradationPhase, ProfileFsPort,
        ProfileMaterializationPort, SubscriptionFetcher,
    },
    scheduler::{ExternalWatchers, RemoteUpdateScheduler},
    sources::{SourceLedger, SourceOrigin, SourceOutcome, SourcesSnapshot},
};

/// Actor-owned recovery pass over durable materialization/cleanup journals.
/// Background work only casts [`ProfilesActorMessage::ReconcileMaterializations`];
/// the actor performs the blocking reconcile under message serialization.
const MATERIALIZATION_RECONCILE_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// How long `StopProducers` waits for aborted downloads to finish (T10 §2.2).
const DOWNLOAD_STOP_BOUND: Duration = Duration::from_secs(1);

#[derive(Debug, thiserror::Error)]
pub enum ProfilesError {
    #[error("profile not found: {0}")]
    ProfileNotFound(ProfileId),
    #[error(
        "profile is referenced and cannot be deleted (referrers: {referrers:?}, current: {current}, global_transforms: {global_transforms})"
    )]
    ProfileInUse {
        referrers: Vec<ProfileId>,
        /// Referenced by the document-level `current` selection.
        current: bool,
        /// Referenced by the document-level `global_transforms` list.
        global_transforms: bool,
    },
    #[error("profile has no materialized file")]
    ProfileHasNoFile,
    #[error("validation failed: {0:?}")]
    ValidationFailed(Vec<ProfileValidationError>),
    #[error("invalid reorder list: {reason}")]
    InvalidReorderList { reason: String },
    #[error("profile is not a remote profile")]
    NotARemoteProfile,
    #[error("file not writable: {reason}")]
    FileNotWritable { reason: String },
    #[error("refresh failed: {message}")]
    RefreshFailed { message: String },
    #[error("import failed: {message}")]
    ImportFailed { message: String },
    #[error("failed to persist profiles: {0}")]
    Persist(String),
    #[error("profiles source transaction failed: {source}; staging cleanup: {cleanup:?}")]
    SourceTransaction {
        #[source]
        source: nyanpasu_core::state::ReplaceIfVersionError,
        cleanup: Option<String>,
    },
    #[error("profiles state version conflict: expected {expected}, actual {actual}")]
    VersionConflict { expected: u64, actual: u64 },
    #[error(
        "profiles state version conflict: expected {expected}, actual {actual}; staging cleanup failed: {cleanup}"
    )]
    ConflictCleanup {
        expected: u64,
        actual: u64,
        cleanup: String,
    },
    #[error("failed to advance profile revision: {0}")]
    Revision(#[from] ProfileRevisionError),
    #[error("profile materialization failed: {0}")]
    Materialization(String),
    #[error("profiles actor rpc failed: {0}")]
    Rpc(String),
    #[error("application is shutting down")]
    ShuttingDown,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CommitReport {
    pub snapshot: Arc<Profiles>,
    /// Dependency-closure judgement per the T04 affects_current rule table.
    pub affects_current: bool,
    /// Crate-internal detail for committed mutations that left maintenance work.
    pub(crate) degradations: Vec<ProfileDegradation>,
    pub(crate) receipt: crate::client::runtime::CommitReceipt,
    pub(crate) runtime_degradations: Vec<crate::client::runtime::Degradation>,
    /// Server-generated uid (D13); set by Add / import commit, consumed by
    /// facade auto-activation (design §9).
    pub created: Option<ProfileId>,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct NewProfileRequest {
    pub metadata: ProfileMetadata,
    /// Add rewrites the materialized path to `{uid}.{ext}`.
    pub definition: ProfileDefinition,
}

#[derive(Debug, Clone)]
pub enum ReorderOp {
    Move { active: ProfileId, over: ProfileId },
    ByList(Vec<ProfileId>),
}

pub struct ProfilesActorArgs {
    pub(crate) mutations: MutationCoordinator,
    pub manager: PersistentStateManager<Profiles, ProfilesFormat>,
    pub fs: Arc<dyn ProfileFsPort>,
    pub fetcher: Arc<dyn SubscriptionFetcher>,
    pub(crate) materialization: Arc<dyn ProfileMaterializationPort>,
    pub(crate) sources: watch::Sender<SourcesSnapshot>,
}

pub struct ProfilesActorState {
    mutations: MutationCoordinator,
    manager: PersistentStateManager<Profiles, ProfilesFormat>,
    index: ProfileDependencyIndex,
    fs: Arc<dyn ProfileFsPort>,
    fetcher: Arc<dyn SubscriptionFetcher>,
    materialization: Arc<dyn ProfileMaterializationPort>,
    pending_refresh: HashMap<ProfileId, PendingRefresh>,
    next_refresh_token: u64,
    pending_imports: HashMap<ImportOperationToken, PendingImport>,
    next_import_token: u64,
    gate: ProducerGate,
    scheduler: RemoteUpdateScheduler,
    external_watchers: ExternalWatchers,
    /// Periodic journal recovery. Background task only casts; actor owns work.
    reconcile_task: Option<JoinHandle<()>>,
    sources: SourceLedger,
}

/// Whether background producers may run (T10 §2.2). Setup holds them until
/// StartupReconcile has proven the runtime; shutdown stops them for good.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProducerGate {
    Held,
    Running,
    Stopped,
}

/// Names one refresh download, so a completion can only settle the attempt
/// that started it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RefreshAttemptToken(u64);

#[cfg(test)]
impl RefreshAttemptToken {
    /// The token of the `n`th refresh an actor starts, counting from 1.
    pub(crate) fn nth(n: u64) -> Self {
        Self(n)
    }
}

struct PendingRefresh {
    token: RefreshAttemptToken,
    origin: RefreshOrigin,
    reply: Option<RpcReplyPort<Result<CommitReport, ProfilesError>>>,
    task: JoinHandle<()>,
}

/// In-memory handle for one fetch-before-commit import. Never durable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ImportOperationToken(u64);

struct PendingImport {
    reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    metadata: ProfileMetadata,
    url: url::Url,
    option: RemoteProfileOptions,
    update_interval_explicit: bool,
    task: JoinHandle<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefreshOrigin {
    Manual,
    Scheduled,
}

impl RefreshOrigin {
    fn source(self) -> SourceOrigin {
        match self {
            Self::Manual => SourceOrigin::ManualRefresh,
            Self::Scheduled => SourceOrigin::ScheduledRefresh,
        }
    }
}

/// What `StopProducers` cut short.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ProducersStopped {
    pub refreshes: usize,
    pub imports: usize,
    /// Aborted downloads that had not finished within the bound.
    pub unfinished: usize,
}

/// How a refresh attempt that reached its commit handler ended.
enum RefreshConclusion {
    Committed(CommitReport),
    Superseded(String),
    Failed(String),
    Rejected(ProfilesError),
}

#[derive(Debug)]
pub enum RefreshOutcome {
    Succeeded {
        subscription: nyanpasu_config::profile::SubscriptionInfo,
        suggested_update_interval_minutes: Option<u64>,
        /// Validated payload; written to the materialized file inside the
        /// commit handler so a stale download can be fenced before any write.
        content: String,
        /// Server-provided display name (`profile-title` / `Content-Disposition`),
        /// applied to a non-user-named profile by name-sync in the commit handler.
        filename: Option<String>,
    },
    Failed {
        message: String,
    },
}

/// Decide the profile name to apply after a refresh. Returns `Some(name)` only
/// when the profile is not user-named and the server supplied a non-blank name;
/// otherwise the current name is kept. Pure so the provenance rule is unit-tested
/// without spawning the actor.
fn synced_name(custom_name: bool, filename: &Option<String>) -> Option<String> {
    if custom_name {
        return None;
    }
    filename
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ProfilesActorMessage {
    SaveFile {
        uid: ProfileId,
        content: String,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    SetCurrent {
        current: Option<ProfileId>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    /// Activate `uid` only if nothing is currently selected. The reply is
    /// `Some(report)` when it activated, `None` when a current already existed.
    SetCurrentIfNone {
        uid: ProfileId,
        reply: RpcReplyPort<Result<Option<CommitReport>, ProfilesError>>,
    },
    SetGlobalTransforms {
        ids: Vec<ProfileId>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    SetValidFields {
        fields: Vec<String>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Replace {
        profiles: Profiles,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Add {
        request: NewProfileRequest,
        initial_file: Option<String>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Delete {
        uid: ProfileId,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Reorder {
        op: ReorderOp,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    PatchMetadata {
        uid: ProfileId,
        patch: ProfileMetadataPatch,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    PatchRemoteOptions {
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    RefreshRemote {
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
        origin: RefreshOrigin,
        reply: Option<RpcReplyPort<Result<CommitReport, ProfilesError>>>,
    },
    CommitRefreshed {
        uid: ProfileId,
        token: RefreshAttemptToken,
        /// The URL and serialized definition fingerprint the download started
        /// for. Commit is discarded if either stale fence changed in flight.
        url: url::Url,
        definition_fingerprint: String,
        outcome: RefreshOutcome,
    },
    /// Fetch-before-commit remote import. No durable placeholder is written
    /// until download + validation succeed and the caller is still live.
    ImportRemote {
        url: url::Url,
        metadata: ProfileMetadata,
        option: RemoteProfileOptions,
        update_interval_explicit: bool,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    CommitImported {
        token: ImportOperationToken,
        outcome: RefreshOutcome,
    },
    ExternalFileChanged {
        uid: ProfileId,
    },
    ReplaceDefinition {
        uid: ProfileId,
        definition: ProfileDefinition,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    /// Recover durable materialization/cleanup journals against the committed
    /// profiles snapshot. Cast-only from the actor-owned periodic task and
    /// handled serially with all other mutations.
    ReconcileMaterializations,
    /// Arms the refresh scheduler (with catch-up), the external watchers and
    /// the materialization ticker. Only the first one after Held counts.
    StartProducers,
    /// Stops every producer, aborts pending downloads and refuses new
    /// refreshes and imports from then on.
    StopProducers {
        reply: RpcReplyPort<ProducersStopped>,
    },
}

pub struct ProfilesActor;

pub(super) enum AffectsRule {
    Never,
    CurrentChanged,
    GlobalChanged,
    Touched(ProfileId),
    Always,
}

impl ProfilesActor {
    fn current_state(state: &ProfilesActorState) -> Profiles {
        state.manager.snapshot_handle().load().state.clone()
    }

    pub(crate) fn current_closure(profiles: &Profiles) -> indexmap::IndexSet<ProfileId> {
        let mut closure: indexmap::IndexSet<ProfileId> =
            profiles.global_transforms.iter().cloned().collect();
        let Some(current) = &profiles.current else {
            return closure;
        };

        closure.insert(current.clone());
        let mut configs = vec![current.clone()];
        if let Some(item) = profiles.items.get(current)
            && let ProfileDefinition::Config {
                config: ConfigDefinition::Composition(composition),
            } = &item.definition
        {
            if let Some(base) = &composition.base {
                closure.insert(base.clone());
                configs.push(base.clone());
            }
            for member in &composition.extend_proxies_from {
                closure.insert(member.clone());
                configs.push(member.clone());
            }
        }

        for config in configs {
            if let Some(item) = profiles.items.get(&config)
                && let ProfileDefinition::Config { config } = &item.definition
            {
                for transform in config.transforms() {
                    closure.insert(transform.clone());
                }
            }
        }

        closure
    }

    fn evaluate_affects(rule: &AffectsRule, before: &Profiles, after: &Profiles) -> bool {
        match rule {
            AffectsRule::Never => false,
            AffectsRule::Always => true,
            AffectsRule::CurrentChanged => before.current != after.current,
            AffectsRule::GlobalChanged => before.global_transforms != after.global_transforms,
            AffectsRule::Touched(uid) => {
                let closure_before = Self::current_closure(before);
                let closure_after = Self::current_closure(after);
                closure_before != closure_after
                    || closure_before.contains(uid)
                    || closure_after.contains(uid)
            }
        }
    }

    fn prepare_candidate(mut next: Profiles) -> Result<Profiles, ProfilesError> {
        next.validate().map_err(ProfilesError::ValidationFailed)?;
        next.bump_revision()?;
        Ok(next)
    }

    /// Persist an already validated, revision-bumped candidate exactly as prepared.
    async fn persist_candidate(
        state: &mut ProfilesActorState,
        expected_version: Version,
        next: Profiles,
        hints: MutationHints,
        class: CommandClass,
    ) -> Result<
        (
            Arc<Profiles>,
            (
                crate::client::runtime::CommitReceipt,
                Vec<crate::client::runtime::Degradation>,
            ),
        ),
        ProfilesError,
    > {
        let operation = OperationId::generate();
        let participant = state
            .mutations
            .participant(operation, hints, class)
            .map_err(|error| ProfilesError::Persist(error.to_string()))?;
        match state
            .manager
            .replace_if_version_with_participant(
                expected_version,
                next.clone(),
                participant,
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await
            .map_err(|source| ProfilesError::SourceTransaction {
                source,
                cleanup: None,
            })? {
            ReplaceIfVersionResult::Replaced => {
                let completion = state
                    .mutations
                    .finish(
                        operation,
                        "profiles",
                        *state.manager.snapshot_handle().load().version.as_ref(),
                    )
                    .await;
                Ok((Arc::new(next), completion))
            }
            ReplaceIfVersionResult::Conflict { actual_version } => {
                Err(ProfilesError::VersionConflict {
                    expected: *expected_version.as_ref(),
                    actual: *actual_version.as_ref(),
                })
            }
        }
    }

    fn reconcile_committed(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        snapshot: &Profiles,
    ) {
        state.index = ProfileDependencyIndex::build(snapshot);
        state.sources.reconcile(snapshot);
        if state.gate == ProducerGate::Running {
            state.scheduler.reconcile(snapshot, myself, false);
            state.external_watchers.reconcile(snapshot, myself);
        }
    }

    fn start_producers(myself: &ActorRef<ProfilesActorMessage>, state: &mut ProfilesActorState) {
        if state.gate != ProducerGate::Held {
            return;
        }
        state.gate = ProducerGate::Running;
        let snapshot = Self::current_state(state);
        state.scheduler.reconcile(&snapshot, myself, true);
        state.external_watchers.reconcile(&snapshot, myself);

        let actor = myself.clone();
        state.reconcile_task = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(MATERIALIZATION_RECONCILE_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Startup already reconciled in pre_start; skip the immediate first tick.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if actor
                    .cast(ProfilesActorMessage::ReconcileMaterializations)
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    async fn stop_producers(state: &mut ProfilesActorState) -> ProducersStopped {
        if state.gate == ProducerGate::Stopped {
            return ProducersStopped::default();
        }
        state.gate = ProducerGate::Stopped;
        state.scheduler.shutdown();
        state.external_watchers.shutdown();
        if let Some(handle) = state.reconcile_task.take() {
            handle.abort();
        }

        // Held admits manual refreshes and imports too, so their downloads
        // are cut short whichever state the gate left. Removing the entries
        // first means a completion already queued finds nothing to settle.
        let refreshes: Vec<_> = state.pending_refresh.drain().map(|(_, p)| p).collect();
        let imports: Vec<_> = state.pending_imports.drain().map(|(_, p)| p).collect();
        let mut tasks = Vec::with_capacity(refreshes.len() + imports.len());
        let mut replies = Vec::with_capacity(tasks.capacity());
        let stopped = ProducersStopped {
            refreshes: refreshes.len(),
            imports: imports.len(),
            unfinished: 0,
        };
        for pending in refreshes {
            pending.task.abort();
            tasks.push(pending.task);
            replies.extend(pending.reply);
        }
        for pending in imports {
            pending.task.abort();
            tasks.push(pending.task);
            replies.push(pending.reply);
        }
        let deadline = tokio::time::Instant::now() + DOWNLOAD_STOP_BOUND;
        let mut unfinished = 0;
        for task in tasks {
            if tokio::time::timeout_at(deadline, task).await.is_err() {
                unfinished += 1;
            }
        }
        for reply in replies {
            let _ = reply.send(Err(ProfilesError::ShuttingDown));
        }
        ProducersStopped {
            unfinished,
            ..stopped
        }
    }

    /// Downloads and validates on a task the pending entry owns; the file is
    /// written by the commit handler, after its stale-download fence. A panic
    /// still settles the attempt. An abort settles nothing: whoever aborts
    /// has already removed the entry.
    fn spawn_download(
        fetcher: Arc<dyn SubscriptionFetcher>,
        url: url::Url,
        option: RemoteProfileOptions,
        definition: ProfileDefinition,
        settle: impl FnOnce(RefreshOutcome) + Send + 'static,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let fetch = async {
                let fetched = fetcher
                    .fetch(&url, &option)
                    .await
                    .map_err(|e| format!("download failed: {e}"))?;
                Self::validate_fetched_content(&definition, &fetched.content)?;
                Ok::<_, String>(fetched)
            };
            let outcome = match std::panic::AssertUnwindSafe(fetch).catch_unwind().await {
                Ok(Ok(fetched)) => RefreshOutcome::Succeeded {
                    subscription: fetched.subscription,
                    suggested_update_interval_minutes: fetched.suggested_update_interval_minutes,
                    content: fetched.content,
                    filename: fetched.filename,
                },
                Ok(Err(message)) => RefreshOutcome::Failed { message },
                // Do not downcast panic payloads; emit a stable diagnostic.
                Err(_) => RefreshOutcome::Failed {
                    message: "subscription fetch task panicked".into(),
                },
            };
            settle(outcome);
        })
    }

    fn record_source(
        state: &mut ProfilesActorState,
        uid: &ProfileId,
        origin: SourceOrigin,
        outcome: SourceOutcome,
    ) {
        let versioned = state.manager.snapshot_handle().load();
        state.sources.record(&versioned.state, uid, origin, outcome);
    }

    /// Reads a changed Mirror target and commits its managed copy.
    async fn sync_mirror(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        uid: &ProfileId,
        expected_target: ExternalProfilePath,
        expected_path: ManagedProfilePath,
        definition: ProfileDefinition,
    ) -> SourceOutcome {
        let expected_fingerprint = match serde_yaml::to_string(&definition) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                return SourceOutcome::Failed {
                    message: format!("failed to fingerprint external profile: {error}"),
                };
            }
        };
        let fs = Arc::clone(&state.fs);
        let read_target = expected_target.clone();
        let content = tokio::task::spawn_blocking(move || fs.read_external(&read_target))
            .await
            .map_err(|error| anyhow::anyhow!("mirror source read task failed: {error}"))
            .and_then(|content| content);
        let content = match content {
            Ok(content) => content,
            Err(error) => {
                return SourceOutcome::Failed {
                    message: format!("failed to read external profile: {error}"),
                };
            }
        };
        if let Err(error) = Self::validate_fetched_content(&definition, &content) {
            return SourceOutcome::rejected(SourceOutcome::EXTERNAL_SOURCE_REJECTED, error);
        }

        let versioned = state.manager.snapshot_handle().load();
        let expected_version = versioned.version;
        let before = versioned.state.clone();
        drop(versioned);
        let still_current = before.items.get(uid).is_some_and(|current| {
            matches!(
                current.definition.source(),
                Some(ProfileSource::Local {
                    binding: LocalBinding::External {
                        materialized,
                        target,
                        mode: ExternalMode::Mirror,
                    },
                }) if *target == expected_target && materialized.file == expected_path
            ) && serde_yaml::to_string(&current.definition)
                .is_ok_and(|fingerprint| fingerprint == expected_fingerprint)
        });
        if !still_current {
            return SourceOutcome::Superseded {
                reason: "external profile definition changed while it was read".into(),
            };
        }
        let mut next = before.clone();
        let item = next
            .items
            .get_mut(uid)
            .expect("fenced external profile remains in the candidate snapshot");
        let Some(ProfileSource::Local {
            binding: LocalBinding::External { materialized, .. },
        }) = item.definition.source_mut()
        else {
            return SourceOutcome::Superseded {
                reason: "external profile definition changed while it was read".into(),
            };
        };
        materialized.updated_at = Some(time::OffsetDateTime::now_utc());
        match Self::commit_file_first(
            myself,
            state,
            expected_version,
            before,
            next,
            AffectsRule::Touched(uid.clone()),
            expected_path,
            content,
        )
        .await
        {
            Ok(report) => SourceOutcome::Committed {
                operation_id: report.receipt.operation_id,
            },
            Err(error) => SourceOutcome::rejected(SourceOutcome::EXTERNAL_SOURCE_REJECTED, error),
        }
    }

    /// Fences a finished download against the live definition and commits it.
    async fn conclude_refresh(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        uid: &ProfileId,
        url: url::Url,
        definition_fingerprint: String,
        outcome: RefreshOutcome,
    ) -> RefreshConclusion {
        let versioned = state.manager.snapshot_handle().load();
        let expected_version = versioned.version;
        let before = versioned.state.clone();
        drop(versioned);
        let changed = || {
            RefreshConclusion::Superseded("subscription definition changed during refresh".into())
        };
        let Some(current) = before.items.get(uid) else {
            return RefreshConclusion::Superseded("profile deleted during refresh".into());
        };
        let current_fingerprint = match serde_yaml::to_string(&current.definition) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                return RefreshConclusion::Failed(format!(
                    "failed to fingerprint current definition: {error}"
                ));
            }
        };
        let path = match current.definition.source() {
            Some(ProfileSource::Remote {
                url: current_url,
                materialized,
                ..
            }) if current_fingerprint == definition_fingerprint && *current_url == url => {
                materialized.file.clone()
            }
            _ => return changed(),
        };
        // Fenced first: a download that failed for a definition that no
        // longer exists says nothing about the current one.
        let (subscription, content, filename) = match outcome {
            RefreshOutcome::Failed { message } => return RefreshConclusion::Failed(message),
            RefreshOutcome::Succeeded {
                subscription,
                suggested_update_interval_minutes: _,
                content,
                filename,
            } => (subscription, content, filename),
        };
        if let Err(message) = Self::validate_fetched_content(&current.definition, &content) {
            return RefreshConclusion::Superseded(format!(
                "stale download no longer valid for the current definition: {message}"
            ));
        }
        let mut next = before.clone();
        let item = next
            .items
            .get_mut(uid)
            .expect("fenced profile remains in the candidate snapshot");
        if let Some(name) = synced_name(item.metadata.custom_name, &filename) {
            item.metadata.name = name;
        }
        let Some(ProfileSource::Remote {
            materialized,
            subscription: slot,
            ..
        }) = item.definition.source_mut()
        else {
            return changed();
        };
        materialized.updated_at = Some(time::OffsetDateTime::now_utc());
        *slot = subscription;
        // Manual/scheduled refresh never adopts server interval suggestions;
        // import applies them only on first commit.
        match Self::commit_file_first(
            myself,
            state,
            expected_version,
            before,
            next,
            AffectsRule::Touched(uid.clone()),
            path,
            content,
        )
        .await
        {
            Ok(report) => RefreshConclusion::Committed(report),
            Err(error) => RefreshConclusion::Rejected(error),
        }
    }

    async fn run_state_write<F>(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        mutate: F,
    ) -> Result<CommitReport, ProfilesError>
    where
        F: FnOnce(&mut Profiles) -> Result<AffectsRule, ProfilesError>,
    {
        let versioned = state.manager.snapshot_handle().load();
        let expected_version = versioned.version;
        let before = versioned.state.clone();
        drop(versioned);
        let mut next = before.clone();
        let affects = mutate(&mut next)?;
        let candidate = Self::prepare_candidate(next)?;
        let (hints, class) = Self::mutation_hints(&affects, &candidate);
        let (snapshot, (receipt, runtime_degradations)) =
            Self::persist_candidate(state, expected_version, candidate, hints, class).await?;
        Self::reconcile_committed(myself, state, &snapshot);
        Ok(CommitReport {
            affects_current: Self::evaluate_affects(&affects, &before, &snapshot),
            snapshot,
            degradations: Vec::new(),
            receipt,
            runtime_degradations,
            created: None,
        })
    }

    async fn materialization_call<T, F>(
        state: &ProfilesActorState,
        operation: F,
    ) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&dyn ProfileMaterializationPort) -> anyhow::Result<T> + Send + 'static,
    {
        let materialization = Arc::clone(&state.materialization);
        tokio::task::spawn_blocking(move || operation(materialization.as_ref()))
            .await
            .map_err(|error| anyhow::anyhow!("materialization task failed: {error}"))?
    }

    fn materialization_error(context: &str, error: impl std::fmt::Display) -> ProfilesError {
        ProfilesError::Materialization(format!("{context}: {error}"))
    }

    async fn resource_for_definition(
        state: &ProfilesActorState,
        definition: &ProfileDefinition,
        initial_file: Option<String>,
    ) -> Result<Option<MaterializationResource>, ProfilesError> {
        let Some(source) = definition.source() else {
            return Ok(None);
        };
        match source {
            ProfileSource::Local {
                binding: LocalBinding::Managed { .. },
            } => Ok(Some(MaterializationResource::File {
                content: initial_file.unwrap_or_default(),
            })),
            ProfileSource::Local {
                binding:
                    LocalBinding::External {
                        target,
                        mode: ExternalMode::Symlink,
                        ..
                    },
            } => Ok(Some(MaterializationResource::Symlink {
                target: target.clone(),
            })),
            ProfileSource::Local {
                binding:
                    LocalBinding::External {
                        target,
                        mode: ExternalMode::Mirror,
                        ..
                    },
            } => {
                let fs = Arc::clone(&state.fs);
                let target = target.clone();
                let content = tokio::task::spawn_blocking(move || fs.read_external(&target))
                    .await
                    .map_err(|error| {
                        Self::materialization_error("mirror source read task failed", error)
                    })?
                    .map_err(|error| {
                        Self::materialization_error("failed to read mirror source", error)
                    })?;
                Self::validate_fetched_content(definition, &content).map_err(|error| {
                    ProfilesError::Materialization(format!(
                        "mirror source validation failed: {error}"
                    ))
                })?;
                Ok(Some(MaterializationResource::File { content }))
            }
            ProfileSource::Remote { .. } => Ok(Some(MaterializationResource::File {
                // Direct Add of a remote source still stages an empty file.
                // Remote *import* never uses this path: it fetch-before-commits
                // real bytes through ImportRemote / CommitImported.
                content: String::new(),
            })),
        }
    }

    fn remote_import_definition(
        url: url::Url,
        option: RemoteProfileOptions,
        file: ManagedProfilePath,
        subscription: SubscriptionInfo,
        updated_at: Option<time::OffsetDateTime>,
    ) -> ProfileDefinition {
        ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Remote {
                    materialized: MaterializedFile { file, updated_at },
                    url,
                    option,
                    subscription,
                },
                transforms: vec![],
            }),
        }
    }

    /// Validate URL/options against the live document without writing state/files.
    fn validate_import_request(
        before: &Profiles,
        metadata: &ProfileMetadata,
        url: url::Url,
        option: RemoteProfileOptions,
    ) -> Result<(), ProfilesError> {
        let definition = Self::remote_import_definition(
            url,
            option,
            ManagedProfilePath::new("pending.yaml").expect("static managed path is valid"),
            SubscriptionInfo::default(),
            None,
        );
        let uid = Self::generate_uid(&definition, before);
        let mut next = before.clone();
        let mut definition = definition;
        let ext = Self::canonical_extension(&definition);
        if let Some(source) = definition.source_mut() {
            source.materialized_mut().file = ManagedProfilePath::new(format!("{uid}.{ext}"))
                .expect("uid-derived path is always a valid managed path");
        }
        if !next.append_item(ProfileItem {
            uid,
            metadata: metadata.clone(),
            definition,
        }) {
            return Err(ProfilesError::Persist("uid collision".into()));
        }
        next.validate().map_err(ProfilesError::ValidationFailed)?;
        Ok(())
    }

    async fn finish_cleanup(
        state: &ProfilesActorState,
        cleanup: PreparedCleanup,
        snapshot: &Profiles,
    ) -> Vec<ProfileDegradation> {
        let retry = cleanup.clone();
        if let Err(error) =
            Self::materialization_call(state, move |port| port.activate_cleanup(&cleanup)).await
        {
            return vec![ProfileDegradation {
                phase: ProfileDegradationPhase::Cleanup,
                code: ProfileDegradationCode::CleanupDeferred,
                message: format!("profile cleanup activation deferred: {error}"),
            }];
        }
        let profiles = snapshot.clone();
        if let Err(error) =
            Self::materialization_call(state, move |port| port.retry_cleanup(&retry, &profiles))
                .await
        {
            return vec![ProfileDegradation {
                phase: ProfileDegradationPhase::Cleanup,
                code: ProfileDegradationCode::CleanupDeferred,
                message: format!("profile cleanup retry deferred: {error}"),
            }];
        }
        Vec::new()
    }

    fn mutation_hints(
        affects: &AffectsRule,
        candidate: &Profiles,
    ) -> (MutationHints, CommandClass) {
        let mut hints = MutationHints::default();
        if let AffectsRule::Touched(uid) = affects {
            if let Some(source) = candidate
                .items
                .get(uid)
                .and_then(|item| item.definition.source())
            {
                hints.touched.push(TouchedContent {
                    path: source.materialized().file.clone(),
                    content_digest: None,
                    resource: None,
                });
            }
        }
        let class = if matches!(affects, AffectsRule::CurrentChanged) {
            hints.activation = candidate
                .current
                .clone()
                .map(ActivationIntent::Activate)
                .unwrap_or(ActivationIntent::Deactivate);
            CommandClass::ExplicitSwitch
        } else {
            if matches!(affects, AffectsRule::Always | AffectsRule::GlobalChanged) {
                hints.requested = RequestedRuntimeFields::runtime();
            }
            CommandClass::Save
        };
        (hints, class)
    }

    #[allow(clippy::too_many_arguments)]
    async fn commit_with_resources(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        expected_version: Version,
        before: Profiles,
        next: Profiles,
        affects: AffectsRule,
        resource: Option<(ManagedProfilePath, MaterializationResource)>,
        cleanup_path: Option<ManagedProfilePath>,
        created: Option<ProfileId>,
    ) -> Result<CommitReport, ProfilesError> {
        let candidate = Self::prepare_candidate(next)?;
        let expected_revision = candidate.revision();
        state
            .mutations
            .ensure_ready()
            .map_err(|error| ProfilesError::Persist(error.to_string()))?;
        let operation = OperationId::generate();
        let (mut hints, class) = Self::mutation_hints(&affects, &candidate);
        let prepared = if let Some((path, resource)) = resource {
            let content = match &resource {
                MaterializationResource::File { content } => Some(content.clone()),
                MaterializationResource::Symlink { .. }
                    if !Self::current_closure(&candidate).iter().any(|uid| {
                        candidate
                            .items
                            .get(uid)
                            .and_then(|item| item.definition.source())
                            .is_some_and(|source| source.materialized().file == path)
                    }) =>
                {
                    None
                }
                MaterializationResource::Symlink { target } => {
                    let fs = state.fs.clone();
                    let target = target.clone();
                    Some(
                        tokio::task::spawn_blocking(move || fs.read_external(&target))
                            .await
                            .map_err(|error| {
                                Self::materialization_error("source read task", error)
                            })?
                            .map_err(|error| Self::materialization_error("source read", error))?,
                    )
                }
            };
            hints.touched.push(TouchedContent {
                path: path.clone(),
                content_digest: content.as_ref().map(|content| {
                    ContentDigest::new(nyanpasu_core_manager::payload_digest(content.as_bytes()))
                }),
                resource: Some(StagedResourceToken::new(operation.to_string())),
            });
            if let Some(content) = content {
                hints.staged_content.insert(path.to_string(), content);
            }
            Some(
                Self::materialization_call(state, move |port| {
                    port.prepare_file_first(&path, resource, expected_revision)
                })
                .await
                .map_err(|error| Self::materialization_error("prepare materialization", error))?,
            )
        } else {
            None
        };
        let cleanup = if let Some(path) = cleanup_path {
            match Self::materialization_call(state, move |port| {
                port.prepare_cleanup(&path, expected_revision)
            })
            .await
            {
                Ok(cleanup) => Some(cleanup),
                Err(error) => {
                    if let Some(prepared) = prepared {
                        Self::materialization_call(state, move |port| port.compensate(&prepared))
                            .await
                            .map_err(|error| {
                                Self::materialization_error("discard staged resource", error)
                            })?;
                    }
                    return Err(Self::materialization_error("prepare cleanup", error));
                }
            }
        } else {
            None
        };
        let participant = state
            .mutations
            .participant(operation, hints, class)
            .map_err(|error| ProfilesError::Persist(error.to_string()))?;
        let write_port = state.materialization.clone();
        let write_resource = prepared.clone();
        let recover_port = state.materialization.clone();
        let recover_resource = prepared.clone();
        let recover_cleanup = cleanup.clone();
        let result = state
            .manager
            .replace_if_version_with_participant(
                expected_version,
                candidate.clone(),
                participant,
                move || async move {
                    tokio::task::spawn_blocking(move || {
                        if let Some(prepared) = write_resource {
                            write_port.promote(&prepared)?;
                        }
                        anyhow::Ok(())
                    })
                    .await?
                },
                move || async move {
                    tokio::task::spawn_blocking(move || {
                        let resource = recover_resource
                            .as_ref()
                            .map(|p| recover_port.compensate(p))
                            .transpose();
                        let cleanup = recover_cleanup
                            .as_ref()
                            .map(|c| recover_port.cancel_cleanup(c))
                            .transpose();
                        match (resource, cleanup) {
                            (Ok(_), Ok(_)) => Ok(()),
                            (resource, cleanup) => anyhow::bail!(
                                "resource recovery: {resource:?}; cleanup recovery: {cleanup:?}"
                            ),
                        }
                    })
                    .await?
                },
            )
            .await;
        match result {
            Ok(ReplaceIfVersionResult::Replaced) => {}
            result => {
                // Only a pre-persistence refusal leaves staging to discard here.
                // Once local_write starts, the source transaction exclusively owns
                // compensation and publishes its result before releasing admission.
                let before_write = matches!(
                    &result,
                    Ok(ReplaceIfVersionResult::Conflict { .. })
                        | Err(nyanpasu_core::state::ReplaceIfVersionError::State(
                            nyanpasu_core::state::error::StateChangedError::PrepareAck(_)
                        ))
                );
                let mut failures = Vec::new();
                if before_write {
                    if let Some(prepared) = prepared {
                        if let Err(error) = Self::materialization_call(state, move |port| {
                            port.compensate(&prepared)
                        })
                        .await
                        {
                            failures.push(error.to_string());
                        }
                    }
                    if let Some(cleanup) = cleanup {
                        if let Err(error) = Self::materialization_call(state, move |port| {
                            port.cancel_cleanup(&cleanup)
                        })
                        .await
                        {
                            failures.push(error.to_string());
                        }
                    }
                }
                return Err(match result {
                    Err(source) => ProfilesError::SourceTransaction {
                        source,
                        cleanup: (!failures.is_empty()).then(|| failures.join("; ")),
                    },
                    Ok(ReplaceIfVersionResult::Conflict { actual_version }) => {
                        if failures.is_empty() {
                            ProfilesError::VersionConflict {
                                expected: *expected_version.as_ref(),
                                actual: *actual_version.as_ref(),
                            }
                        } else {
                            ProfilesError::ConflictCleanup {
                                expected: *expected_version.as_ref(),
                                actual: *actual_version.as_ref(),
                                cleanup: failures.join("; "),
                            }
                        }
                    }
                    Ok(ReplaceIfVersionResult::Replaced) => unreachable!(),
                });
            }
        }
        let (receipt, runtime_degradations) = state
            .mutations
            .finish(
                operation,
                "profiles",
                *state.manager.snapshot_handle().load().version.as_ref(),
            )
            .await;
        let snapshot = Arc::new(candidate);
        Self::reconcile_committed(myself, state, &snapshot);
        let mut degradations = Vec::new();
        if let Some(prepared) = prepared {
            if let Err(error) =
                Self::materialization_call(state, move |port| port.complete(&prepared)).await
            {
                degradations.push(ProfileDegradation {
                    phase: ProfileDegradationPhase::Reconcile,
                    code: ProfileDegradationCode::MaterializationDeferred,
                    message: format!("materialization completion deferred: {error}"),
                });
            }
        }
        if let Some(cleanup) = cleanup {
            degradations.extend(Self::finish_cleanup(state, cleanup, &snapshot).await);
        }
        Ok(CommitReport {
            affects_current: Self::evaluate_affects(&affects, &before, &snapshot),
            snapshot,
            degradations,
            receipt,
            runtime_degradations,
            created,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn commit_file_first(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        expected_version: Version,
        before: Profiles,
        next: Profiles,
        affects: AffectsRule,
        path: ManagedProfilePath,
        content: String,
    ) -> Result<CommitReport, ProfilesError> {
        Self::commit_with_resources(
            myself,
            state,
            expected_version,
            before,
            next,
            affects,
            Some((path, MaterializationResource::File { content })),
            None,
            None,
        )
        .await
    }

    async fn reconcile_materializations(
        state: &ProfilesActorState,
    ) -> anyhow::Result<MaterializationReconcileReport> {
        let snapshot = Self::current_state(state);
        let materialization = Arc::clone(&state.materialization);
        tokio::task::spawn_blocking(move || materialization.reconcile(&snapshot))
            .await
            .map_err(|error| anyhow::anyhow!("materialization reconcile join failed: {error}"))?
    }

    fn log_reconcile_report(report: &MaterializationReconcileReport) {
        for degradation in &report.degradations {
            tracing::warn!(
                phase = ?degradation.phase,
                code = ?degradation.code,
                retryable = degradation.code.retryable(),
                message = %degradation.message,
                "profile materialization reconcile degradation"
            );
        }
        if report.discarded
            + report.promoted
            + report.completed
            + report.compensated
            + report.cleanups_completed
            + report.cleanups_fenced
            > 0
        {
            tracing::info!(
                discarded = report.discarded,
                promoted = report.promoted,
                completed = report.completed,
                compensated = report.compensated,
                cleanups_completed = report.cleanups_completed,
                cleanups_fenced = report.cleanups_fenced,
                "profile materialization reconcile advanced journals"
            );
        }
    }

    fn generate_uid(definition: &ProfileDefinition, existing: &Profiles) -> ProfileId {
        let prefix = match definition {
            ProfileDefinition::Config { .. } => 'c',
            ProfileDefinition::Transform { .. } => 't',
        };
        loop {
            let candidate = ProfileId(format!("{prefix}{}", nanoid::nanoid!(11)));
            if existing.items.get(&candidate).is_none() {
                return candidate;
            }
        }
    }

    fn canonical_extension(definition: &ProfileDefinition) -> &'static str {
        match definition {
            ProfileDefinition::Config { .. } => "yaml",
            ProfileDefinition::Transform { transform } => match transform {
                TransformDefinition::Overlay(_) => "yaml",
                TransformDefinition::Script(script) => match script.runtime {
                    ScriptRuntime::JavaScript => "js",
                    ScriptRuntime::Lua => "lua",
                },
            },
        }
    }

    fn validate_fetched_content(
        definition: &ProfileDefinition,
        content: &str,
    ) -> Result<(), String> {
        let needs_yaml = match definition {
            ProfileDefinition::Config { .. } => true,
            ProfileDefinition::Transform { transform } => {
                matches!(transform, TransformDefinition::Overlay(_))
            }
        };
        if needs_yaml {
            let mapping = serde_yaml::from_str::<serde_yaml::Mapping>(content)
                .map_err(|e| format!("downloaded content is not a YAML mapping: {e}"))?;
            // Legacy subscription semantics (remote.rs BC): a Config
            // subscription must actually carry proxies, otherwise arbitrary
            // mappings (e.g. `{}`) get persisted and can be auto-activated.
            if matches!(definition, ProfileDefinition::Config { .. })
                && !mapping.contains_key("proxies")
                && !mapping.contains_key("proxy-providers")
            {
                return Err("subscription does not contain `proxies` or `proxy-providers`".into());
            }
            Ok(())
        } else if content.trim().is_empty() {
            Err("downloaded script is empty".into())
        } else {
            Ok(())
        }
    }

    /// design §17 five reference categories. Item-level referrers plus the two
    /// document-level flags (current / global_transforms) so the IPC layer can
    /// render an unambiguous message even when the referrer list is empty.
    fn referrers_of(
        state: &ProfilesActorState,
        profiles: &Profiles,
        uid: &ProfileId,
    ) -> Option<(Vec<ProfileId>, bool, bool)> {
        let mut referrers: indexmap::IndexSet<ProfileId> = Default::default();
        if let Some(set) = state.index.composition_base_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }
        if let Some(set) = state.index.extend_proxies_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }
        if let Some(set) = state.index.transform_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }

        let current = profiles.current.as_ref() == Some(uid);
        let global_transforms = state.index.global_transform_ids.contains(uid);
        if referrers.is_empty() && !current && !global_transforms {
            None
        } else {
            Some((referrers.into_iter().collect(), current, global_transforms))
        }
    }

    fn retains_materialization(
        previous: &ProfileSource,
        next: &ProfileSource,
        canonical: &ManagedProfilePath,
    ) -> bool {
        previous.materialized().file == *canonical
            && match (previous, next) {
                (
                    ProfileSource::Local {
                        binding: LocalBinding::Managed { .. },
                    },
                    ProfileSource::Local {
                        binding: LocalBinding::Managed { .. },
                    },
                ) => true,
                (
                    ProfileSource::Local {
                        binding:
                            LocalBinding::External {
                                target: previous_target,
                                mode: previous_mode,
                                ..
                            },
                    },
                    ProfileSource::Local {
                        binding:
                            LocalBinding::External {
                                target: next_target,
                                mode: next_mode,
                                ..
                            },
                    },
                ) => previous_target == next_target && previous_mode == next_mode,
                (
                    ProfileSource::Remote {
                        url: previous_url, ..
                    },
                    ProfileSource::Remote { url: next_url, .. },
                ) => previous_url == next_url,
                _ => false,
            }
    }
}

impl Actor for ProfilesActor {
    type Msg = ProfilesActorMessage;
    type State = ProfilesActorState;
    type Arguments = ProfilesActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        // Startup recovery must finish before mutations are admitted; the
        // producers wait for StartProducers. Blocking port work stays off the
        // async runtime via spawn_blocking.
        let loaded = args.manager.snapshot_handle().load().state.clone();
        let materialization = Arc::clone(&args.materialization);
        let report = tokio::task::spawn_blocking(move || materialization.reconcile(&loaded))
            .await
            .map_err(|error| {
                ActorProcessingErr::from(anyhow::anyhow!(
                    "startup materialization reconcile join failed: {error}"
                ))
            })?
            .map_err(|error| {
                ActorProcessingErr::from(anyhow::anyhow!(
                    "startup materialization reconcile failed: {error}"
                ))
            })?;
        Self::log_reconcile_report(&report);

        let index = ProfileDependencyIndex::build(&args.manager.snapshot_handle().load().state);
        Ok(ProfilesActorState {
            mutations: args.mutations,
            manager: args.manager,
            index,
            fs: args.fs,
            fetcher: args.fetcher,
            materialization: args.materialization,
            pending_refresh: HashMap::new(),
            next_refresh_token: 1,
            pending_imports: HashMap::new(),
            next_import_token: 1,
            gate: ProducerGate::Held,
            scheduler: RemoteUpdateScheduler::default(),
            external_watchers: ExternalWatchers::default(),
            reconcile_task: None,
            sources: SourceLedger::new(args.sources),
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ProfilesActorMessage::SaveFile {
                uid,
                content,
                reply,
            } => {
                let result = async {
                    let versioned = state.manager.snapshot_handle().load();
                    let before = versioned.state.clone();
                    let item = before
                        .items
                        .get(&uid)
                        .ok_or_else(|| ProfilesError::ProfileNotFound(uid.clone()))?;
                    let source = item
                        .definition
                        .source()
                        .ok_or(ProfilesError::ProfileHasNoFile)?;
                    let path = match source {
                        ProfileSource::Local {
                            binding: LocalBinding::Managed { materialized },
                        } => materialized.file.clone(),
                        _ => {
                            return Err(ProfilesError::FileNotWritable {
                                reason: "only managed local profiles are writable".into(),
                            });
                        }
                    };
                    Self::commit_file_first(
                        &myself,
                        state,
                        versioned.version,
                        before.clone(),
                        before,
                        AffectsRule::Touched(uid),
                        path,
                        content,
                    )
                    .await
                }
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetCurrent { current, reply } => {
                let result = Self::run_state_write(&myself, state, |profiles| {
                    profiles.set_current(current);
                    Ok(AffectsRule::CurrentChanged)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetCurrentIfNone { uid, reply } => {
                // Atomic conditional activation: select `uid` only when nothing
                // is currently selected. Serialized actor message handling makes
                // this read-then-write race-free without a second RPC, so a
                // concurrent SetCurrent cannot be silently overwritten.
                if Self::current_state(state).current.is_some() {
                    let _ = reply.send(Ok(None));
                } else {
                    let result = Self::run_state_write(&myself, state, |profiles| {
                        profiles.set_current(Some(uid));
                        Ok(AffectsRule::CurrentChanged)
                    })
                    .await;
                    let _ = reply.send(result.map(Some));
                }
            }
            ProfilesActorMessage::SetGlobalTransforms { ids, reply } => {
                let result = Self::run_state_write(&myself, state, |profiles| {
                    profiles.global_transforms = ids;
                    Ok(AffectsRule::GlobalChanged)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetValidFields { fields, reply } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    profiles.valid = fields;
                    // Whitelist changes reshape runtime extraction for the active
                    // config, so a rebuild is always required.
                    Ok(AffectsRule::Always)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Replace {
                profiles: next,
                reply,
            } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    // Client-supplied documents must not reset the server-owned
                    // materialization recovery generation; state writes bump once.
                    profiles.current = next.current;
                    profiles.global_transforms = next.global_transforms;
                    profiles.valid = next.valid;
                    profiles.items = next.items;
                    Ok(AffectsRule::Always)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Add {
                request,
                initial_file,
                reply,
            } => {
                let versioned = state.manager.snapshot_handle().load();
                let expected_version = versioned.version;
                let before = versioned.state.clone();
                drop(versioned);
                let uid = Self::generate_uid(&request.definition, &before);
                let ext = Self::canonical_extension(&request.definition);
                let canonical = ManagedProfilePath::new(format!("{uid}.{ext}"))
                    .expect("uid-derived path is always a valid managed path");
                let mut definition = request.definition;
                if let Some(source) = definition.source_mut() {
                    let materialized = source.materialized_mut();
                    materialized.file = canonical.clone();
                    materialized.updated_at = None;
                    if let ProfileSource::Remote { subscription, .. } = source {
                        *subscription = SubscriptionInfo::default();
                    }
                }
                let result =
                    match Self::resource_for_definition(state, &definition, initial_file).await {
                        Ok(resource) => {
                            let item = ProfileItem {
                                uid: uid.clone(),
                                metadata: request.metadata,
                                definition,
                            };
                            let mut next = before.clone();
                            if !next.append_item(item) {
                                Err(ProfilesError::Persist("uid collision".into()))
                            } else {
                                Self::commit_with_resources(
                                    &myself,
                                    state,
                                    expected_version,
                                    before,
                                    next,
                                    AffectsRule::Never,
                                    resource.map(|resource| (canonical, resource)),
                                    None,
                                    Some(uid),
                                )
                                .await
                            }
                        }
                        Err(error) => Err(error),
                    };
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Delete { uid, reply } => {
                let versioned = state.manager.snapshot_handle().load();
                let expected_version = versioned.version;
                let before = versioned.state.clone();
                drop(versioned);
                let result = if before.items.get(&uid).is_none() {
                    Err(ProfilesError::ProfileNotFound(uid.clone()))
                } else if let Some((referrers, current, global_transforms)) =
                    Self::referrers_of(state, &before, &uid)
                {
                    Err(ProfilesError::ProfileInUse {
                        referrers,
                        current,
                        global_transforms,
                    })
                } else {
                    let cleanup_path = before
                        .items
                        .get(&uid)
                        .and_then(|item| item.definition.source())
                        .map(|source| source.materialized().file.clone());
                    let mut next = before.clone();
                    next.remove_item_unchecked(&uid);
                    Self::commit_with_resources(
                        &myself,
                        state,
                        expected_version,
                        before,
                        next,
                        AffectsRule::Never,
                        None,
                        cleanup_path,
                        None,
                    )
                    .await
                };
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Reorder { op, reply } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    match op {
                        ReorderOp::Move { active, over } => {
                            if profiles.items.get(&active).is_none() {
                                return Err(ProfilesError::ProfileNotFound(active));
                            }
                            if profiles.items.get(&over).is_none() {
                                return Err(ProfilesError::ProfileNotFound(over));
                            }
                            profiles.reorder(&active, &over);
                        }
                        ReorderOp::ByList(list) => {
                            if list.len() != profiles.items.len() {
                                return Err(ProfilesError::InvalidReorderList {
                                    reason: format!(
                                        "expected {} uids, got {}",
                                        profiles.items.len(),
                                        list.len()
                                    ),
                                });
                            }
                            let mut seen = indexmap::IndexSet::with_capacity(list.len());
                            for uid in &list {
                                if !seen.insert(uid.clone()) {
                                    return Err(ProfilesError::InvalidReorderList {
                                        reason: format!("duplicate uid {uid}"),
                                    });
                                }
                                if profiles.items.get(uid).is_none() {
                                    return Err(ProfilesError::ProfileNotFound(uid.clone()));
                                }
                            }
                            let mut reordered = indexmap::IndexMap::with_capacity(list.len());
                            for uid in list {
                                let item = profiles
                                    .items
                                    .shift_remove(&uid)
                                    .ok_or_else(|| ProfilesError::ProfileNotFound(uid.clone()))?;
                                reordered.insert(uid, item);
                            }
                            profiles.items = reordered;
                        }
                    }
                    Ok(AffectsRule::Never)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::PatchMetadata { uid, patch, reply } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    let Some(item) = profiles.items.get_mut(&uid) else {
                        return Err(ProfilesError::ProfileNotFound(uid));
                    };
                    item.apply_metadata_patch(patch);
                    Ok(AffectsRule::Never)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::PatchRemoteOptions { uid, patch, reply } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    let Some(item) = profiles.items.get_mut(&uid) else {
                        return Err(ProfilesError::ProfileNotFound(uid));
                    };
                    match item.definition.source_mut() {
                        Some(ProfileSource::Remote { option, .. }) => {
                            use struct_patch::Patch as _;
                            option.apply(patch);
                            Ok(AffectsRule::Never)
                        }
                        _ => Err(ProfilesError::NotARemoteProfile),
                    }
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::RefreshRemote {
                uid,
                patch,
                origin,
                reply,
            } => {
                match (origin, state.gate) {
                    (RefreshOrigin::Manual, ProducerGate::Stopped) => {
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(ProfilesError::ShuttingDown));
                        }
                        return Ok(());
                    }
                    (RefreshOrigin::Scheduled, ProducerGate::Held | ProducerGate::Stopped) => {
                        return Ok(());
                    }
                    _ => {}
                }
                if state.pending_refresh.contains_key(&uid) {
                    // A tick folds into the download in flight without a receipt
                    // of its own: that download reports the outcome, and a
                    // healthy row now would hide the last failure until then.
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::RefreshFailed {
                            message: "refresh already in progress".into(),
                        }));
                    }
                    return Ok(());
                }

                if let Some(patch) = patch {
                    let patched = Self::run_state_write(&myself, state, {
                        let uid = uid.clone();
                        move |profiles| {
                            let Some(item) = profiles.items.get_mut(&uid) else {
                                return Err(ProfilesError::ProfileNotFound(uid.clone()));
                            };
                            match item.definition.source_mut() {
                                Some(ProfileSource::Remote { option, .. }) => {
                                    use struct_patch::Patch as _;
                                    option.apply(patch);
                                    Ok(AffectsRule::Never)
                                }
                                _ => Err(ProfilesError::NotARemoteProfile),
                            }
                        }
                    })
                    .await;
                    if let Err(err) = patched {
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(err));
                        }
                        return Ok(());
                    }
                }

                let snapshot = Self::current_state(state);
                let Some(item) = snapshot.items.get(&uid) else {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::ProfileNotFound(uid.clone())));
                    }
                    return Ok(());
                };
                let Some(ProfileSource::Remote { url, option, .. }) = item.definition.source()
                else {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::NotARemoteProfile));
                    }
                    return Ok(());
                };

                let definition = item.definition.clone();
                let definition_fingerprint = match serde_yaml::to_string(&definition) {
                    Ok(fingerprint) => fingerprint,
                    Err(error) => {
                        let message =
                            format!("failed to fingerprint subscription definition: {error}");
                        Self::record_source(
                            state,
                            &uid,
                            origin.source(),
                            SourceOutcome::Failed {
                                message: message.clone(),
                            },
                        );
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(ProfilesError::RefreshFailed { message }));
                        }
                        return Ok(());
                    }
                };
                let url = url.clone();
                let option = option.clone();
                let token = RefreshAttemptToken(state.next_refresh_token);
                state.next_refresh_token += 1;
                let actor = myself.clone();
                let settle_uid = uid.clone();
                let settle_url = url.clone();
                let task = Self::spawn_download(
                    Arc::clone(&state.fetcher),
                    url,
                    option,
                    definition,
                    move |outcome| {
                        let _ = actor.cast(ProfilesActorMessage::CommitRefreshed {
                            uid: settle_uid,
                            token,
                            url: settle_url,
                            definition_fingerprint,
                            outcome,
                        });
                    },
                );
                state.pending_refresh.insert(
                    uid,
                    PendingRefresh {
                        token,
                        origin,
                        reply,
                        task,
                    },
                );
            }
            ProfilesActorMessage::CommitRefreshed {
                uid,
                token,
                url,
                definition_fingerprint,
                outcome,
            } => {
                // Only the attempt that started this download may settle it. A
                // completion whose entry was aborted, or replaced by a newer
                // attempt, commits nothing and leaves that entry alone.
                if state
                    .pending_refresh
                    .get(&uid)
                    .is_none_or(|pending| pending.token != token)
                {
                    return Ok(());
                }
                let Some(pending) = state.pending_refresh.remove(&uid) else {
                    return Ok(());
                };
                let conclusion = Self::conclude_refresh(
                    &myself,
                    state,
                    &uid,
                    url,
                    definition_fingerprint,
                    outcome,
                )
                .await;
                let (outcome, result) = match conclusion {
                    RefreshConclusion::Committed(report) => (
                        SourceOutcome::Committed {
                            operation_id: report.receipt.operation_id.clone(),
                        },
                        Ok(report),
                    ),
                    RefreshConclusion::Superseded(reason) => (
                        SourceOutcome::Superseded {
                            reason: reason.clone(),
                        },
                        Err(ProfilesError::RefreshFailed { message: reason }),
                    ),
                    RefreshConclusion::Failed(message) => (
                        SourceOutcome::Failed {
                            message: message.clone(),
                        },
                        Err(ProfilesError::RefreshFailed { message }),
                    ),
                    RefreshConclusion::Rejected(error) => (
                        SourceOutcome::rejected(SourceOutcome::SUBSCRIPTION_REJECTED, &error),
                        Err(error),
                    ),
                };
                Self::record_source(state, &uid, pending.origin.source(), outcome);
                if let Some(reply) = pending.reply {
                    let _ = reply.send(result);
                }
            }
            ProfilesActorMessage::ImportRemote {
                url,
                metadata,
                option,
                update_interval_explicit,
                reply,
            } => {
                if state.gate == ProducerGate::Stopped {
                    let _ = reply.send(Err(ProfilesError::ShuttingDown));
                    return Ok(());
                }
                let before = Self::current_state(state);
                if let Err(error) =
                    Self::validate_import_request(&before, &metadata, url.clone(), option.clone())
                {
                    let _ = reply.send(Err(error));
                    return Ok(());
                }

                let token = ImportOperationToken(state.next_import_token);
                state.next_import_token = state.next_import_token.wrapping_add(1).max(1);
                let actor = myself.clone();
                // Content validation uses a Config definition shape; import is
                // always a remote Config File profile.
                let definition_for_validation = Self::remote_import_definition(
                    url.clone(),
                    option.clone(),
                    ManagedProfilePath::new("pending.yaml").expect("static managed path is valid"),
                    SubscriptionInfo::default(),
                    None,
                );
                let task = Self::spawn_download(
                    Arc::clone(&state.fetcher),
                    url.clone(),
                    option.clone(),
                    definition_for_validation,
                    move |outcome| {
                        let _ = actor.cast(ProfilesActorMessage::CommitImported { token, outcome });
                    },
                );
                state.pending_imports.insert(
                    token,
                    PendingImport {
                        reply,
                        metadata,
                        url,
                        option,
                        update_interval_explicit,
                        task,
                    },
                );
            }
            ProfilesActorMessage::CommitImported { token, outcome } => {
                let Some(pending) = state.pending_imports.remove(&token) else {
                    // Actor restart / late completion: nothing durable was written.
                    return Ok(());
                };
                // Cancellation before durable commit begins: discard fetch result.
                if pending.reply.is_closed() {
                    return Ok(());
                }

                let result = match outcome {
                    RefreshOutcome::Failed { message } => {
                        Err(ProfilesError::ImportFailed { message })
                    }
                    RefreshOutcome::Succeeded {
                        subscription,
                        suggested_update_interval_minutes,
                        content,
                        filename,
                    } => {
                        let versioned = state.manager.snapshot_handle().load();
                        let expected_version = versioned.version;
                        let before = versioned.state.clone();
                        drop(versioned);

                        let mut option = pending.option;
                        if !pending.update_interval_explicit
                            && let Some(minutes) = suggested_update_interval_minutes
                        {
                            option.update_interval_minutes = minutes;
                        }
                        let mut metadata = pending.metadata;
                        if let Some(name) = synced_name(metadata.custom_name, &filename) {
                            metadata.name = name;
                        }

                        let mut definition = Self::remote_import_definition(
                            pending.url,
                            option,
                            ManagedProfilePath::new("pending.yaml")
                                .expect("static managed path is valid"),
                            subscription,
                            Some(time::OffsetDateTime::now_utc()),
                        );
                        if let Err(message) = Self::validate_fetched_content(&definition, &content)
                        {
                            Err(ProfilesError::ImportFailed {
                                message: format!(
                                    "downloaded content is not valid for import: {message}"
                                ),
                            })
                        } else {
                            let uid = Self::generate_uid(&definition, &before);
                            let ext = Self::canonical_extension(&definition);
                            let canonical = ManagedProfilePath::new(format!("{uid}.{ext}"))
                                .expect("uid-derived path is always a valid managed path");
                            if let Some(source) = definition.source_mut() {
                                source.materialized_mut().file = canonical.clone();
                            }
                            let mut next = before.clone();
                            if !next.append_item(ProfileItem {
                                uid: uid.clone(),
                                metadata,
                                definition,
                            }) {
                                Err(ProfilesError::Persist("uid collision".into()))
                            } else {
                                // If the caller closed between the pre-check and
                                // the first durable step, a complete valid profile
                                // may still remain — never an empty shell.
                                Self::commit_with_resources(
                                    &myself,
                                    state,
                                    expected_version,
                                    before,
                                    next,
                                    AffectsRule::Never,
                                    Some((canonical, MaterializationResource::File { content })),
                                    None,
                                    Some(uid),
                                )
                                .await
                            }
                        }
                    }
                };
                let _ = pending.reply.send(result);
            }
            ProfilesActorMessage::ExternalFileChanged { uid } => {
                if state.gate != ProducerGate::Running {
                    return Ok(());
                }
                let snapshot = Self::current_state(state);
                let Some(item) = snapshot.items.get(&uid) else {
                    return Ok(());
                };
                let Some(ProfileSource::Local {
                    binding:
                        LocalBinding::External {
                            materialized,
                            target,
                            mode,
                        },
                }) = item.definition.source()
                else {
                    return Ok(());
                };
                // External content that cannot be accepted is reported, never
                // written back: the user's file stays as they left it (V33).
                let outcome = if *mode == ExternalMode::Mirror {
                    let expected_target = target.clone();
                    let expected_path = materialized.file.clone();
                    let definition = item.definition.clone();
                    Self::sync_mirror(
                        &myself,
                        state,
                        &uid,
                        expected_target,
                        expected_path,
                        definition,
                    )
                    .await
                } else {
                    let result = Self::run_state_write(&myself, state, {
                        let uid = uid.clone();
                        move |profiles| {
                            let Some(item) = profiles.items.get_mut(&uid) else {
                                return Err(ProfilesError::ProfileNotFound(uid.clone()));
                            };
                            match item.definition.source_mut() {
                                Some(ProfileSource::Local {
                                    binding: LocalBinding::External { materialized, .. },
                                }) => {
                                    materialized.updated_at = Some(time::OffsetDateTime::now_utc());
                                    Ok(AffectsRule::Touched(uid.clone()))
                                }
                                _ => Err(ProfilesError::ProfileNotFound(uid.clone())),
                            }
                        }
                    })
                    .await;
                    match result {
                        Ok(report) => SourceOutcome::Committed {
                            operation_id: report.receipt.operation_id,
                        },
                        Err(error) => {
                            SourceOutcome::rejected(SourceOutcome::EXTERNAL_SOURCE_REJECTED, error)
                        }
                    }
                };
                if let SourceOutcome::Failed { message } | SourceOutcome::Rejected { message, .. } =
                    &outcome
                {
                    tracing::warn!(uid = %uid, error = %message, "external profile change not applied");
                }
                Self::record_source(state, &uid, SourceOrigin::ExternalFile, outcome);
            }
            ProfilesActorMessage::ReplaceDefinition {
                uid,
                definition,
                reply,
            } => {
                let versioned = state.manager.snapshot_handle().load();
                let expected_version = versioned.version;
                let before = versioned.state.clone();
                drop(versioned);
                let result = match before.items.get(&uid) {
                    None => Err(ProfilesError::ProfileNotFound(uid.clone())),
                    Some(previous_item) => {
                        let previous_source = previous_item.definition.source().cloned();
                        let mut definition = definition;
                        let ext = Self::canonical_extension(&definition);
                        let canonical = ManagedProfilePath::new(format!("{uid}.{ext}"))
                            .expect("uid-derived path is always a valid managed path");
                        let same_slot = match (&previous_source, definition.source()) {
                            (Some(previous), Some(next)) => {
                                Self::retains_materialization(previous, next, &canonical)
                            }
                            _ => false,
                        };
                        if let Some(source) = definition.source_mut() {
                            let materialized = source.materialized_mut();
                            materialized.file = canonical.clone();
                            materialized.updated_at = if same_slot {
                                previous_source
                                    .as_ref()
                                    .and_then(|source| source.materialized().updated_at)
                            } else {
                                None
                            };
                            if let ProfileSource::Remote { subscription, .. } = source {
                                *subscription = match (same_slot, previous_source.as_ref()) {
                                    (
                                        true,
                                        Some(ProfileSource::Remote {
                                            subscription: previous,
                                            ..
                                        }),
                                    ) => previous.clone(),
                                    _ => SubscriptionInfo::default(),
                                };
                            }
                        }
                        let cleanup_path = previous_source.as_ref().and_then(|previous| {
                            let old_path = previous.materialized().file.clone();
                            (definition.source().is_none() || old_path != canonical)
                                .then_some(old_path)
                        });
                        // A changed remote definition receives a durable empty
                        // placeholder instead of retaining stale bytes. Its next
                        // refresh replaces it through the file-first protocol.
                        let resource = if same_slot {
                            Ok(None)
                        } else {
                            Self::resource_for_definition(state, &definition, None).await
                        };
                        match resource {
                            Err(error) => Err(error),
                            Ok(resource) => {
                                let mut next = before.clone();
                                let item = next
                                    .items
                                    .get_mut(&uid)
                                    .expect("replacement target remains in the candidate snapshot");
                                item.set_definition(definition);
                                Self::commit_with_resources(
                                    &myself,
                                    state,
                                    expected_version,
                                    before,
                                    next,
                                    AffectsRule::Touched(uid),
                                    resource.map(|resource| (canonical, resource)),
                                    cleanup_path,
                                    None,
                                )
                                .await
                            }
                        }
                    }
                };
                let _ = reply.send(result);
            }
            ProfilesActorMessage::ReconcileMaterializations => {
                if state.gate != ProducerGate::Running {
                    return Ok(());
                }
                match Self::reconcile_materializations(state).await {
                    Ok(report) => Self::log_reconcile_report(&report),
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "profile materialization reconcile failed"
                        );
                    }
                }
            }
            ProfilesActorMessage::StartProducers => Self::start_producers(&myself, state),
            ProfilesActorMessage::StopProducers { reply } => {
                let stopped = Self::stop_producers(state).await;
                let _ = reply.send(stopped);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(handle) = state.reconcile_task.take() {
            handle.abort();
        }
        state.scheduler.shutdown();
        state.external_watchers.shutdown();
        for (_, pending) in state.pending_refresh.drain() {
            pending.task.abort();
        }
        for (_, pending) in state.pending_imports.drain() {
            pending.task.abort();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-2 review fix regression pins: Config subscriptions must carry
    /// proxies (legacy remote.rs semantics); overlays only need a mapping.
    #[test]
    fn config_content_requires_proxies_key() {
        let config = crate::enhance::golden_support::file_config("p1", "p1.yaml", &[]);
        assert!(ProfilesActor::validate_fetched_content(&config.definition, "{}\n").is_err());
        assert!(
            ProfilesActor::validate_fetched_content(&config.definition, "proxies: []\n").is_ok()
        );
        assert!(
            ProfilesActor::validate_fetched_content(&config.definition, "proxy-providers: {}\n")
                .is_ok()
        );
    }

    #[test]
    fn overlay_content_needs_only_a_mapping() {
        let overlay = crate::enhance::golden_support::overlay("t1", "t1.yaml");
        assert!(ProfilesActor::validate_fetched_content(&overlay.definition, "a: 1\n").is_ok());
    }

    #[test]
    fn synced_name_syncs_only_unpinned_profiles_with_a_server_name() {
        // Not user-named + server name present -> adopt it.
        assert_eq!(
            synced_name(false, &Some("Server Name".into())),
            Some("Server Name".into())
        );
        // User-named -> never overwritten, even with a server name.
        assert_eq!(synced_name(true, &Some("Server Name".into())), None);
        // No server name / blank server name -> keep the current name.
        assert_eq!(synced_name(false, &None), None);
        assert_eq!(synced_name(false, &Some("   ".into())), None);
    }
}
