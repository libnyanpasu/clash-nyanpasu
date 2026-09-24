use std::sync::Arc;

use nyanpasu_config::application::NyanpasuAppConfig;
use nyanpasu_core::state::StateSnapshot;
use nyanpasu_core_manager::{CoreError, CoreErrorKind};

use super::{
    super::{SessionPortResolver, UiEventSink, runtime},
    Command, Output,
    ports::{BinaryInstaller, PreparedCoreBinary, PreparedRuntime, RuntimePreparationPort},
};
use crate::core::actor_v2::{
    EndpointConnectivity, HandoffReport,
    endpoint::{ExecutionHost, wire_core_type_to_kind},
    facade::{CoreFacade, ReconcileReport, ReconcileResult, RolledBackReport, UncertainReconcile},
    service_actor::ServicePhase,
};

pub(in crate::client) struct CoreLifecycleWorkflow {
    /// Committed application config, read-only: the core lifecycle applies the
    /// facade's commits and never writes the domain itself.
    pub application: StateSnapshot<NyanpasuAppConfig>,
    pub core: CoreFacade,
    pub installer: Arc<dyn BinaryInstaller>,
    pub ui: Arc<dyn UiEventSink>,
    pub runtime: runtime::RuntimeSnapshotStore,
    /// The session's port bindings. The workflow is the only writer: it
    /// confirms a candidate when the core accepts it and ends the confirmed
    /// binding when the core stops.
    pub ports: Arc<SessionPortResolver>,
    // A lost lower-level reply is not evidence its side effects have finished.
    pub uncertain: bool,
    pub recovery: ServiceRecovery,
    pub closing: tokio_util::sync::CancellationToken,
}

/// A connection retry budget, separate from the OS daemon restart budget the
/// `ServiceActor` keeps: this one bounds IPC reconnections, that one bounds
/// elevated daemon starts, and a caller can exhaust either without touching
/// the other. Success does not re-arm it: repeated disconnects cannot create
/// a restart loop. Explicit Service selection/start re-arms recovery.
#[derive(Default)]
pub(in crate::client) struct ServiceRecovery {
    attempts: u8,
    suppressed: bool,
    intent: CoreIntent,
    /// Queued ticks coalesce into one flag, so a tick that arrived during a
    /// long attempt would otherwise fire the next one immediately. This is
    /// what makes the interval a floor between attempts rather than between
    /// their starts.
    next_attempt: Option<tokio::time::Instant>,
}

/// What the workflow owes the core across a Service outage. The two committed
/// states are exclusive by construction: a core the user stopped is never also
/// a core waiting to be restored.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum CoreIntent {
    /// Nothing owed; the host's own published state is the only truth.
    #[default]
    Idle,
    /// The user stopped the core. Snapshots outlive the stop that invalidated
    /// them, so this is what keeps a stale `Running` frame from reading as a
    /// reason to start the core again.
    Stopped,
    /// An outage interrupted a running core that is not back yet. It outlives
    /// a single attempt because adopting a fresh endpoint drops the snapshot
    /// it was derived from: nothing else records that the core should run.
    Restore,
}

/// Connection recovery attempts per outage, spent by both the reconnection and
/// the core restoration it may owe.
const RECOVERY_BUDGET: u8 = 3;

impl ServiceRecovery {
    /// Explicit Service intent re-opens the budget. What is owed to the core is
    /// deliberately untouched: neither a stop the user asked for nor a
    /// restoration still owed is a connection concern.
    fn rearm(&mut self) {
        self.attempts = 0;
        self.suppressed = false;
        self.next_attempt = None;
    }

    /// A terminal failure: stop retrying, but keep what is owed so that an
    /// explicit Service start can still finish the restoration it interrupted.
    fn pause(&mut self) {
        self.suppressed = true;
    }

