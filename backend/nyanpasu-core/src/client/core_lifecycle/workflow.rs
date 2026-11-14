use std::sync::Arc;

use nyanpasu_config::application::NyanpasuAppConfig;
use nyanpasu_contracts::core_install::{
    BinaryInstaller, CoreBinaryCompanionPaths, CoreBinaryInstallRequest,
};
use nyanpasu_core_manager::{CoreError, CoreErrorKind, CoreKind};
use nyanpasu_state_engine::StateSnapshot;
use nyanpasu_updater::PreparedCoreBinary;
use snafu::{ResultExt, ensure};

use super::{
    super::{SessionPortResolver, runtime},
    Output, ServiceOperation,
    ports::{PreparedRuntime, RuntimePreparationPort},
};
use crate::{
    actor_v2::{
        HandoffReport,
        endpoint::wire_core_type_to_kind,
        facade::{
            CoreFacade, HostChangeFailure, ReconcileReport, ReconcileResult, RolledBackReport,
            StopReport, UncertainReconcile,
        },
    },
    client::{
        runtime_error,
        runtime_recovery::{ObservedRuntime, matches_confirmed_apply},
    },
    state::mutation::runtime_apply::RuntimeApplyReceipt,
};
use nyanpasu_runtime_values::{
    core_status::{EndpointConnectivity, ServicePhase},
    execution_host::ExecutionHost,
    publish::PublishRuntimeError,
    runtime_error::{
        ApplyRuntimeSnafu, InstallCoreBinarySnafu, InstallServiceSnafu, RecoverRuntimeSnafu,
        RecoverServiceEndpointSnafu, RefreshStatusSnafu, ResolveCoreBinarySnafu,
        RestartServiceSnafu, RuntimeError, ServiceHostsCoreSnafu, StartServiceSnafu, StopCoreSnafu,
        StopServiceSnafu, UninstallServiceSnafu,
    },
};

pub(in crate::client) struct CoreLifecycleWorkflow {
    /// Committed application config, read-only: the core lifecycle applies the
    /// facade's commits and never writes the domain itself.
    pub application: StateSnapshot<NyanpasuAppConfig>,
    pub core: CoreFacade,
    pub installer: Arc<dyn BinaryInstaller>,
    pub runtime: runtime::RuntimeSnapshotStore,
    /// The session's port bindings. The workflow is the only writer: it
    /// confirms a candidate when the core accepts it and ends the confirmed
    /// binding when the core stops.
    pub ports: Arc<SessionPortResolver>,
    pub recovery: ServiceRecovery,
    pub closing: tokio_util::sync::CancellationToken,
    /// Which host the reconciler has proven to own the runtime. Failed
    /// ownership actions revoke that proof until observation establishes it again.
    pub ownership: Ownership,
    /// The config dir this instance installs the daemon with. A daemon that
    /// reports another one serves another instance of the app.
    pub instance_config_dir: std::path::PathBuf,
    /// The ready generation of a daemon that failed to run the core. That
    /// daemon is not used again; one that became `Ready` since is a new
    /// daemon and gets a try. Retrying the same one on every save or tick
    /// would bounce the core between the hosts.
    pub service_failed: Option<u64>,
}

/// Whether the application has proven which host owns the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ownership {
    /// Nothing rules out a second instance on the other host.
    Unproven,
    /// `host` owns the runtime, and the other host is proven not to hold one.
    Established { host: ExecutionHost },
}

/// The host the core should run on. `enable_service_mode` is a preference:
/// the core runs on the service host while the service is usable — the
/// daemon is `Ready` (installed, running and through the version gate) and has
/// not failed to run the core since — and locally once it is known not to
/// be, so an unusable service never leaves the user without a core (#5443).
/// While that cannot be told (`service` is `None`: a transition, or a probe
/// that failed) the core stays on `current`.
///
/// Every path that decides a host resolves it here: startup and explicit
/// starts from the phase their own probe just published, saves and retries
/// from the phase last published. None of them installs or starts the daemon
/// to make Service usable; only an explicit service action or switching
/// service mode on does.
pub(in crate::client) fn effective_host(
    application: &NyanpasuAppConfig,
    service: Option<bool>,
    current: ExecutionHost,
) -> ExecutionHost {
    match (application.enable_service_mode, service) {
        (true, Some(true)) => ExecutionHost::Service,
        (true, None) => current,
        _ => ExecutionHost::Local,
    }
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
    /// Ticks queue up behind a long attempt, so the first one after it would
    /// otherwise fire the next one immediately. This is what makes the
    /// interval a floor between attempts rather than between their starts.
    next_attempt: Option<tokio::time::Instant>,
}

