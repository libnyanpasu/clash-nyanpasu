//! Control-protocol helpers owned by the application lifecycle workflow.

use std::time::Duration;

use futures::future::{BoxFuture, FutureExt, Shared};
use nyanpasu_core_manager::{
    ConfigInput, CoreCommand, CoreCommandEnvelope, CoreError, CoreErrorKind, CoreSpec, Epoch,
    InstanceOptions, OperationId, ReconcileRequest, RevisionId,
};
use nyanpasu_ipc::api::core::v2::{
    OperationInfo, OperationOutputInfo, OperationPhase, ReconcileOutcomeKind,
};
use tokio::sync::OnceCell;

use super::{
    CoreClient, CoreStatusProjection, HandoffReport, ShutdownReport, SubmitFailure,
    endpoint::{CoreSubmission, ExecutionHost},
    intent::RuntimeIntent,
    service_actor::{ServiceClient, ServiceHostStatus},
};

const OPERATION_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq)]
pub struct AppliedConfigBinding {
    pub revision: nyanpasu_ipc::api::status::ConfigRevisionInfo,
    pub host: ExecutionHost,
    pub generation: u64,
}

/// Failure before the router's host handoff cannot change the running core.
#[derive(Debug)]
pub(crate) struct HostChangeFailure {
    pub error: CoreError,
    pub handoff_started: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReconcileReport {
    pub applied: AppliedConfigBinding,
    pub effective_config: Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>,
    pub output: OperationOutputInfo,
    pub status: CoreStatusProjection,
}

/// The core cleanly restored *its own* previous revision: the submitted
/// document never took effect. The name is not a recovery proof — after a
/// failed B→A restore the core is usually still on B (C4) — so the binding
/// here is reported as what the core says it fell back to, nothing more.
#[derive(Debug, Clone, PartialEq)]
pub struct RolledBackReport {
    pub restored: AppliedConfigBinding,
    pub failed_apply: Option<String>,
    pub output: OperationOutputInfo,
    pub status: CoreStatusProjection,
}

/// The submission's outcome could not be observed: a lost reply, a wait that
/// elapsed, or an operation still queued/running. The core may or may not have
/// applied it, so a caller must isolate the execution domain rather than
/// classify this as a failure (§2.2).
#[derive(Debug, Clone, PartialEq)]
pub struct UncertainReconcile {
    pub error: CoreError,
    pub status: CoreStatusProjection,
}

/// Evidence from the apply boundary. Read-only failures and router refusals
/// are distinct from submissions whose effects cannot yet be established.
#[derive(Debug, Clone, PartialEq)]
pub enum ReconcileResult {
    NotSubmitted(CoreError),
    /// A terminal refusal with a fresh observation proving the precondition still holds.
    Unchanged(CoreError),
    Reconciled(ReconcileReport),
    RolledBack(RolledBackReport),
    Unknown(UncertainReconcile),
}

impl ReconcileResult {
    /// The report of an apply that actually took effect, or the error a caller
    /// that can only handle success must report.
    ///
    /// `RolledBack` becomes a non-retryable `ApplyFailed`: the old config keeps
    /// running, so letting activation or a config patch claim success here is
    /// exactly the audited bug. `Unknown` keeps its own error, which the facade
    /// has already recorded as an uncertain outcome.
    pub fn into_applied(self) -> Result<ReconcileReport, CoreError> {
        match self {
            Self::Reconciled(report) => Ok(report),
            Self::NotSubmitted(error) | Self::Unchanged(error) => Err(error),
            Self::RolledBack(report) => Err(CoreError::new(
                CoreErrorKind::ApplyFailed,
                format!(
                    "core reconcile was rolled back to the previous revision: {}",
                    report.failed_apply.as_deref().unwrap_or("unknown reason")
                ),
                false,
            )),
            Self::Unknown(uncertain) => Err(uncertain.error),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StopReport {
    pub output: OperationOutputInfo,
    pub status: CoreStatusProjection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecoverReport {
    pub output: OperationOutputInfo,
    pub status: CoreStatusProjection,
}

enum CommandFailure {
    NotSubmitted(CoreError),
    Terminal(CoreError),
    Unknown(CoreError),
}
impl CommandFailure {
    fn into_error(self) -> CoreError {
        match self {
            Self::NotSubmitted(e) | Self::Terminal(e) | Self::Unknown(e) => e,
        }
    }
}

type SharedShutdown = Shared<BoxFuture<'static, ShutdownReport>>;

pub struct CoreFacade {
    core: CoreClient,
    service: ServiceClient,
    shutdown: OnceCell<SharedShutdown>,
    // Lost mutation replies must not release the application's execution domain.
    // Terminal operation failures and failed read-only preflights do not set this.
    outcome_uncertain: bool,
}

impl CoreFacade {
    pub fn new(core: CoreClient, service: ServiceClient) -> Self {
        Self {
            core,
            service,
            shutdown: OnceCell::new(),
            outcome_uncertain: false,
        }
    }

    pub(crate) fn outcome_uncertain(&self) -> bool {
        self.outcome_uncertain
    }

    fn observe_mutation<T>(&mut self, result: Result<T, CoreError>) -> Result<T, CoreError> {
        if let Err(error) = &result
            && matches!(
                error.kind,
                Some(CoreErrorKind::BackendUnavailable | CoreErrorKind::Internal)
            )
        {
            self.outcome_uncertain = true;
        }
        result
    }

    pub(crate) async fn api_client_if_running(
        &self,
    ) -> Result<Option<super::api::ApiClient>, super::api::ApiError> {
        let status = self
            .core
            .refresh_status()
            .await
            .map_err(|error| super::api::ApiError::Unavailable(error.to_string()))?;
        // Only an authoritative Stopped state proves there were no source connections.
        // Unknown state must still try the binding instead of silently skipping policy.
        if matches!(
            status.snapshot.and_then(|s| s.state),
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. })
        ) {
            return Ok(None);
        }
        self.core.api_client().await.map(Some)
    }

    /// Submits `intent` to the core. The bytes are the caller's, built once
    /// and shared with the advisory check, so the two provably agree.
    pub async fn reconcile(
        &mut self,
        intent: &RuntimeIntent,
        core_spec: CoreSpec,
        expected: &CoreStatusProjection,
    ) -> Result<ReconcileResult, CoreError> {
        let observed = match self.core.refresh_status().await {
            Ok(observed) => observed,
            Err(error) => return Ok(ReconcileResult::NotSubmitted(error)),
        };
        if !same_runtime_version(expected, &observed) {
            return Ok(ReconcileResult::NotSubmitted(CoreError::new(
                CoreErrorKind::RevisionConflict,
                "runtime changed after the apply baseline was captured",
                false,
            )));
        }
        let expected_applied = expected
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.revision.clone());
        // `expected_applied` is the optimistic-concurrency guard: apply only if
        // the manager is still on this revision. The wire carries the epoch as a
        // plain `u64`, and `Epoch` rejects zero. Dropping the guard on a zero
        // would turn a guarded apply into an unguarded one, so a value we cannot
        // express is an error rather than `None`.
        let expected_applied = expected_applied
            .map(|revision| {
                let epoch = Epoch::new(revision.epoch).ok_or_else(|| {
                    CoreError::new(
                        CoreErrorKind::Internal,
                        "the applied revision reported epoch 0, which is not a valid epoch"
                            .to_owned(),
                        false,
                    )
                })?;
                Ok::<_, CoreError>(RevisionId {
                    epoch,
                    generation: revision.generation,
                    effective_hash: revision.effective_hash,
                })
            })
            .transpose();
        let expected_applied = match expected_applied {
            Ok(revision) => revision,
            Err(error) => return Ok(ReconcileResult::NotSubmitted(error)),
        };
        let operation_id = OperationId::generate();
        let submission = CoreSubmission {
            expected_owner: Some((expected.host, expected.generation)),
            envelope: CoreCommandEnvelope {
                operation_id,
                command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                    core: core_spec,
                    config: ConfigInput::Inline {
                        bytes: intent.config_text.clone().into_bytes(),
                        expected_digest: Some(intent.digest.clone()),
                    },
                    options: InstanceOptions {
                        local_ipc: Some(intent.local_ipc),
                        ..InstanceOptions::default()
                    },
                    expected_applied,
                })),
            },
            core_type: Some(intent.core_type.clone()),
        };
        let applied_owner = expected.clone();
        // Only a *new* uncertainty belongs to this submission; the flag is
        // sticky, so a caller that was already uncertain must not have a
        // terminal failure re-labelled as an unobserved one.
        let output = match self.submit_and_wait(submission).await {
            Ok(output) => output,
            Err(CommandFailure::NotSubmitted(error)) => {
                return Ok(ReconcileResult::NotSubmitted(error));
            }
            Err(CommandFailure::Unknown(error)) => {
                return Ok(ReconcileResult::Unknown(UncertainReconcile {
                    error: error.with_operation(operation_id),
                    status: self.core.status(),
                }));
            }
            Err(CommandFailure::Terminal(error)) => {
                let status = self.core.refresh_status().await;
                if status
                    .as_ref()
                    .is_ok_and(|status| same_runtime_version(expected, status))
                {
                    return Ok(ReconcileResult::Unchanged(error));
                }
                self.outcome_uncertain = true;
                return Ok(ReconcileResult::Unknown(UncertainReconcile {
                    error: error.with_operation(operation_id),
                    status: self.core.status(),
                }));
            }
        };
        let outcome = match &output {
            OperationOutputInfo::Reconciled(outcome) => outcome,
            _ => {
                self.outcome_uncertain = true;
                return Ok(ReconcileResult::Unknown(UncertainReconcile {
                    error: unexpected_output("reconcile", &output).with_operation(operation_id),
                    status: self.core.status(),
                }));
            }
        };
        if let Some(warning) = &outcome.warning {
            tracing::warn!("core reconcile completed with a durability warning: {warning}");
        }
        let owner = applied_owner;
        // `RolledBack` is an `Ok` transaction from the runtime's point of view
        // (the manager cleanly restored the previous revision), but the
        // caller's desired config never took effect, so it stays a distinct
        // answer rather than folding into either success or a plain error.
        if outcome.outcome == ReconcileOutcomeKind::RolledBack {
            return Ok(ReconcileResult::RolledBack(RolledBackReport {
                restored: AppliedConfigBinding {
                    revision: outcome.revision.clone(),
                    host: owner.host,
                    generation: owner.generation,
                },
                failed_apply: outcome.failed_apply.clone(),
                output,
                status: self.core.status(),
            }));
        }
        let effective_config = self
            .core
            .effective_config()
            .await
            .ok()
            .flatten()
            .filter(|snapshot| snapshot.revision == outcome.revision);
        let status = self.core.status();
        let effective_config = effective_config
            .filter(|_| (owner.host, owner.generation) == (status.host, status.generation));
        if effective_config.is_none() {
            tracing::warn!("core applied configuration, but its effective snapshot is unavailable");
        }
        Ok(ReconcileResult::Reconciled(ReconcileReport {
            applied: AppliedConfigBinding {
                revision: outcome.revision.clone(),
                host: owner.host,
                generation: owner.generation,
            },
            effective_config,
            output,
            status,
        }))
    }

    pub async fn stop(&mut self) -> Result<StopReport, CoreError> {
        let output = self.command(CoreCommand::Stop).await?;
        if output != OperationOutputInfo::Stopped {
            return Err(unexpected_output("stop", &output));
        }
        Ok(StopReport {
            output,
            status: self.core.status(),
        })
    }

    pub async fn recover(&mut self) -> Result<RecoverReport, CoreError> {
        let output = self.command(CoreCommand::Recover).await?;
        if output != OperationOutputInfo::Recovered {
            return Err(unexpected_output("recover", &output));
        }
        Ok(RecoverReport {
            output,
            status: self.core.status(),
        })
    }

    pub(crate) async fn change_execution_host(
        &mut self,
        host: ExecutionHost,
    ) -> Result<HandoffReport, HostChangeFailure> {
        let target = match host {
            ExecutionHost::Local => self.core.initial_endpoint(),
            ExecutionHost::Service => {
                self.service
                    .ensure_ready()
                    .await
                    .map_err(|error| HostChangeFailure {
                        error,
                        handoff_started: false,
                    })?
            }
        };
        let result = self.core.change_host(target).await;
        self.observe_mutation(result)
            .map_err(|error| HostChangeFailure {
                error,
                handoff_started: true,
            })
    }

    /// Move to the Service host only if the daemon is already `Ready`, never
    /// by converging one. Boot uses this to restore a persisted host without
    /// installing or starting a service on the user's behalf.
    pub async fn adopt_service_host(&mut self) -> Result<HandoffReport, CoreError> {
        let target = self.service.adopt_if_ready().await?;
        let result = self.core.change_host(target).await;
        self.observe_mutation(result)
    }

    /// Re-adopt the same Service owner after a transport failure. Ordinary
    /// unavailability is retryable; a lost actor reply remains fail-closed.
    pub(crate) async fn recover_service_endpoint(
        &mut self,
        closing: &tokio_util::sync::CancellationToken,
    ) -> Result<HandoffReport, CoreError> {
        let result = async {
            let endpoint = self.service.recover_endpoint().await?;
            if closing.is_cancelled() {
                return Err(CoreError::new(
                    CoreErrorKind::ShuttingDown,
                    "service recovery cancelled by shutdown",
                    false,
                ));
            }
            self.core.change_host(endpoint).await
        }
        .await;
        if let Err(error) = &result
            && error.kind == Some(CoreErrorKind::Internal)
        {
            self.outcome_uncertain = true;
        }
        result
    }

    pub fn core_status(&self) -> CoreStatusProjection {
        self.core.status()
    }

    /// Authoritative status read for callers that must decide from the
    /// host's *applied* identity rather than the router's cached projection
    /// (R5): the same in-mailbox endpoint read `reconcile`'s CAS token uses,
    /// exposed for `replace_core_binary`'s stop decision.
    pub async fn refresh_status(&self) -> Result<CoreStatusProjection, CoreError> {
        self.core.refresh_status().await
    }

    pub fn service_status(&self) -> ServiceHostStatus {
        self.service.status()
    }

    pub async fn probe_service(&self) -> Result<ServiceHostStatus, CoreError> {
        self.service.probe().await
    }

    pub async fn install_service(&mut self) -> Result<(), CoreError> {
        let result = self.service.install().await;
        self.observe_mutation(result)
    }

    pub async fn start_service(&mut self) -> Result<(), CoreError> {
        let result = self.service.start_daemon().await;
        self.observe_mutation(result)
    }

    pub async fn stop_service(&mut self) -> Result<(), CoreError> {
        let result = self.service.stop_daemon().await;
        self.observe_mutation(result)
    }

    pub async fn uninstall_service(&mut self) -> Result<(), CoreError> {
        let result = self.service.uninstall().await;
        self.observe_mutation(result)
    }

    pub async fn shutdown(&self) -> ShutdownReport {
        self.shutdown
            .get_or_init(|| {
                let core = self.core.clone();
                std::future::ready(
                    async move {
                        match core.shutdown().await {
                            Ok(report) => report,
                            Err(error) => ShutdownReport {
                                stop: Err(error),
                                final_status: core.status().snapshot,
                            },
                        }
                    }
                    .boxed()
                    .shared(),
                )
            })
            .await
            .clone()
            .await
    }

    async fn command(&mut self, command: CoreCommand) -> Result<OperationOutputInfo, CoreError> {
        self.submit_and_wait(CoreSubmission {
            expected_owner: None,
            envelope: CoreCommandEnvelope {
                operation_id: OperationId::generate(),
                command,
            },
            core_type: None,
        })
        .await
        .map_err(CommandFailure::into_error)
    }

    async fn submit_and_wait(
        &mut self,
        submission: CoreSubmission,
    ) -> Result<OperationOutputInfo, CommandFailure> {
        let ticket = match self.core.submit(submission).await {
            Ok(ticket) => ticket,
            Err(SubmitFailure::NotSubmitted(error)) => {
                return Err(CommandFailure::NotSubmitted(error));
            }
            Err(SubmitFailure::Unknown(error)) => {
                self.outcome_uncertain = true;
                return Err(CommandFailure::Unknown(error));
            }
        };
        let info = ticket
            .endpoint
            .wait_operation(ticket.id, OPERATION_WAIT)
            .await;
        let Some(info) = info else {
            self.outcome_uncertain = true;
            return Err(CommandFailure::Unknown(
                CoreError::new(
                    CoreErrorKind::BackendUnavailable,
                    "the admitted core operation disappeared before reaching a terminal state",
                    true,
                )
                .with_operation(ticket.id),
            ));
        };
        if matches!(info.phase, OperationPhase::Queued | OperationPhase::Running)
            || (info.phase == OperationPhase::Succeeded && info.output.is_none())
        {
            self.outcome_uncertain = true;
            return Err(CommandFailure::Unknown(
                terminal_output(info, ticket.id).unwrap_err(),
            ));
        }
        terminal_output(info, ticket.id).map_err(CommandFailure::Terminal)
    }
}