    /// The user left the Service host: stop retrying and drop a restoration
    /// that was only ever owed to that host. A stop the user asked for is not
    /// dropped -- it outlives every host change.
    fn suppress(&mut self) {
        self.suppressed = true;
        if self.intent == CoreIntent::Restore {
            self.intent = CoreIntent::Idle;
        }
    }
}

pub(in crate::client) fn domain_error(error: impl std::fmt::Display) -> CoreError {
    CoreError::new(CoreErrorKind::Internal, error.to_string(), false)
}

/// What one submitted candidate did to the running core.
///
/// The three answers are kept apart because they need opposite handling: an
/// apply that took effect is a fact to accept, a rolled-back request left the
/// previous document running, and an unobserved one proves nothing at all and
/// has to isolate the execution domain (v2 §2.2). Collapsing the last two into
/// one error is what a caller does only when it cannot act on the difference.
pub(in crate::client) enum RuntimeSubmission {
    NotSubmitted(CoreError),
    Unchanged(CoreError),
    Applied {
        /// The built snapshot, bound to what the core accepted. Its product
        /// file is published separately, after the source commit (v2 §5.6).
        product: Arc<runtime::RuntimeSnapshot>,
        report: ReconcileReport,
        /// The recovery baseline this apply established.
        receipt: Arc<runtime::RuntimeApplyReceipt>,
    },
    RolledBack(RolledBackReport),
    Unknown(UncertainReconcile),
}