/// The connection attempts allowed before explicit service selection re-arms recovery.
const RECOVERY_BUDGET: u8 = 3;

impl ServiceRecovery {
    /// Explicit service selection or start re-arms connection recovery.
    fn rearm(&mut self) {
        self.attempts = 0;
        self.suppressed = false;
        self.next_attempt = None;
    }

    /// A terminal connection failure pauses automatic reconnection.
    fn pause(&mut self) {
        self.suppressed = true;
    }

    /// Leaving the service host or explicitly stopping its daemon suppresses reconnection.
    fn suppress(&mut self) {
        self.suppressed = true;
    }
}

/// What one submitted candidate did to the running core.
///
/// The three answers are kept apart because they need opposite handling: an
/// apply that took effect is a fact to accept, a rolled-back request left the
/// previous document running, and an unobserved one proves nothing at all and
/// invalidates its binding until observation can confirm the instance again.
pub(in crate::client) enum RuntimeSubmission {
    NotSubmitted(CoreError),
    Unchanged(CoreError),
    Applied {
        /// The built snapshot, bound to what the core accepted. Its product
        /// file is published separately, after the source commit (v2 §5.6).
        product: Arc<runtime::RuntimeSnapshot>,
        report: ReconcileReport,
    },
    RolledBack(RolledBackReport),
    Unknown(UncertainReconcile),
}