fn same_runtime_version(expected: &CoreStatusProjection, observed: &CoreStatusProjection) -> bool {
    expected.host == observed.host
        && expected.generation == observed.generation
        && expected
            .snapshot
            .as_ref()
            .map(|s| (&s.revision, &s.source_hash, &s.state, &s.applied_kind))
            == observed
                .snapshot
                .as_ref()
                .map(|s| (&s.revision, &s.source_hash, &s.state, &s.applied_kind))
}

fn terminal_output(info: OperationInfo, id: OperationId) -> Result<OperationOutputInfo, CoreError> {
    match info.phase {
        OperationPhase::Succeeded => info.output.ok_or_else(|| {
            CoreError::new(
                CoreErrorKind::Internal,
                "a successful core operation had no output",
                false,
            )
            .with_operation(id)
        }),
        OperationPhase::Failed => {
            let error =
                info.error
                    .unwrap_or_else(|| nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                        kind: None,
                        message: "the core operation failed without an error payload".into(),
                        retryable: false,
                    });
            Err(CoreError {
                kind: error.kind.as_deref().and_then(CoreErrorKind::from_wire),
                message: error.message,
                retryable: error.retryable,
                operation_id: Some(id),
            })
        }
        OperationPhase::Queued | OperationPhase::Running => Err(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "the core operation did not reach a terminal state before the wait elapsed",
            true,
        )
        .with_operation(id)),
    }
}