impl CoreLifecycleWorkflow {
    pub async fn execute(
        &mut self,
        command: Command,
        preparation: &mut dyn RuntimePreparationPort,
    ) -> Result<Output, CoreError> {
        match command {
            Command::RecoverServiceEndpoint => {
                self.recover_service_endpoint(preparation).await?;
                Ok(Output::Unit)
            }
            Command::ApplyControlChannel => {
                let status = self.core.refresh_status().await?;
                if !matches!(
                    status.snapshot.and_then(|s| s.state),
                    Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. })
                ) {
                    self.reconcile(preparation).await?;
                }
                Ok(Output::Unit)
            }
            Command::Reconcile => Ok(Output::Reconcile(self.reconcile(preparation).await?)),
            Command::RuntimeDirty => {
                self.reconcile(preparation).await?;
                self.ui.refresh_clash();
                Ok(Output::Unit)
            }
            Command::ChangeHost(host) => {
                let report = self
                    .core
                    .change_execution_host(host)
                    .await
                    .map_err(|failure| failure.error)?;
                self.follow_host();
                self.note_interrupted_core(report.interrupted_running());
                Ok(Output::Handoff(report))
            }
            Command::SetExecutionHost(service_mode) => {
                let effect = self.set_host(service_mode, preparation).await;
                let degradations = effect.err().map_or_else(Vec::new, |error| {
                    vec![runtime::Degradation {
                        phase: runtime::DegradationPhase::SystemEffect,
                        code: "service_host_transition_failed".into(),
                        message: error.message,
                        retryable: error.retryable,
                    }]
                });
                Ok(Output::Mutation(runtime::MutationOutcome::from_parts(
                    (),
                    degradations,
                )))
            }
            Command::RestoreExecutionHost => {
                if self.application.load().state.enable_service_mode {
                    let report = self.core.adopt_service_host().await?;
                    self.recovery.rearm();
                    self.note_interrupted_core(report.interrupted_running());
                }
                Ok(Output::Unit)
            }
            Command::ReplaceCoreBinary(artifact) => {
                self.replace_binary(artifact, preparation).await?;
                Ok(Output::Unit)
            }
            Command::StopCore => {
                // A stop the user asked for outlives both its own failure and
                // the snapshot it invalidates: the projection can still say
                // Running when the endpoint degrades a moment later, and
                // recovery must not read that as a reason to start the core.
                self.recovery.intent = CoreIntent::Stopped;
                let report = self.core.stop().await?;
                // Nothing holds those ports any more. A later reader must get
                // "unavailable", never the endpoint the stopped core used.
                self.ports.invalidate();
                Ok(Output::Stop(report))
            }
            Command::RecoverCore => Ok(Output::Recover(self.core.recover().await?)),
            Command::ProbeService => {
                Ok(Output::Service(Box::new(self.core.probe_service().await?)))
            }
            Command::InstallService => {
                self.core.install_service().await?;
                Ok(Output::Unit)
            }
            Command::StartService => {
                self.core.start_service().await?;
                self.recovery.rearm();
                Ok(Output::Unit)
            }
            Command::StopService => {
                self.recovery.suppress();
                self.core.stop_service().await?;
                Ok(Output::Unit)
            }
            Command::RestartService => {
                self.core.stop_service().await?;
                self.core.start_service().await?;
                self.recovery.rearm();
                Ok(Output::Unit)
            }
            Command::UninstallService => {
                if self.core.core_status().host == ExecutionHost::Service {
                    return Err(CoreError::new(
                        CoreErrorKind::OperationConflict,
                        "handoff to the local host before uninstalling the service",
                        false,
                    ));
                }
                // Past the ownership guard the intent is committed, so a
                // half-finished uninstall still suppresses recovery. A refusal
                // above changed nothing and must leave the policy alone.
                self.recovery.suppress();
                self.core.uninstall_service().await?;
                Ok(Output::Unit)
            }
            Command::Shutdown => Ok(Output::Shutdown(self.core.shutdown().await)),
        }
    }

    /// Recovery policy follows the host a completed handoff actually landed
    /// on. It is called only once ownership has moved: a refused or failed
    /// handoff changes nothing, so an armed policy stays armed on a still-live
    /// Service endpoint and an explicit suppression is not undone by a
    /// rejection.
    fn follow_host(&mut self) {
        // Ownership moved, so the binding confirmed on the previous host is
        // no longer a fact about anything: the ports the old owner held are
        // not the new owner's until it applies a runtime and confirms them.
        self.ports.invalidate();
        if self.core.core_status().host == ExecutionHost::Service {
            self.recovery.rearm();
        } else {
            self.recovery.suppress();
        }
    }

    /// Records that a running core was interrupted rather than stopped.
    /// Suppressed recovery records nothing: the user has left the host, and
    /// there is nothing to owe.
    fn note_interrupted_core(&mut self, interrupted: bool) {
        if interrupted && !self.recovery.suppressed && self.recovery.intent == CoreIntent::Idle {
            self.recovery.intent = CoreIntent::Restore;
        }
    }

    /// The same observation taken from the projection, for the case where no
    /// handoff reports it: the endpoint degraded and nothing has replaced it
    /// yet. The pump publishes asynchronously, so this can miss a degradation
    /// that lands mid-command -- the handoff's own report is what closes that
    /// window.
    pub fn capture_core_intent(&mut self) {
        let status = self.core.core_status();
        self.note_interrupted_core(
            matches!(
                status.connectivity,
                EndpointConnectivity::Degraded {
                    desired: ExecutionHost::Service,
                    ..
                }
            ) && matches!(
                status.snapshot.and_then(|s| s.state),
                Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { .. })
            ),
        );
    }

    /// What recovery exists to clear: a Service endpoint that stopped
    /// answering, or a core the outage stopped that is not back yet.
    fn recovery_incomplete(&self) -> bool {
        let status = self.core.core_status();
        matches!(
            status.connectivity,
            EndpointConnectivity::Degraded {
                desired: ExecutionHost::Service,
                ..
            }
        ) || (self.recovery.intent == CoreIntent::Restore && status.host == ExecutionHost::Service)
    }

    /// Whether the user asked for the core to be stopped and no apply has
    /// discharged that yet.
    ///
    /// A stop the user asked for outlives its own failure: the host can still
    /// publish `Running` a moment later, and nothing may read that as licence
    /// to start a core again (v2 §2.3 `SavedInactive`, V11).
    pub fn stop_requested(&self) -> bool {
        self.recovery.intent == CoreIntent::Stopped
    }

    pub fn recovery_due(&self) -> bool {
        !self.recovery.suppressed
            && self.recovery.attempts < RECOVERY_BUDGET
            && !self.closing.is_cancelled()
            && self
                .recovery
                .next_attempt
                .is_none_or(|at| tokio::time::Instant::now() >= at)
            && self.recovery_incomplete()
    }

    async fn recover_service_endpoint(
        &mut self,
        preparation: &mut dyn RuntimePreparationPort,
    ) -> Result<(), CoreError> {
        if !self.recovery_due() {
            return Ok(());
        }
        self.recovery.attempts += 1;
        let result = self.recovery_attempt(preparation).await;
        self.recovery.next_attempt = Some(tokio::time::Instant::now() + super::RECOVERY_INTERVAL);
        if result.as_ref().is_err_and(|e: &CoreError| !e.retryable) {
            // Paused, not abandoned: a terminal failure ends the automatic
            // retries, and an explicit Service start still has a restoration
            // to finish.
            self.recovery.pause();
        }
        if self.recovery.attempts >= RECOVERY_BUDGET && self.recovery_incomplete() {
            tracing::warn!(
                "service endpoint recovery budget spent; explicitly select or start Service to re-arm"
            );
        }
        result
    }

    async fn recovery_attempt(
        &mut self,
        preparation: &mut dyn RuntimePreparationPort,
    ) -> Result<(), CoreError> {
        if matches!(
            self.core.core_status().connectivity,
            EndpointConnectivity::Degraded { .. }
        ) {
            let report = self.core.recover_service_endpoint(&self.closing).await?;
            self.note_interrupted_core(report.interrupted_running());
        }
        if self.closing.is_cancelled() || self.recovery.intent != CoreIntent::Restore {
            return Ok(());
        }
        // A transport-only outage must not reapply or stop an already live
        // runtime. Only restore a previously running core to a stopped host.
        match self
            .core
            .refresh_status()
            .await?
            .snapshot
            .and_then(|s| s.state)
        {
            // The runtime outlived the outage; there is nothing left to owe.
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { .. }) => {
                self.recovery.intent = CoreIntent::Idle;
            }
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. })
                if !self.closing.is_cancelled() =>
            {
                // Every phase boundary this workflow owns is checked, but a
                // cancel landing inside the build or the submission still
                // completes: abandoning a submitted config mid-flight would
                // turn a known outcome into an uncertain one. It stays bounded
                // because shutdown is serialized behind this operation and
                // stops whatever it started.
                self.reconcile(preparation).await?;
                self.ui.refresh_clash();
            }
            // Any other state proves nothing about whether the core should be
            // running, so the intent stays owed to the next attempt.
            _ => {}
        }
        Ok(())
    }

    async fn set_host(
        &mut self,
        service_mode: bool,
        preparation: &mut dyn RuntimePreparationPort,
    ) -> Result<(), CoreError> {
        let host = if service_mode {
            ExecutionHost::Service
        } else {
            ExecutionHost::Local
        };
        let report = self
            .move_execution_host(host)
            .await
            .map_err(|failure| failure.error)?;
        if matches!(report, HandoffReport::Completed { .. }) {
            self.reconcile(preparation).await?;
        }
        if !service_mode
            && !matches!(
                self.core.service_status().phase,
                ServicePhase::NotInstalled | ServicePhase::DaemonStopped
            )
        {
            self.core.stop_service().await?;
        }
        Ok(())
    }

    /// Moves ownership of the runtime to `host` and nothing else.
    ///
    /// A completed handoff leaves the runtime stopped awaiting a reconcile, so
    /// every caller owes one — with the candidate it is trying, or with the
    /// baseline it is putting back. That is why this is separate from
    /// [`CoreLifecycleWorkflow::set_host`], which follows it with the committed
    /// configuration: a Try has a candidate that is not committed yet, and a
    /// Cancel has a receipt rather than a configuration to rebuild (v2 §5.3).
    pub(in crate::client) async fn move_execution_host(
        &mut self,
        host: ExecutionHost,
    ) -> Result<HandoffReport, crate::core::actor_v2::facade::HostChangeFailure> {
        let report = self.core.change_execution_host(host).await?;
        // The host moved; whatever the caller does next is a follow-up effect
        // whose failure must not put the policy back on the old host.
        self.follow_host();
        self.note_interrupted_core(report.interrupted_running());
        Ok(report)
    }

    async fn reconcile(
        &mut self,
        preparation: &mut dyn RuntimePreparationPort,
    ) -> Result<ReconcileReport, CoreError> {
        let prepared = preparation.prepare_latest().await?;
        self.apply_runtime(prepared, preparation).await
    }

    pub async fn apply_runtime(
        &mut self,
        prepared: PreparedRuntime,
        preparation: &dyn RuntimePreparationPort,
    ) -> Result<ReconcileReport, CoreError> {
        preparation
            .publish(&prepared.snapshot)
            .await
            .map_err(domain_error)?;
        // Promoted means the product was published, not that the host applied it.
        self.runtime.generated(prepared.snapshot.clone());
        let expected = self.core.refresh_status().await?;
        match self
            .submit_runtime(prepared, preparation, &expected)
            .await?
        {
            RuntimeSubmission::Applied { report, .. } => Ok(report),
            RuntimeSubmission::NotSubmitted(error) | RuntimeSubmission::Unchanged(error) => {
                Err(error)
            }
            RuntimeSubmission::RolledBack(report) => {
                ReconcileResult::RolledBack(report).into_applied()
            }
            RuntimeSubmission::Unknown(uncertain) => {
                ReconcileResult::Unknown(uncertain).into_applied()
            }
        }
    }

    /// Submits a candidate to the core and records what it confirmed, without
    /// publishing the derived product.
    ///
    /// The public runtime YAML is a derived view of an *accepted* source
    /// configuration, and a Try runs before its source is committed. Publishing
    /// it here would announce a document the transaction may still abort, so the
    /// Try path publishes after Confirm instead (v2 §5.6). The three answers
    /// stay apart on the way out: only the caller knows whether a rolled-back
    /// request or an unobserved one may be collapsed into an error.
    pub(in crate::client) async fn submit_runtime(
        &mut self,
        prepared: PreparedRuntime,
        preparation: &dyn RuntimePreparationPort,
        expected: &crate::core::actor_v2::CoreStatusProjection,
    ) -> Result<RuntimeSubmission, CoreError> {
        let PreparedRuntime {
            snapshot,
            intent,
            ports,
        } = prepared;
        let spec = preparation
            .core_spec(&snapshot.target_core)
            .map_err(|error| {
                CoreError::new(CoreErrorKind::BinaryNotFound, error.to_string(), false)
            })?;
        let result = self.core.reconcile(&intent, spec.clone(), expected).await?;
        // An unobserved outcome may still have applied: the core can already
        // be listening on the candidate's ports. The previously confirmed
        // binding then describes an instance that may no longer exist, and
        // §6.2 forbids it decaying into "the port we used last time".
        //
        // Matching port numbers do not exempt it. A candidate that asks for
        // exactly the confirmed ports still replaces the instance holding them,
        // and an unobserved apply can have stopped the old one and failed to
        // start the new: the numbers are identical and nothing is listening.
        // Only a confirmed apply says a listener exists, and that is the path
        // below (D1).
        if matches!(result, ReconcileResult::Unknown(_)) {
            self.ports.invalidate();
        }
        let report = match result {
            ReconcileResult::Reconciled(report) => report,
            ReconcileResult::NotSubmitted(error) => {
                return Ok(RuntimeSubmission::NotSubmitted(error));
            }
            ReconcileResult::Unchanged(error) => return Ok(RuntimeSubmission::Unchanged(error)),
            ReconcileResult::RolledBack(report) => {
                return Ok(RuntimeSubmission::RolledBack(report));
            }
            ReconcileResult::Unknown(uncertain) => {
                return Ok(RuntimeSubmission::Unknown(uncertain));
            }
        };
        let mut bound = snapshot.as_ref().clone();
        bound.applied_binding = Some(report.applied.clone());
        let snapshot = Arc::new(bound);
        // The receipt is the fact the core confirmed. It is what a recovery
        // restores and the only thing that may promote a candidate port
        // binding, and it advances here rather than below: the effective-config
        // inspection is diagnostic and may never arrive (C3).
        let receipt = Arc::new(runtime::RuntimeApplyReceipt {
            revision: snapshot.revision,
            config_text: Arc::from(intent.config_text.as_str()),
            config_digest: intent.digest.clone(),
            target_core: snapshot.target_core,
            core_spec: spec,
            host: report.applied.host,
            run_intent: crate::client::application_workflow::policy::CoreRunIntent::Running,
            local_ipc: intent.local_ipc,
            binding: report.applied.clone(),
            ports,
        });
        self.runtime
            .record_confirmed_apply(Some(snapshot.clone()), receipt.clone());
        if let Some(effective) = report.effective_config.clone() {
            match snapshot.with_effective_config(
                effective,
                report.applied.host,
                report.applied.generation,
            ) {
                Ok(applied) => self
                    .runtime
                    .applied(&snapshot.inspection_id, Arc::new(applied)),
                Err(error) => tracing::warn!("effective config inspection unavailable: {error}"),
            }
        }
        // An applied config is the only thing that discharges what is owed --
        // it both restores an interrupted core and ends a user's stop. Clearing
        // on entry would lose the intent to a failure halfway through.
        self.recovery.intent = CoreIntent::Idle;
        Ok(RuntimeSubmission::Applied {
            product: snapshot,
            report,
            receipt,
        })
    }

    /// Publishes the derived product of a candidate the core already accepted.
    ///
    /// Only reached after the source configuration is committed. A failure here
    /// leaves the applied runtime and the committed source alone: the product
    /// file is a view, and losing it is a degradation to report and retry, not
    /// a reason to undo anything (v2 §5.6).
    pub(in crate::client) async fn publish_applied_product(
        &self,
        product: Arc<runtime::RuntimeSnapshot>,
        preparation: &dyn RuntimePreparationPort,
    ) -> anyhow::Result<()> {
        preparation.publish(&product).await?;
        self.runtime.generated_confirmed(product);
        Ok(())
    }

    async fn replace_binary(
        &mut self,
        artifact: PreparedCoreBinary,
        preparation: &mut dyn RuntimePreparationPort,
    ) -> Result<(), CoreError> {
        let desired: crate::config::nyanpasu::ClashCore = self.application.load().state.core.into();
        let status = self.core.refresh_status().await?;
        let (state, applied_kind) = status
            .snapshot
            .map_or((None, None), |s| (s.state, s.applied_kind));
        let not_proven_stopped = !matches!(
            state,
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. })
        );
        let target_type: nyanpasu_utils::core::CoreType = (&artifact.target).into();
        let needs_stop = not_proven_stopped
            && (applied_kind.is_none() || applied_kind == wire_core_type_to_kind(&target_type));
        let restart = desired == artifact.target || needs_stop;
        if needs_stop {
            match self.core.stop().await {
                Ok(_) => {}
                Err(error) if error.kind == Some(CoreErrorKind::NotStarted) => {}
                Err(error) => return Err(error),
            }
            self.ports.invalidate();
        }
        // Stopped/NotStarted alone cannot prove quarantined processes are dead.
        self.core.recover().await?;
        self.installer
            .install(&artifact)
            .await
            .map_err(domain_error)?;
        if restart {
            artifact.progress.restarting();
            self.reconcile(preparation).await?;
        }
        Ok(())
    }
}