impl CoreLifecycleWorkflow {
    pub async fn execute(&mut self, command: ServiceOperation) -> Result<Output, RuntimeError> {
        match command {
            ServiceOperation::Install => {
                self.core
                    .install_service()
                    .await
                    .map_err(runtime_error::core_failure)
                    .context(InstallServiceSnafu)?;
                Ok(Output::Unit)
            }
            ServiceOperation::Start => {
                self.core
                    .start_service()
                    .await
                    .map_err(runtime_error::core_failure)
                    .context(StartServiceSnafu)?;
                self.recovery.rearm();
                Ok(Output::Unit)
            }
            ServiceOperation::Stop => {
                self.recovery.suppress();
                self.core
                    .stop_service()
                    .await
                    .map_err(runtime_error::core_failure)
                    .context(StopServiceSnafu)?;
                Ok(Output::Unit)
            }
            ServiceOperation::Restart => {
                self.core
                    .stop_service()
                    .await
                    .map_err(runtime_error::core_failure)
                    .context(RestartServiceSnafu)?;
                self.core
                    .start_service()
                    .await
                    .map_err(runtime_error::core_failure)
                    .context(RestartServiceSnafu)?;
                self.recovery.rearm();
                Ok(Output::Unit)
            }
            ServiceOperation::Uninstall => {
                ensure!(
                    self.core.core_status().host != ExecutionHost::Service,
                    ServiceHostsCoreSnafu
                );
                // Past the ownership guard the intent is committed, so a
                // half-finished uninstall still suppresses recovery. A refusal
                // above changed nothing and must leave the policy alone.
                self.recovery.suppress();
                self.core
                    .uninstall_service()
                    .await
                    .map_err(runtime_error::core_failure)
                    .context(UninstallServiceSnafu)?;
                Ok(Output::Unit)
            }
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

    fn recovery_incomplete(&self) -> bool {
        matches!(
            self.core.core_status().connectivity,
            EndpointConnectivity::Degraded {
                desired: ExecutionHost::Service,
                ..
            }
        )
    }

    /// Whether the service host can run the core, given the daemon's `phase`
    /// and the ready generation read with it: `None` while that cannot be
    /// told.
    pub fn service_usable(&self, phase: ServicePhase, generation: u64) -> Option<bool> {
        phase
            .readiness()
            .map(|ready| ready && self.service_failed != Some(generation))
    }

    /// [`Self::service_usable`] for the status last published.
    pub fn service_usable_now(&self) -> Option<bool> {
        let status = self.core.service_status();
        self.service_usable(status.phase, status.ready_generation)
    }

    /// Hands the runtime to a daemon that is already `Ready`, and never
    /// converges one: no install, no daemon start, no elevation prompt
    /// (T10 §1.3). Proving who owns the runtime afterwards is the caller's.
    pub(in crate::client) async fn adopt_ready_service(
        &mut self,
    ) -> Result<HandoffReport, HostChangeFailure> {
        let report = self.core.adopt_service_host().await?;
        self.follow_host();
        Ok(report)
    }

    /// Stops a running core this session holds no receipt for, and ends the
    /// binding it may be holding. The stop intent is left alone: retiring an
    /// instance the application cannot vouch for is not the user asking for
    /// a stop (T10 §1.4).
    pub(in crate::client) async fn retire_unreceipted(&mut self) -> Result<StopReport, CoreError> {
        let stopped = self.core.stop().await;
        self.ports.invalidate();
        stopped
    }

    #[cfg(test)]
    pub(in crate::client) fn recovery_attempts(&self) -> u8 {
        self.recovery.attempts
    }

    pub(in crate::client) fn recovery_armed(&self) -> bool {
        !self.recovery.suppressed && self.recovery.attempts < RECOVERY_BUDGET
    }

    pub fn recovery_due(&self) -> bool {
        self.recovery_armed()
            && !self.closing.is_cancelled()
            && self
                .recovery
                .next_attempt
                .is_none_or(|at| tokio::time::Instant::now() >= at)
            && self.recovery_incomplete()
    }

    pub(in crate::client) async fn recover_service_endpoint(
        &mut self,
    ) -> Result<bool, RuntimeError> {
        self.recovery.attempts += 1;
        if matches!(
            self.core.core_status().connectivity,
            EndpointConnectivity::Degraded { .. }
        ) {
            self.core
                .recover_service_endpoint(&self.closing)
                .await
                .map_err(runtime_error::core_failure)
                .context(RecoverServiceEndpointSnafu)?;
        }
        Ok(!self.closing.is_cancelled())
    }

    pub(in crate::client) fn finish_service_recovery(&mut self, error: Option<&RuntimeError>) {
        self.recovery.next_attempt = Some(tokio::time::Instant::now() + super::RECOVERY_INTERVAL);
        if error.is_some_and(|error| !error.retryable()) {
            self.recovery.pause();
        }
        if self.recovery.attempts >= RECOVERY_BUDGET && self.recovery_incomplete() {
            tracing::warn!(
                "service endpoint recovery budget spent; explicitly select or start Service to re-arm"
            );
        }
    }

    /// Moves ownership of the runtime to `host` and nothing else.
    ///
    /// A completed handoff leaves the runtime stopped; its caller applies the
    /// committed target afterward. Only `converge` may install or start the
    /// daemon to get there; without it, Service is adopted only if it is
    /// already `Ready`.
    pub(in crate::client) async fn move_execution_host(
        &mut self,
        host: ExecutionHost,
        converge: bool,
    ) -> Result<HandoffReport, crate::actor_v2::facade::HostChangeFailure> {
        let report = if host == ExecutionHost::Service && !converge {
            self.core.adopt_service_host().await?
        } else {
            self.core.change_execution_host(host).await?
        };
        // A completed handoff proved the source stopped before the target
        // was adopted, which is the ownership proof itself.
        if report.completed() {
            self.ownership = Ownership::Established { host };
        }
        // The host moved; whatever the caller does next is a follow-up effect
        // whose failure must not put the policy back on the old host.
        self.follow_host();
        Ok(report)
    }

    /// Submits a candidate to the core and records what it confirmed, without
    /// publishing the derived product.
    ///
    /// The caller publishes the derived YAML after the core accepts the
    /// application. A rollback and an unobserved result remain distinct so the
    /// caller can report whether the committed configuration took effect.
    pub(in crate::client) async fn submit_runtime(
        &mut self,
        prepared: PreparedRuntime,
        preparation: &dyn RuntimePreparationPort,
        expected: &crate::actor_v2::CoreStatusProjection,
    ) -> Result<RuntimeSubmission, RuntimeError> {
        let PreparedRuntime {
            snapshot,
            intent,
            ports,
            target,
        } = prepared;
        let spec = preparation
            .core_spec(&snapshot.target_core)
            .context(ResolveCoreBinarySnafu)?;
        let result = self
            .core
            .reconcile(&intent, spec, expected)
            .await
            .map_err(runtime_error::core_failure)
            .context(ApplyRuntimeSnafu)?;
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
        let instance_id = self
            .core
            .refresh_status()
            .await
            .ok()
            .and_then(|status| status.snapshot.and_then(|snapshot| snapshot.instance_id));
        let receipt = Arc::new(RuntimeApplyReceipt {
            instance_id,
            revision: snapshot.revision,
            target_core: snapshot.target_core,
            host: report.applied.host,

            binding: report.applied.clone(),
            ports,
            target,
        });
        self.runtime
            .record_confirmed_apply(Some(snapshot.clone()), receipt);
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
        Ok(RuntimeSubmission::Applied {
            product: snapshot,
            report,
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
    ) -> Result<(), PublishRuntimeError> {
        preparation.publish(&product).await?;
        self.runtime.generated_confirmed(product);
        Ok(())
    }

    /// Recovers the applied core kind from a confirmed apply when the host does
    /// not publish it. A verified status means that apply's instance is still
    /// running, so its receipt names the binary that is in use.
    fn confirmed_applied_kind(
        &self,
        status: &crate::actor_v2::CoreStatusProjection,
    ) -> Option<CoreKind> {
        let confirmed = self.runtime.confirmed()?;
        let observed = ObservedRuntime::from_status(status);
        if !matches_confirmed_apply(&confirmed.receipt, &observed) {
            return None;
        }
        let core_type: nyanpasu_utils::core::CoreType = (&confirmed.receipt.target_core).into();
        wire_core_type_to_kind(&core_type)
    }

    /// Installs `artifact` and restarts the core when the replacement owes it
    /// one. The caller applies the committed target when a restart is owed.
    pub(in crate::client) async fn replace_binary(
        &mut self,
        artifact: &PreparedCoreBinary,
        desired: nyanpasu_config::application::ClashCore,
    ) -> Result<RestartIntent, RuntimeError> {
        let status = self
            .core
            .refresh_status()
            .await
            .map_err(runtime_error::core_failure)
            .context(RefreshStatusSnafu)?;
        let (state, applied_kind) = {
            let snapshot = status.snapshot.as_ref();
            (
                snapshot.and_then(|snapshot| snapshot.state.clone()),
                snapshot
                    .and_then(|snapshot| snapshot.applied_kind)
                    .or_else(|| self.confirmed_applied_kind(&status)),
            )
        };
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
                Err(error) => {
                    return Err(error)
                        .map_err(runtime_error::core_failure)
                        .context(StopCoreSnafu);
                }
            }
            self.ports.invalidate();
        }
        // Stopped/NotStarted alone cannot prove quarantined processes are dead.
        self.core
            .recover()
            .await
            .map_err(runtime_error::core_failure)
            .context(RecoverRuntimeSnafu)?;
        let request = CoreBinaryInstallRequest {
            target: artifact.target,
            source: artifact.source.clone(),
            destination: artifact.destination.clone(),
            staging: artifact.staging.clone(),
            companion: artifact
                .companion
                .as_ref()
                .map(|companion| CoreBinaryCompanionPaths {
                    staged: companion.staged(),
                    destination: companion.destination.clone(),
                }),
        };
        self.installer
            .install(&request)
            .await
            .context(InstallCoreBinarySnafu)?;
        Ok(if restart {
            RestartIntent::Owed
        } else {
            RestartIntent::NotOwed
        })
    }
}

pub(in crate::client) enum RestartIntent {
    Owed,
    NotOwed,
}