fn unexpected_output(command: &str, output: &OperationOutputInfo) -> CoreError {
    CoreError::new(
        CoreErrorKind::Internal,
        format!("{command} returned an unexpected operation output: {output:?}"),
        false,
    )
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use camino::Utf8PathBuf;
    use nyanpasu_config::application::ClashCore;
    use nyanpasu_core_manager::{CoreKind, CoreSpec};
    use nyanpasu_ipc::{
        api::{
            core::v2::{
                OperationInfo, OperationOutputInfo, OperationPhase, ReconcileOutcomeInfo,
                ReconcileOutcomeKind,
            },
            status::{
                ConfigRevisionInfo, CoreInfos, CoreState, CoreStateDetail, RevisionIdInfo,
                RuntimeInfos, StatusResBody,
            },
        },
        types::{ServiceStatus, StatusInfo},
    };

    use super::*;
    use crate::core::actor_v2::{
        endpoint::{ControlEndpoint, CoreStatusSnapshot},
        service_actor::ServiceHostAdapter,
    };

    struct RecordingEndpoint {
        host: ExecutionHost,
        status: CoreStatusSnapshot,
        submissions: Mutex<Vec<CoreSubmission>>,
        stops: AtomicUsize,
        calls: Arc<Mutex<Vec<&'static str>>>,
        reconcile_outcome: Mutex<ReconcileOutcomeInfo>,
        /// Scripts a lost operation result: the submission was admitted but
        /// the registry answers nothing, which is the shape of an outcome the
        /// app cannot observe.
        result_lost: std::sync::atomic::AtomicBool,
    }

    impl RecordingEndpoint {
        fn new(host: ExecutionHost, revision: Option<RevisionIdInfo>) -> Arc<Self> {
            Arc::new(Self {
                host,
                status: CoreStatusSnapshot {
                    controller: None,
                    state: Some(CoreStateDetail::Running { pid: 7, epoch: 1 }),
                    state_changed_at: 1,
                    revision,
                    source_hash: None,
                    healthy: Some(true),
                    applied_kind: None,
                },
                submissions: Mutex::new(Vec::new()),
                stops: AtomicUsize::new(0),
                calls: Arc::new(Mutex::new(Vec::new())),
                reconcile_outcome: Mutex::new(ReconcileOutcomeInfo {
                    outcome: ReconcileOutcomeKind::Noop,
                    revision: ConfigRevisionInfo {
                        epoch: 1,
                        generation: 2,
                        source_hash: "source".into(),
                        effective_hash: "effective".into(),
                    },
                    warning: None,
                    failed_apply: None,
                }),
                result_lost: std::sync::atomic::AtomicBool::new(false),
            })
        }

        fn lose_the_result(&self) {
            self.result_lost.store(true, Ordering::SeqCst);
        }

        /// Overrides the outcome the next `Reconcile` submissions answer with,
        /// so a test can drive the facade through a non-default terminal
        /// outcome such as `RolledBack`.
        fn set_reconcile_outcome(&self, outcome: ReconcileOutcomeInfo) {
            *self.reconcile_outcome.lock().unwrap() = outcome;
        }

        fn operation(&self, submission: &CoreSubmission) -> OperationInfo {
            let output = match submission.envelope.command {
                CoreCommand::Reconcile(_) => {
                    OperationOutputInfo::Reconciled(self.reconcile_outcome.lock().unwrap().clone())
                }
                CoreCommand::Stop => OperationOutputInfo::Stopped,
                CoreCommand::Recover => OperationOutputInfo::Recovered,
                CoreCommand::Shutdown => OperationOutputInfo::ShutDown,
            };
            OperationInfo {
                id: submission.envelope.operation_id.to_string(),
                phase: OperationPhase::Succeeded,
                output: Some(output),
                error: None,
            }
        }
    }

    #[async_trait::async_trait]
    impl ControlEndpoint for RecordingEndpoint {
        fn host(&self) -> ExecutionHost {
            self.host
        }

        async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
            if matches!(submission.envelope.command, CoreCommand::Stop) {
                self.stops.fetch_add(1, Ordering::SeqCst);
            }
            let info = self.operation(&submission);
            self.submissions.lock().unwrap().push(submission);
            Ok(info)
        }

        async fn wait_operation(
            &self,
            id: OperationId,
            _timeout: Duration,
        ) -> Option<OperationInfo> {
            if self.result_lost.load(Ordering::SeqCst) {
                return None;
            }
            self.submissions
                .lock()
                .unwrap()
                .iter()
                .find(|submission| submission.envelope.operation_id == id)
                .map(|submission| self.operation(submission))
        }

        async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
            if self.host == ExecutionHost::Service {
                self.calls.lock().unwrap().push("change_host");
            }
            Ok(self.status.clone())
        }
    }

    struct ReadyService {
        endpoint: Arc<RecordingEndpoint>,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait::async_trait]
    impl ServiceHostAdapter for ReadyService {
        async fn probe(&self) -> Result<StatusInfo<'static>, String> {
            self.calls.lock().unwrap().push("ensure_ready");
            Ok(StatusInfo {
                name: Cow::Borrowed("nyanpasu-service"),
                version: Cow::Borrowed("test"),
                status: ServiceStatus::Running,
                server: Some(StatusResBody {
                    log_query_version: None,
                    version: Cow::Borrowed("2.0.0"),
                    core_infos: CoreInfos {
                        instance_id: None,
                        r#type: None,
                        state: CoreState::Running,
                        state_changed_at: 1,
                        config_path: None,
                        controller: None,
                        health: None,
                        revision: None,
                        detail: Some(CoreStateDetail::Stopped { reason: None }),
                    },
                    runtime_infos: RuntimeInfos {
                        service_data_dir: Cow::Owned(Default::default()),
                        service_config_dir: Cow::Owned(Default::default()),
                        nyanpasu_config_dir: Cow::Owned(Default::default()),
                        nyanpasu_data_dir: Cow::Owned(Default::default()),
                    },
                    logs: None,
                }),
            })
        }

        async fn install(&self) -> Result<(), String> {
            Ok(())
        }
        async fn uninstall(&self) -> Result<(), String> {
            Ok(())
        }
        async fn start_daemon(&self) -> Result<(), String> {
            Ok(())
        }
        async fn stop_daemon(&self) -> Result<(), String> {
            Ok(())
        }
        async fn update(&self) -> Result<(), String> {
            Ok(())
        }
        fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
            self.endpoint.clone()
        }
    }

    async fn facade(local: Arc<RecordingEndpoint>) -> (CoreFacade, Arc<Mutex<Vec<&'static str>>>) {
        let core = CoreClient::spawn(local).await.unwrap();
        let service_endpoint = RecordingEndpoint::new(ExecutionHost::Service, None);
        let calls = service_endpoint.calls.clone();
        let service = ServiceClient::spawn(
            Arc::new(ReadyService {
                calls: calls.clone(),
                endpoint: service_endpoint,
            }),
            1,
        )
        .await
        .unwrap();
        calls.lock().unwrap().clear();
        (CoreFacade::new(core, service), calls)
    }

    async fn wait_for_snapshot(core: &CoreClient) {
        let mut status = core.subscribe();
        while status.borrow().snapshot.is_none() {
            status.changed().await.unwrap();
        }
    }

    fn intent(document: &serde_yaml::Mapping) -> super::RuntimeIntent {
        super::super::intent::RuntimeIntentBuilder::build(
            (&ClashCore::Mihomo).into(),
            document,
            nyanpasu_core_manager::LocalIpcSettings {
                policy: nyanpasu_core_manager::LocalIpcPolicy::Disable,
                keep_http_controller: true,
            },
        )
        .expect("the fixture document serializes")
    }

    #[tokio::test]
    async fn reconcile_builds_an_inline_intent_with_the_status_revision_as_cas_token() {
        let expected = RevisionIdInfo {
            epoch: 4,
            generation: 9,
            effective_hash: "old-effective".into(),
        };
        let local = RecordingEndpoint::new(ExecutionHost::Local, Some(expected.clone()));
        let (mut facade, _) = facade(local.clone()).await;
        wait_for_snapshot(&facade.core).await;
        let document = serde_yaml::from_str("mode: rule\n").unwrap();
        let spec = CoreSpec {
            kind: CoreKind::Mihomo,
            binary_path: Utf8PathBuf::from("fake-mihomo"),
            version: None,
            features: vec![],
        };

        facade
            .reconcile(&intent(&document), spec, &facade.core_status())
            .await
            .unwrap();

        let submissions = local.submissions.lock().unwrap();
        let CoreCommand::Reconcile(request) = &submissions[0].envelope.command else {
            panic!("expected reconcile");
        };
        let ConfigInput::Inline {
            bytes,
            expected_digest,
        } = &request.config;
        let digest = nyanpasu_core_manager::payload_digest(bytes);
        assert_eq!(expected_digest.as_deref(), Some(digest.as_str()));
        assert_eq!(
            request.expected_applied,
            Some(RevisionId {
                epoch: Epoch::new(expected.epoch).expect("the fixture epoch is nonzero"),
                generation: expected.generation,
                effective_hash: expected.effective_hash,
            })
        );
        assert_eq!(submissions[0].core_type, Some((&ClashCore::Mihomo).into()));
    }

    #[tokio::test]
    async fn reconcile_keeps_a_rolled_back_outcome_distinct_from_success() {
        let local = RecordingEndpoint::new(ExecutionHost::Local, None);
        local.set_reconcile_outcome(ReconcileOutcomeInfo {
            outcome: ReconcileOutcomeKind::RolledBack,
            revision: ConfigRevisionInfo {
                epoch: 1,
                generation: 2,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
            },
            warning: None,
            failed_apply: Some("boom".into()),
        });
        let (mut facade, _) = facade(local).await;
        wait_for_snapshot(&facade.core).await;
        let document = serde_yaml::from_str("mode: rule\n").unwrap();
        let spec = CoreSpec {
            kind: CoreKind::Mihomo,
            binary_path: Utf8PathBuf::from("fake-mihomo"),
            version: None,
            features: vec![],
        };

        let result = facade
            .reconcile(&intent(&document), spec, &facade.core_status())
            .await
            .unwrap();

        let ReconcileResult::RolledBack(report) = &result else {
            panic!("expected a structured rolled-back result, got {result:?}");
        };
        assert_eq!(report.failed_apply.as_deref(), Some("boom"));
        assert_eq!(report.restored.revision.generation, 2);
        // A caller that can only handle success still gets the non-retryable
        // apply failure -- the old config is what keeps running.
        let error = result.into_applied().unwrap_err();
        assert_eq!(error.kind, Some(CoreErrorKind::ApplyFailed));
        assert!(!error.retryable);
        assert!(error.message.contains("boom"));
    }

    /// A lost operation result is not a failure: the core may well have
    /// applied the document. It stays its own answer so a caller can isolate
    /// the execution domain instead of classifying it (§2.2, §11.4).
    #[tokio::test]
    async fn reconcile_reports_a_lost_result_as_unknown_rather_than_failed() {
        let local = RecordingEndpoint::new(ExecutionHost::Local, None);
        local.lose_the_result();
        let (mut facade, _) = facade(local).await;
        wait_for_snapshot(&facade.core).await;
        let document = serde_yaml::from_str("mode: rule\n").unwrap();
        let spec = CoreSpec {
            kind: CoreKind::Mihomo,
            binary_path: Utf8PathBuf::from("fake-mihomo"),
            version: None,
            features: vec![],
        };

        let result = facade
            .reconcile(&intent(&document), spec, &facade.core_status())
            .await
            .unwrap();

        let ReconcileResult::Unknown(uncertain) = &result else {
            panic!("expected an unobserved outcome, got {result:?}");
        };
        assert_eq!(
            uncertain.error.kind,
            Some(CoreErrorKind::BackendUnavailable)
        );
        assert!(
            facade.outcome_uncertain(),
            "an unobserved mutation must keep holding the execution domain"
        );
        assert!(result.into_applied().is_err());
    }

    #[tokio::test]
    async fn reconcile_still_succeeds_when_the_outcome_only_carries_a_durability_warning() {
        let local = RecordingEndpoint::new(ExecutionHost::Local, None);
        local.set_reconcile_outcome(ReconcileOutcomeInfo {
            outcome: ReconcileOutcomeKind::Patched,
            revision: ConfigRevisionInfo {
                epoch: 1,
                generation: 2,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
            },
            warning: Some("durability uncertain".into()),
            failed_apply: None,
        });
        let (mut facade, _) = facade(local).await;
        wait_for_snapshot(&facade.core).await;
        let document = serde_yaml::from_str("mode: rule\n").unwrap();
        let spec = CoreSpec {
            kind: CoreKind::Mihomo,
            binary_path: Utf8PathBuf::from("fake-mihomo"),
            version: None,
            features: vec![],
        };

        facade
            .reconcile(&intent(&document), spec, &facade.core_status())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn reconcile_reports_a_zero_epoch_status_revision_as_an_internal_error() {
        let zero_epoch = RevisionIdInfo {
            epoch: 0,
            generation: 9,
            effective_hash: "effective".into(),
        };
        let local = RecordingEndpoint::new(ExecutionHost::Local, Some(zero_epoch));
        let (mut facade, _) = facade(local).await;
        wait_for_snapshot(&facade.core).await;
        let document = serde_yaml::from_str("mode: rule\n").unwrap();
        let spec = CoreSpec {
            kind: CoreKind::Mihomo,
            binary_path: Utf8PathBuf::from("fake-mihomo"),
            version: None,
            features: vec![],
        };

        let error = facade
            .reconcile(&intent(&document), spec, &facade.core_status())
            .await
            .unwrap()
            .into_applied()
            .unwrap_err();

        assert_eq!(error.kind, Some(CoreErrorKind::Internal));
        assert!(!error.retryable);
        assert!(
            !facade.outcome_uncertain(),
            "validation failed before any mutation was submitted"
        );
    }

    #[tokio::test]
    async fn a_second_shutdown_awaits_the_same_future() {
        let local = RecordingEndpoint::new(ExecutionHost::Local, None);
        let (facade, _) = facade(local.clone()).await;
        let facade = Arc::new(facade);

        let (first, second) = tokio::join!(facade.shutdown(), facade.shutdown());

        assert_eq!(first.stop, second.stop);
        assert_eq!(local.stops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn change_execution_host_to_service_ensures_ready_first() {
        let local = RecordingEndpoint::new(ExecutionHost::Local, None);
        let (mut facade, calls) = facade(local).await;

        facade
            .change_execution_host(ExecutionHost::Service)
            .await
            .unwrap();

        let calls = calls.lock().unwrap();
        let ensure = calls
            .iter()
            .position(|call| *call == "ensure_ready")
            .unwrap();
        let handoff = calls
            .iter()
            .position(|call| *call == "change_host")
            .unwrap();
        assert!(ensure < handoff);
    }
}
