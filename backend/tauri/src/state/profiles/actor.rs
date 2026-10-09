//! ProfilesActor: single owner of the profiles document.
//! Tauri-free; every filesystem/network effect goes through the ports.

use nyanpasu_core::{
    migration::modules::profiles::ProfilesFormat,
    profiles::{
        current_closure,
        error::{
            EmptyScriptSnafu, MissingProxiesSnafu, NotYamlMappingSnafu, ProfileContentError,
            ProfileFileError,
        },
        ports::{
            MaterializationReconcileReport, MaterializationResource, PreparedCleanup,
            ProfileDegradation, ProfileDegradationCode, ProfileDegradationPhase, ProfileFsPort,
            ProfileMaterializationPort, SubscriptionFetcher,
        },
    },
};
use std::{collections::HashMap, sync::Arc};

use nyanpasu_config::profile::{
    ConfigDefinition, ExternalMode, ExternalProfilePath, FileConfig, LocalBinding,
    ManagedProfilePath, MaterializedFile, OverlayTransform, ProfileDefinition,
    ProfileDependencyIndex, ProfileId, ProfileItem, ProfileMetadata, ProfileMetadataPatch,
    ProfileSource, Profiles, RemoteProfileOptions, RemoteProfileOptionsPatch, ScriptRuntime,
    ScriptTransform, SubscriptionInfo, TransformDefinition, TransformKind,
};
use nyanpasu_core::state::{
    PersistentStateManager, ReplaceIfVersionError, ReplaceIfVersionResult, Version,
    error::StateChangedError,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use snafu::{IntoError, OptionExt, ResultExt, ensure};

use crate::{
    client::application_workflow::{
        impact::{
            self, ActivationIntent, ContentDigest, MutationHints, RequestedRuntimeFields,
            StagedResourceToken, TouchedContent,
        },
        policy::CommandClass,
    },
    state::mutation::{CommitAborted, MutationCoordinator},
};
use nyanpasu_core_manager::OperationId;
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use super::{
    error::*,
    jobs::{ProfileJobs, ProfileSyncContext},
    scheduler::ExternalWatchers,
    sources::{SourceLedger, SourceOrigin, SourceOutcome, SourcesSnapshot},
};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CommitReport {
    pub snapshot: Arc<Profiles>,
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
    pub(crate) jobs: ProfileJobs,
    /// Once cancelled, every write and every producer is refused.
    pub shutdown: CancellationToken,
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
    jobs: ProfileJobs,
    jobs_revision: u64,
    external_watchers: ExternalWatchers,
    reconcile_task: Option<JoinHandle<()>>,
    sources: SourceLedger,
    shutdown: CancellationToken,
}

/// Whether background producers may run (T10 §2.2). Setup holds them until
/// StartupReconcile has proven the runtime; the shutdown token stops them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProducerGate {
    Held,
    Running,
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
    context: Option<nyanpasu_jobs::JobContext>,
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
    transform: Option<TransformKind>,
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

/// How a refresh attempt that reached its commit handler ended.
enum RefreshConclusion {
    Committed(CommitReport),
    Superseded(ProfilesError),
    Failed(ProfilesError),
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
        error: ProfilesError,
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
    SyncRemote {
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
        context: ProfileSyncContext,
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
        /// `None` imports a Config File; `Some` imports a Transform of that kind.
        transform: Option<TransformKind>,
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
}

impl ProfilesActorMessage {
    /// Answers a message that reached the actor once the shutdown began: a
    /// write is refused, and producer input is dropped. A completion's caller
    /// is answered by `post_stop` with its pending entry.
    fn refuse(self) {
        match self {
            Self::SaveFile { reply, .. }
            | Self::SetCurrent { reply, .. }
            | Self::SetGlobalTransforms { reply, .. }
            | Self::SetValidFields { reply, .. }
            | Self::Replace { reply, .. }
            | Self::Add { reply, .. }
            | Self::Delete { reply, .. }
            | Self::Reorder { reply, .. }
            | Self::PatchMetadata { reply, .. }
            | Self::PatchRemoteOptions { reply, .. }
            | Self::ImportRemote { reply, .. }
            | Self::ReplaceDefinition { reply, .. } => {
                let _ = reply.send(Err(ProfilesError::ShuttingDown));
            }
            Self::SetCurrentIfNone { reply, .. } => {
                let _ = reply.send(Err(ProfilesError::ShuttingDown));
            }
            Self::SyncRemote { reply, .. } => {
                let _ = reply.send(Err(ProfilesError::ShuttingDown));
            }
            Self::RefreshRemote { reply, .. } => {
                if let Some(reply) = reply {
                    let _ = reply.send(Err(ProfilesError::ShuttingDown));
                }
            }
            Self::CommitRefreshed { .. }
            | Self::CommitImported { .. }
            | Self::ExternalFileChanged { .. }
            | Self::ReconcileMaterializations
            | Self::StartProducers => {}
        }
    }
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

    fn prepare_candidate(mut next: Profiles) -> Result<Profiles, ProfilesError> {
        if let Err(errors) = next.validate() {
            return ValidationFailedSnafu { errors }.fail();
        }
        next.bump_revision()?;
        Ok(next)
    }

    /// Persist an already validated, revision-bumped candidate exactly as prepared.
    async fn persist_candidate(
        state: &mut ProfilesActorState,
        expected_version: Version,
        before: &Profiles,
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
        state.mutations.ensure_ready()?;
        // Only a mutation that reaches the runtime takes the Runtime into its
        // transaction; any other save commits on its own.
        let (operation, result, settlement) =
            match impact::runtime_impact(before, &next, &hints, class) {
                Some(impact) => {
                    let operation = OperationId::generate();
                    let (participant, settlement) = state
                        .mutations
                        .participant(operation, hints, class, impact)?;
                    let result = state
                        .manager
                        .replace_if_version_with_participant(
                            expected_version,
                            next.clone(),
                            participant,
                            || async { Ok(()) },
                            || async { Ok(()) },
                        )
                        .await;
                    (Some(operation), result, settlement.await.ok())
                }
                None => (
                    None,
                    state
                        .manager
                        .replace_if_version(expected_version, next.clone())
                        .await,
                    None,
                ),
            };
        match result {
            Ok(ReplaceIfVersionResult::Replaced) => {
                state.mutations.effects().profiles_committed();
                let completion = state.mutations.committed(
                    operation,
                    "profiles",
                    *state.manager.snapshot_handle().load().version.as_ref(),
                    settlement,
                );
                Ok((Arc::new(next), completion))
            }
            Ok(ReplaceIfVersionResult::Conflict { actual_version }) => VersionConflictSnafu {
                expected: *expected_version.as_ref(),
                actual: *actual_version.as_ref(),
                cleanup_failures: Vec::new(),
            }
            .fail(),
            Err(error) => Err(Self::commit_failure(error, settlement.as_ref(), Vec::new())),
        }
    }

    /// The one place a persistence error becomes a profiles error. A CAS
    /// mismatch reported by the coordinator is the same conflict as the
    /// manager's own `Conflict` result.
    fn commit_failure(
        error: ReplaceIfVersionError,
        settlement: Option<&crate::client::application_workflow::mutation::MutationReceipt>,
        cleanup_failures: Vec<ProfilesError>,
    ) -> ProfilesError {
        match error {
            ReplaceIfVersionError::State(StateChangedError::StateCasMismatch {
                expected,
                actual,
            }) => VersionConflictSnafu {
                expected: *expected.as_ref(),
                actual: *actual.as_ref(),
                cleanup_failures,
            }
            .build(),
            error => CommitSnafu { cleanup_failures }
                .into_error(CommitAborted::classify(error, settlement)),
        }
    }

    async fn reconcile_committed(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        snapshot: &Profiles,
    ) {
        state.index = ProfileDependencyIndex::build(snapshot);
        state.sources.reconcile(snapshot);
        state.jobs_revision += 1;
        if let Err(error) = state
            .jobs
            .reconcile(
                snapshot,
                myself,
                state.jobs_revision,
                state.gate == ProducerGate::Running,
            )
            .await
        {
            tracing::error!(%error, "failed to reconcile profile jobs");
        }
        if state.gate == ProducerGate::Running {
            state.external_watchers.reconcile(snapshot, myself);
        }
    }

    async fn start_producers(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
    ) {
        if state.gate != ProducerGate::Held {
            return;
        }
        state.gate = ProducerGate::Running;
        let snapshot = Self::current_state(state);
        Self::reconcile_committed(myself, state, &snapshot).await;
        state.jobs.catch_up(&snapshot).await;
        let actor = myself.clone();
        state.reconcile_task = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5 * 60));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
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

    /// Downloads and validates on a task the pending entry owns; the file is
    /// written by the commit handler, after its stale-download fence. An
    /// abort settles nothing: whoever aborts has already removed the entry.
    fn spawn_download(
        fetcher: Arc<dyn SubscriptionFetcher>,
        url: url::Url,
        option: RemoteProfileOptions,
        definition: ProfileDefinition,
        context: Option<nyanpasu_jobs::JobContext>,
        settle: impl FnOnce(RefreshOutcome) + Send + 'static,
    ) -> JoinHandle<()> {
        let download = async move {
            let fetch = async {
                tracing::info!(target: "nyanpasu::profile_sync", stage = "download", "Downloading profile");
                let fetched = fetcher
                    .fetch(&url, &option)
                    .await
                    .context(FetchSubscriptionSnafu { url: url.clone() })?;
                Self::validate_fetched_content(&definition, &fetched.content)
                    .context(ProfileContentRejectedSnafu)?;
                tracing::info!(target: "nyanpasu::profile_sync", stage = "validated", "Profile content validated");
                Ok::<_, ProfilesError>(fetched)
            };
            let outcome = match fetch.await {
                Ok(fetched) => RefreshOutcome::Succeeded {
                    subscription: fetched.subscription,
                    suggested_update_interval_minutes: fetched.suggested_update_interval_minutes,
                    content: fetched.content,
                    filename: fetched.filename,
                },
                Err(error) => RefreshOutcome::Failed { error },
            };
            settle(outcome);
        };
        match context {
            Some(context) => context
                .spawn(download)
                .expect("the running sync owns an open child scope"),
            None => tokio::spawn(download),
        }
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
        let expected_fingerprint = match Self::fingerprint(uid, &definition) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                return SourceOutcome::Failed {
                    message: error.report(),
                };
            }
        };
        let content = match Self::read_external(&state.fs, &expected_target).await {
            Ok(content) => content,
            Err(error) => {
                return SourceOutcome::Failed {
                    message: error.report(),
                };
            }
        };
        if let Err(error) = Self::validate_fetched_content(&definition, &content) {
            return SourceOutcome::rejected(
                SourceOutcome::EXTERNAL_SOURCE_REJECTED,
                snafu::Report::from_error(error),
            );
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
            Err(error) => {
                SourceOutcome::rejected(SourceOutcome::EXTERNAL_SOURCE_REJECTED, error.report())
            }
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
            RefreshConclusion::Superseded(
                ProfileChangedDuringRefreshSnafu { uid: uid.clone() }.build(),
            )
        };
        let Some(current) = before.items.get(uid) else {
            return RefreshConclusion::Superseded(
                ProfileDeletedDuringRefreshSnafu { uid: uid.clone() }.build(),
            );
        };
        let current_fingerprint = match Self::fingerprint(uid, &current.definition) {
            Ok(fingerprint) => fingerprint,
            Err(error) => return RefreshConclusion::Failed(error),
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
            RefreshOutcome::Failed { error } => return RefreshConclusion::Failed(error),
            RefreshOutcome::Succeeded {
                subscription,
                suggested_update_interval_minutes: _,
                content,
                filename,
            } => (subscription, content, filename),
        };
        if let Err(error) = Self::validate_fetched_content(&current.definition, &content) {
            return RefreshConclusion::Superseded(ProfileContentRejectedSnafu.into_error(error));
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
            Self::persist_candidate(state, expected_version, &before, candidate, hints, class)
                .await?;
        Self::reconcile_committed(myself, state, &snapshot).await;
        Ok(CommitReport {
            snapshot,
            degradations: Vec::new(),
            receipt,
            runtime_degradations,
            created: None,
        })
    }

    /// Awaits a blocking task. A panic in it is resumed, never turned into an
    /// error; the join only fails otherwise when the runtime cancelled the task.
    async fn blocking<T>(task: JoinHandle<T>) -> Result<T, ProfilesError> {
        match task.await {
            Ok(value) => Ok(value),
            Err(error) => match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(error) => Err(BlockingTaskCancelledSnafu.into_error(error)),
            },
        }
    }

    async fn materialization_call<T, F>(
        state: &ProfilesActorState,
        operation: MaterializationOperation,
        call: F,
    ) -> Result<T, ProfilesError>
    where
        T: Send + 'static,
        F: FnOnce(&dyn ProfileMaterializationPort) -> Result<T, ProfileFileError> + Send + 'static,
    {
        let materialization = Arc::clone(&state.materialization);
        Self::blocking(tokio::task::spawn_blocking(move || {
            call(materialization.as_ref())
        }))
        .await?
        .context(MaterializationSnafu { operation })
    }

    async fn read_external(
        fs: &Arc<dyn ProfileFsPort>,
        target: &ExternalProfilePath,
    ) -> Result<String, ProfilesError> {
        let fs = Arc::clone(fs);
        let read_target = target.clone();
        Self::blocking(tokio::task::spawn_blocking(move || {
            fs.read_external(&read_target)
        }))
        .await?
        .context(ReadExternalProfileSnafu {
            target: target.clone(),
        })
    }

    fn fingerprint(
        uid: &ProfileId,
        definition: &ProfileDefinition,
    ) -> Result<String, ProfilesError> {
        serde_yaml::to_string(definition).context(FingerprintDefinitionSnafu { uid: uid.clone() })
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
                let content = Self::read_external(&state.fs, target).await?;
                Self::validate_fetched_content(definition, &content)
                    .context(ProfileContentRejectedSnafu)?;
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
        transform: Option<TransformKind>,
        option: RemoteProfileOptions,
        file: ManagedProfilePath,
        subscription: SubscriptionInfo,
        updated_at: Option<time::OffsetDateTime>,
    ) -> ProfileDefinition {
        let source = ProfileSource::Remote {
            materialized: MaterializedFile { file, updated_at },
            url,
            option,
            subscription,
        };
        match transform {
            None => ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source,
                    transforms: vec![],
                }),
            },
            Some(TransformKind::Overlay) => ProfileDefinition::Transform {
                transform: TransformDefinition::Overlay(OverlayTransform { source }),
            },
            Some(TransformKind::Script { runtime }) => ProfileDefinition::Transform {
                transform: TransformDefinition::Script(ScriptTransform { source, runtime }),
            },
        }
    }

    /// Validate URL/options against the live document without writing state/files.
    fn validate_import_request(
        before: &Profiles,
        metadata: &ProfileMetadata,
        url: url::Url,
        transform: Option<TransformKind>,
        option: RemoteProfileOptions,
    ) -> Result<(), ProfilesError> {
        let definition = Self::remote_import_definition(
            url,
            transform,
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
        let collision = uid.clone();
        ensure!(
            next.append_item(ProfileItem {
                uid,
                metadata: metadata.clone(),
                definition,
            }),
            ProfileIdCollisionSnafu { uid: collision }
        );
        if let Err(errors) = next.validate() {
            return ValidationFailedSnafu { errors }.fail();
        }
        Ok(())
    }

    async fn finish_cleanup(
        state: &ProfilesActorState,
        cleanup: PreparedCleanup,
        snapshot: &Profiles,
    ) -> Vec<ProfileDegradation> {
        let retry = cleanup.clone();
        if let Err(error) = Self::materialization_call(
            state,
            MaterializationOperation::ActivateCleanup,
            move |port| port.activate_cleanup(&cleanup),
        )
        .await
        {
            return vec![ProfileDegradation {
                phase: ProfileDegradationPhase::Cleanup,
                code: ProfileDegradationCode::CleanupDeferred,
                message: format!("profile cleanup activation deferred: {}", error.report()),
            }];
        }
        let profiles = snapshot.clone();
        if let Err(error) =
            Self::materialization_call(state, MaterializationOperation::RetryCleanup, move |port| {
                port.retry_cleanup(&retry, &profiles)
            })
            .await
        {
            return vec![ProfileDegradation {
                phase: ProfileDegradationPhase::Cleanup,
                code: ProfileDegradationCode::CleanupDeferred,
                message: format!("profile cleanup retry deferred: {}", error.report()),
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
        state.mutations.ensure_ready()?;
        let (mut hints, class) = Self::mutation_hints(&affects, &candidate);
        let prepared = if let Some((path, resource)) = resource {
            let content = match &resource {
                MaterializationResource::File { content } => Some(content.clone()),
                MaterializationResource::Symlink { .. }
                    if !current_closure(&candidate).iter().any(|uid| {
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
                    Some(Self::read_external(&state.fs, target).await?)
                }
            };
            hints.touched.push(TouchedContent {
                path: path.clone(),
                content_digest: content.as_ref().map(|content| {
                    ContentDigest::new(nyanpasu_core_manager::payload_digest(content.as_bytes()))
                }),
                resource: None,
            });
            if let Some(content) = content {
                hints.staged_content.insert(path.to_string(), content);
            }
            Some(
                Self::materialization_call(
                    state,
                    MaterializationOperation::PrepareFileFirst,
                    move |port| port.prepare_file_first(&path, resource, expected_revision),
                )
                .await?,
            )
        } else {
            None
        };
        let cleanup = if let Some(path) = cleanup_path {
            match Self::materialization_call(
                state,
                MaterializationOperation::PrepareCleanup,
                move |port| port.prepare_cleanup(&path, expected_revision),
            )
            .await
            {
                Ok(cleanup) => Some(cleanup),
                Err(error) => {
                    if let Some(prepared) = prepared {
                        Self::materialization_call(
                            state,
                            MaterializationOperation::Compensate,
                            move |port| port.compensate(&prepared),
                        )
                        .await?;
                    }
                    return Err(error);
                }
            }
        } else {
            None
        };
        // Only a mutation that reaches the runtime takes the Runtime into its
        // transaction, under an operation that also addresses what it staged;
        // any other save commits on its own.
        let admission = impact::runtime_impact(&before, &candidate, &hints, class)
            .map(|impact| (OperationId::generate(), impact));
        if let Some((operation, _)) = admission {
            for touched in &mut hints.touched {
                touched.resource = Some(StagedResourceToken::new(operation.to_string()));
            }
        }
        let write_port = state.materialization.clone();
        let write_resource = prepared.clone();
        let recover_port = state.materialization.clone();
        let recover_resource = prepared.clone();
        let recover_cleanup = cleanup.clone();
        // nyanpasu-core's local write and recovery steps return anyhow; the typed
        // port error is kept as their source and shows in the abort's detail.
        let write = move || async move {
            tokio::task::spawn_blocking(move || {
                if let Some(prepared) = write_resource {
                    write_port.promote(&prepared)?;
                }
                anyhow::Ok(())
            })
            .await
            .map_err(|error| match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(error) => error,
            })?
        };
        let recover = move || async move {
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
            .await
            .map_err(|error| match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(error) => error,
            })?
        };
        let (result, settlement) = match admission {
            Some((operation, impact)) => {
                let (participant, settlement) = state
                    .mutations
                    .participant(operation, hints, class, impact)?;
                let result = state
                    .manager
                    .replace_if_version_with_participant(
                        expected_version,
                        candidate.clone(),
                        participant,
                        write,
                        recover,
                    )
                    .await;
                (result, settlement.await.ok())
            }
            None => (
                state
                    .manager
                    .replace_if_version_with_local_write(
                        expected_version,
                        candidate.clone(),
                        write,
                        recover,
                    )
                    .await,
                None,
            ),
        };
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
                        if let Err(error) = Self::materialization_call(
                            state,
                            MaterializationOperation::Compensate,
                            move |port| port.compensate(&prepared),
                        )
                        .await
                        {
                            failures.push(error);
                        }
                    }
                    if let Some(cleanup) = cleanup {
                        if let Err(error) = Self::materialization_call(
                            state,
                            MaterializationOperation::CancelCleanup,
                            move |port| port.cancel_cleanup(&cleanup),
                        )
                        .await
                        {
                            failures.push(error);
                        }
                    }
                }
                return Err(match result {
                    Err(error) => Self::commit_failure(error, settlement.as_ref(), failures),
                    Ok(ReplaceIfVersionResult::Conflict { actual_version }) => {
                        VersionConflictSnafu {
                            expected: *expected_version.as_ref(),
                            actual: *actual_version.as_ref(),
                            cleanup_failures: failures,
                        }
                        .build()
                    }
                    Ok(ReplaceIfVersionResult::Replaced) => unreachable!(),
                });
            }
        }
        state.mutations.effects().profiles_committed();
        let (receipt, runtime_degradations) = state.mutations.committed(
            admission.map(|(operation, _)| operation),
            "profiles",
            *state.manager.snapshot_handle().load().version.as_ref(),
            settlement,
        );
        let snapshot = Arc::new(candidate);
        Self::reconcile_committed(myself, state, &snapshot).await;
        let mut degradations = Vec::new();
        if let Some(prepared) = prepared {
            if let Err(error) =
                Self::materialization_call(state, MaterializationOperation::Complete, move |port| {
                    port.complete(&prepared)
                })
                .await
            {
                degradations.push(ProfileDegradation {
                    phase: ProfileDegradationPhase::Reconcile,
                    code: ProfileDegradationCode::MaterializationDeferred,
                    message: format!("materialization completion deferred: {}", error.report()),
                });
            }
        }
        if let Some(cleanup) = cleanup {
            degradations.extend(Self::finish_cleanup(state, cleanup, &snapshot).await);
        }
        Ok(CommitReport {
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
    ) -> Result<MaterializationReconcileReport, ProfilesError> {
        let snapshot = Self::current_state(state);
        Self::materialization_call(state, MaterializationOperation::Reconcile, move |port| {
            port.reconcile(&snapshot)
        })
        .await
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
    ) -> Result<(), ProfileContentError> {
        let needs_yaml = match definition {
            ProfileDefinition::Config { .. } => true,
            ProfileDefinition::Transform { transform } => {
                matches!(transform, TransformDefinition::Overlay(_))
            }
        };
        if needs_yaml {
            let mapping = serde_yaml::from_str::<serde_yaml::Mapping>(content)
                .context(NotYamlMappingSnafu)?;
            // Legacy subscription semantics (remote.rs BC): a Config
            // subscription must actually carry proxies, otherwise arbitrary
            // mappings (e.g. `{}`) get persisted and can be auto-activated.
            ensure!(
                !matches!(definition, ProfileDefinition::Config { .. })
                    || mapping.contains_key("proxies")
                    || mapping.contains_key("proxy-providers"),
                MissingProxiesSnafu
            );
            Ok(())
        } else {
            ensure!(!content.trim().is_empty(), EmptyScriptSnafu);
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
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        // Startup recovery must finish before mutations are admitted; the
        // producers wait for StartProducers. Blocking port work stays off the
        // async runtime via spawn_blocking.
        let loaded = args.manager.snapshot_handle().load().state.clone();
        let materialization = Arc::clone(&args.materialization);
        let report = tokio::task::spawn_blocking(move || materialization.reconcile(&loaded))
            .await
            .map_err(|error| match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(error) => ActorProcessingErr::from(anyhow::anyhow!(
                    "startup materialization reconcile join failed: {error}"
                )),
            })?
            .map_err(|error| {
                ActorProcessingErr::from(anyhow::anyhow!(
                    "startup materialization reconcile failed: {error}"
                ))
            })?;
        Self::log_reconcile_report(&report);

        args.jobs
            .reconcile(
                &args.manager.snapshot_handle().load().state,
                &myself,
                1,
                false,
            )
            .await?;
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
            jobs: args.jobs,
            jobs_revision: 1,
            external_watchers: ExternalWatchers::default(),
            reconcile_task: None,
            sources: SourceLedger::new(args.sources),
            shutdown: args.shutdown,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if state.shutdown.is_cancelled() {
            message.refuse();
            return Ok(());
        }
        let (message, job_context) = match message {
            ProfilesActorMessage::SyncRemote {
                uid,
                patch,
                context,
                reply,
            } => {
                let origin = if context.0.trigger == nyanpasu_jobs::Trigger::Manual {
                    RefreshOrigin::Manual
                } else {
                    RefreshOrigin::Scheduled
                };
                (
                    ProfilesActorMessage::RefreshRemote {
                        uid,
                        patch,
                        origin,
                        reply: Some(reply),
                    },
                    Some(context.0),
                )
            }
            message => (message, None),
        };
        match message {
            ProfilesActorMessage::SyncRemote { .. } => unreachable!(),
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
                        .context(ProfileNotFoundSnafu { uid: uid.clone() })?;
                    let source = item
                        .definition
                        .source()
                        .context(ProfileHasNoFileSnafu { uid: uid.clone() })?;
                    let path = match source {
                        ProfileSource::Local {
                            binding: LocalBinding::Managed { materialized },
                        } => materialized.file.clone(),
                        _ => return ProfileFileNotWritableSnafu { uid }.fail(),
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
                                ProfileIdCollisionSnafu { uid }.fail()
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
                    ProfileNotFoundSnafu { uid: uid.clone() }.fail()
                } else if let Some((referrers, current, global_transforms)) =
                    Self::referrers_of(state, &before, &uid)
                {
                    ProfileInUseSnafu {
                        uid: uid.clone(),
                        referrers,
                        current,
                        global_transforms,
                    }
                    .fail()
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
                                return ProfileNotFoundSnafu { uid: active }.fail();
                            }
                            if profiles.items.get(&over).is_none() {
                                return ProfileNotFoundSnafu { uid: over }.fail();
                            }
                            profiles.reorder(&active, &over);
                        }
                        ReorderOp::ByList(list) => {
                            ensure!(
                                list.len() == profiles.items.len(),
                                ReorderListSizeMismatchSnafu {
                                    expected: profiles.items.len(),
                                    got: list.len()
                                }
                            );
                            let mut seen = indexmap::IndexSet::with_capacity(list.len());
                            for uid in &list {
                                ensure!(
                                    seen.insert(uid.clone()),
                                    ReorderListDuplicateSnafu { uid: uid.clone() }
                                );
                                ensure!(
                                    profiles.items.get(uid).is_some(),
                                    ProfileNotFoundSnafu { uid: uid.clone() }
                                );
                            }
                            let mut reordered = indexmap::IndexMap::with_capacity(list.len());
                            for uid in list {
                                let item = profiles
                                    .items
                                    .shift_remove(&uid)
                                    .context(ProfileNotFoundSnafu { uid: uid.clone() })?;
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
                        return ProfileNotFoundSnafu { uid }.fail();
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
                        return ProfileNotFoundSnafu { uid }.fail();
                    };
                    match item.definition.source_mut() {
                        Some(ProfileSource::Remote { option, .. }) => {
                            use struct_patch::Patch as _;
                            option.apply(patch);
                            Ok(AffectsRule::Never)
                        }
                        _ => NotARemoteProfileSnafu { uid }.fail(),
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
                if origin == RefreshOrigin::Scheduled && state.gate == ProducerGate::Held {
                    return Ok(());
                }
                if state.pending_refresh.contains_key(&uid) {
                    // A tick folds into the download in flight without a receipt
                    // of its own: that download reports the outcome, and a
                    // healthy row now would hide the last failure until then.
                    if let Some(reply) = reply {
                        let _ = reply.send(RefreshInProgressSnafu { uid }.fail());
                    }
                    return Ok(());
                }

                if let Some(patch) = patch {
                    let patched = Self::run_state_write(&myself, state, {
                        let uid = uid.clone();
                        move |profiles| {
                            let Some(item) = profiles.items.get_mut(&uid) else {
                                return ProfileNotFoundSnafu { uid }.fail();
                            };
                            match item.definition.source_mut() {
                                Some(ProfileSource::Remote { option, .. }) => {
                                    use struct_patch::Patch as _;
                                    option.apply(patch);
                                    Ok(AffectsRule::Never)
                                }
                                _ => NotARemoteProfileSnafu { uid }.fail(),
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
                        let _ = reply.send(ProfileNotFoundSnafu { uid }.fail());
                    }
                    return Ok(());
                };
                let Some(ProfileSource::Remote { url, option, .. }) = item.definition.source()
                else {
                    if let Some(reply) = reply {
                        let _ = reply.send(NotARemoteProfileSnafu { uid }.fail());
                    }
                    return Ok(());
                };

                let definition = item.definition.clone();
                let definition_fingerprint = match Self::fingerprint(&uid, &definition) {
                    Ok(fingerprint) => fingerprint,
                    Err(error) => {
                        Self::record_source(
                            state,
                            &uid,
                            origin.source(),
                            SourceOutcome::Failed {
                                message: error.report(),
                            },
                        );
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(error));
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
                    job_context.clone(),
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
                        context: job_context,
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
                let conclude = Self::conclude_refresh(
                    &myself,
                    state,
                    &uid,
                    url,
                    definition_fingerprint,
                    outcome,
                );
                let conclusion = match &pending.context {
                    Some(context) => context.instrument(conclude).await,
                    None => conclude.await,
                };
                let (outcome, result) = match conclusion {
                    RefreshConclusion::Committed(report) => (
                        SourceOutcome::Committed {
                            operation_id: report.receipt.operation_id.clone(),
                        },
                        Ok(report),
                    ),
                    RefreshConclusion::Superseded(error) => (
                        SourceOutcome::Superseded {
                            reason: error.report(),
                        },
                        Err(error),
                    ),
                    RefreshConclusion::Failed(error) => (
                        SourceOutcome::Failed {
                            message: error.report(),
                        },
                        Err(error),
                    ),
                    RefreshConclusion::Rejected(error) => (
                        SourceOutcome::rejected(
                            SourceOutcome::SUBSCRIPTION_REJECTED,
                            error.report(),
                        ),
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
                transform,
                metadata,
                option,
                update_interval_explicit,
                reply,
            } => {
                let before = Self::current_state(state);
                if let Err(error) = Self::validate_import_request(
                    &before,
                    &metadata,
                    url.clone(),
                    transform,
                    option.clone(),
                ) {
                    let _ = reply.send(Err(error));
                    return Ok(());
                }

                let token = ImportOperationToken(state.next_import_token);
                state.next_import_token = state.next_import_token.wrapping_add(1).max(1);
                let actor = myself.clone();
                let definition_for_validation = Self::remote_import_definition(
                    url.clone(),
                    transform,
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
                    None,
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
                        transform,
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
                    RefreshOutcome::Failed { error } => Err(error),
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
                            pending.transform,
                            option,
                            ManagedProfilePath::new("pending.yaml")
                                .expect("static managed path is valid"),
                            subscription,
                            Some(time::OffsetDateTime::now_utc()),
                        );
                        if let Err(error) = Self::validate_fetched_content(&definition, &content) {
                            Err(ProfileContentRejectedSnafu.into_error(error))
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
                                ProfileIdCollisionSnafu { uid }.fail()
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
                                return ProfileNotFoundSnafu { uid }.fail();
                            };
                            match item.definition.source_mut() {
                                Some(ProfileSource::Local {
                                    binding: LocalBinding::External { materialized, .. },
                                }) => {
                                    materialized.updated_at = Some(time::OffsetDateTime::now_utc());
                                    Ok(AffectsRule::Touched(uid))
                                }
                                _ => ProfileNotFoundSnafu { uid }.fail(),
                            }
                        }
                    })
                    .await;
                    match result {
                        Ok(report) => SourceOutcome::Committed {
                            operation_id: report.receipt.operation_id,
                        },
                        Err(error) => SourceOutcome::rejected(
                            SourceOutcome::EXTERNAL_SOURCE_REJECTED,
                            error.report(),
                        ),
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
                    None => ProfileNotFoundSnafu { uid: uid.clone() }.fail(),
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
                            error = %error.report(),
                            "profile materialization reconcile failed"
                        );
                    }
                }
            }
            ProfilesActorMessage::StartProducers => Self::start_producers(&myself, state).await,
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(timer) = state.reconcile_task.take() {
            timer.abort();
        }
        state.external_watchers.shutdown();
        // The downloads are cut short and awaited, so none outlives the actor;
        // their callers learn that the application is shutting down.
        let mut downloads = Vec::new();
        for (_, pending) in state.pending_refresh.drain() {
            pending.task.abort();
            downloads.push((pending.task, pending.reply));
        }
        for (_, pending) in state.pending_imports.drain() {
            pending.task.abort();
            downloads.push((pending.task, Some(pending.reply)));
        }
        for (task, reply) in downloads {
            if let Err(error) = task.await
                && let Ok(panic) = error.try_into_panic()
            {
                std::panic::resume_unwind(panic);
            }
            if let Some(reply) = reply {
                let _ = reply.send(Err(ProfilesError::ShuttingDown));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// V13, profiles: whether an operation reaches the runtime, for the hints
    /// and class this actor derives from the operation's rule. Selecting the
    /// current profile again and re-setting the same global transforms still
    /// ask for the runtime, and so does a definition change of the running
    /// profile, whatever produced it.
    #[test]
    fn profiles_operations_reach_the_runtime_by_request_and_diff() {
        use crate::{
            client::application_workflow::impact::{RuntimeImpact, runtime_impact},
            enhance::golden_support::{file_config, metadata, overlay},
        };

        // current -> `sel` with its scoped transform `scoped`; `global` is a
        // document-level transform; `spare` is reachable from neither.
        let mut base = Profiles::default();
        base.append_item(file_config("sel", "sel.yaml", &["scoped"]));
        base.append_item(overlay("scoped", "scoped.yaml"));
        base.append_item(overlay("global", "global.yaml"));
        base.append_item(file_config("spare", "spare.yaml", &[]));
        base.current = Some(ProfileId("sel".into()));
        base.global_transforms = vec![ProfileId("global".into())];

        // The same document with a remote profile as the current one.
        let mut remote = base.clone();
        remote.append_item(ProfileItem {
            uid: ProfileId("remote".into()),
            metadata: metadata("remote"),
            definition: ProfilesActor::remote_import_definition(
                "https://example.com/sub".parse().unwrap(),
                None,
                RemoteProfileOptions::default(),
                ManagedProfilePath::new("remote.yaml").unwrap(),
                SubscriptionInfo::default(),
                None,
            ),
        });
        remote.current = Some(ProfileId("remote".into()));

        let id = |uid: &str| ProfileId(uid.into());
        let with = |from: &Profiles, change: &dyn Fn(&mut Profiles)| {
            let mut next = from.clone();
            change(&mut next);
            next
        };
        let reconcile = Some(RuntimeImpact::Reconcile);
        let cases: Vec<(&str, AffectsRule, Profiles, Profiles, Option<RuntimeImpact>)> = vec![
            (
                "select another profile",
                AffectsRule::CurrentChanged,
                base.clone(),
                with(&base, &|p| p.current = Some(id("spare"))),
                reconcile,
            ),
            (
                "select the current profile again",
                AffectsRule::CurrentChanged,
                base.clone(),
                base.clone(),
                reconcile,
            ),
            (
                "change the global transforms",
                AffectsRule::GlobalChanged,
                base.clone(),
                with(&base, &|p| p.global_transforms.clear()),
                reconcile,
            ),
            (
                "set the same global transforms",
                AffectsRule::GlobalChanged,
                base.clone(),
                base.clone(),
                reconcile,
            ),
            (
                "change the valid fields",
                AffectsRule::Always,
                base.clone(),
                with(&base, &|p| p.valid = vec!["dns".into()]),
                reconcile,
            ),
            (
                "set the same valid fields",
                AffectsRule::Always,
                base.clone(),
                base.clone(),
                reconcile,
            ),
            (
                "save the current profile's file",
                AffectsRule::Touched(id("sel")),
                base.clone(),
                base.clone(),
                reconcile,
            ),
            (
                "save a scoped transform's file",
                AffectsRule::Touched(id("scoped")),
                base.clone(),
                base.clone(),
                reconcile,
            ),
            (
                "save an unrelated profile's file",
                AffectsRule::Touched(id("spare")),
                base.clone(),
                base.clone(),
                None,
            ),
            (
                "patch the options of the current remote profile",
                AffectsRule::Never,
                remote.clone(),
                with(&remote, &|p| {
                    let item = p.items.get_mut(&id("remote")).unwrap();
                    let Some(ProfileSource::Remote { option, .. }) = item.definition.source_mut()
                    else {
                        unreachable!("the fixture is remote")
                    };
                    option.update_interval_minutes += 1;
                }),
                reconcile,
            ),
            (
                "patch the options of an unrelated remote profile",
                AffectsRule::Never,
                with(&remote, &|p| p.current = Some(id("sel"))),
                with(&remote, &|p| {
                    p.current = Some(id("sel"));
                    let item = p.items.get_mut(&id("remote")).unwrap();
                    let Some(ProfileSource::Remote { option, .. }) = item.definition.source_mut()
                    else {
                        unreachable!("the fixture is remote")
                    };
                    option.update_interval_minutes += 1;
                }),
                None,
            ),
            (
                "rename the current profile",
                AffectsRule::Never,
                base.clone(),
                with(&base, &|p| {
                    p.items.get_mut(&id("sel")).unwrap().metadata.name = "renamed".into()
                }),
                None,
            ),
            (
                "add a profile",
                AffectsRule::Never,
                base.clone(),
                with(&base, &|p| {
                    p.append_item(file_config("added", "added.yaml", &[]));
                }),
                None,
            ),
        ];
        for (case, rule, before, after, expected) in cases {
            let (hints, class) = ProfilesActor::mutation_hints(&rule, &after);
            assert_eq!(
                runtime_impact(&before, &after, &hints, class),
                expected,
                "{case}"
            );
        }
    }

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
