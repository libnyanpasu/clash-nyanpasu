use std::sync::Arc;

use nyanpasu_config::clash::config::ClashConfig;
use nyanpasu_core_manager::OperationId;
use nyanpasu_state_engine::StateSnapshot;

use super::{
    Command, Output,
    attempt::LiveAttempt,
    mutation::{DeferredTarget, ReestablishCause, TargetOrigin},
    preparation::RuntimePreparation,
    startup::StartupReport,
};
use crate::client::{
    core_lifecycle::{Command as CoreCommand, CoreLifecycleWorkflow},
    runtime,
};
use nyanpasu_runtime_values::runtime_error::RuntimeError;

/// Applies already committed source config to the running core. Source handles
/// are read-only; preparation captures committed Profiles and its content under
/// one guard before building.
pub(super) struct ApplicationWorkflow {
    pub notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
    pub clash: StateSnapshot<ClashConfig>,
    pub preparation: RuntimePreparation,
    pub lifecycle: CoreLifecycleWorkflow,
    /// A committed desired value the core is not running.
    pub deferred: Option<DeferredTarget>,
    pub pending_product: Option<(Arc<runtime::RuntimeSnapshot>, String)>,
    /// The daemon a confirmed move off service mode still has to release,
    /// and why the last attempt failed. An explicit retry releases it.
    pub pending_release: Option<String>,
    pub sampled: Option<super::EngineVersions>,
    pub checked: Option<crate::client::core_lifecycle::ports::PreparedRuntime>,
    pub applied_selection: Option<super::committed::Selection>,
    pub service_preferred: bool,
    pub release_owed: bool,
    /// The attempt running now, or the latest one that did not settle
    /// (T10 §1.11). With the facade's pending action it is all that an
    /// explicit recovery reads.
    pub live: Option<LiveAttempt>,
    /// StartupReconcile's first report. It runs once; asking again returns
    /// this and touches nothing (T10 §1.2).
    pub startup: Option<StartupReport>,
}

impl ApplicationWorkflow {
    /// The retryable work confirmed commits left behind, for the status
    /// surface.
    pub(super) fn maintenance(&self) -> Option<String> {
        let items: Vec<&str> = self
            .pending_product
            .iter()
            .map(|(_, error)| error.as_str())
            .chain(self.pending_release.as_deref())
            .collect();
        (!items.is_empty()).then(|| items.join("; "))
    }

    /// Hands the effects owner the ports the core is bound to now, the
    /// Runtime's own slice. `refresh` asks the tray for a partial refresh.
    pub(super) fn notify_bound(&self, refresh: bool) {
        self.notifications
            .runtime_bound(self.lifecycle.ports.confirmed(), refresh);
    }

    /// Hands every owner its complete desired value. StartupReconcile sends
    /// this exactly once, whatever it found (T10 §1.9).
    pub(super) fn publish_full(&self) {
        self.notifications
            .publish_full(self.lifecycle.ports.confirmed());
    }

    pub async fn execute(
        &mut self,
        operation_id: OperationId,
        command: Command,
    ) -> Result<Output, RuntimeError> {
        // Observe the old host before any operation can replace its projection.
        self.lifecycle.capture_core_intent();
        match command {
            Command::StartupReconcile => Ok(Output::Startup(Box::new(
                self.startup_reconcile(operation_id).await,
            ))),
            Command::Core(command) => self.execute_lifecycle(operation_id, command).await,
            Command::RetryRuntime { explicit } => self
                .retry_runtime(operation_id, explicit)
                .await
                .map(|_| Output::Unit),
            Command::InterruptConnections => {
                let failure = if self.clash.load().state.break_connection.on_mode_change {
                    let context = self
                        .lifecycle
                        .prepare_apply(
                            operation_id,
                            super::super::core_lifecycle::apply::RuntimeApplyOptions {
                                interrupt_connections: Some(
                                    crate::connections::ConnectionScope::All,
                                ),
                            },
                        )
                        .await;
                    CoreLifecycleWorkflow::finish_interruption(context, false).await
                } else {
                    None
                };
                Ok(Output::Interruption(failure))
            }
            Command::PrepareService => {
                use snafu::IntoError as _;
                self.lifecycle
                    .core
                    .prepare_service_host()
                    .await
                    .map_err(|error| {
                        nyanpasu_runtime_values::runtime_error::MoveHostSnafu {
                            host: nyanpasu_runtime_values::execution_host::ExecutionHost::Service,
                        }
                        .into_error(crate::client::runtime_error::core_failure(error))
                    })?;
                self.lifecycle.service_failed = None;
                Ok(Output::Unit)
            }
            Command::FollowService => {
                self.follow_service(operation_id).await;
                Ok(Output::Unit)
            }
        }
    }

    /// Runs one lifecycle command as an attempt of its own.
    async fn execute_lifecycle(
        &mut self,
        operation_id: OperationId,
        command: CoreCommand,
    ) -> Result<Output, RuntimeError> {
        // An explicit start without a proven owner of the desired host
        // re-establishes one instead of refusing (T10 §1.7 #5).
        if matches!(command, CoreCommand::Reconcile) && !self.lifecycle.start_permitted() {
            let result = self.explicit_start(operation_id).await;
            self.notify_bound(true);
            return result;
        }
        // A stop accepted after an explicit start takes back the start it
        // authorised; the ownership obligation stays (T10 §1.7).
        if matches!(command, CoreCommand::StopCore)
            && let Some(target) = &mut self.deferred
            && target.origin == TargetOrigin::Reestablish(ReestablishCause::ExplicitStart)
        {
            target.origin = TargetOrigin::Reestablish(ReestablishCause::Recovery);
        }
        // The actor admits nothing but an explicit recovery while the domain
        // is isolated, so no unresolved attempt is here.
        debug_assert!(
            self.live.is_none(),
            "an unresolved attempt is never replaced"
        );
        self.live = Some(LiveAttempt::lifecycle(operation_id));
        let result = match self.lifecycle.execute(command, &self.preparation).await {
            // The restart a binary replacement owed waits for a proven owner:
            // the open reestablish target is what brings it (T10 §1.7 #7).
            Ok(Output::RestartWithheld) => {
                if let Some(target) = &mut self.deferred
                    && matches!(target.origin, TargetOrigin::Reestablish(_))
                {
                    target.next_attempt = Some(tokio::time::Instant::now());
                }
                Ok(Output::Unit)
            }
            result => result,
        };
        if matches!(&result, Ok(Output::Reconcile(_))) {
            self.deferred = None;
        }
        // A stop does not clear a reestablish target: the next attempt proves
        // the owner and confirms the stop, and that is what ends it.
        if matches!(&result, Ok(Output::Stop(_)))
            && let Some(deferred) = &mut self.deferred
            && matches!(deferred.origin, TargetOrigin::Mutation)
        {
            deferred.health =
                nyanpasu_runtime_values::configuration::ConvergenceHealth::WaitingDependency;
            deferred.next_attempt = None;
        }
        self.notify_bound(true);
        // Returning is the command's own conclusion; an action it left
        // pending is not, and the error it returned says why.
        let reason = self
            .lifecycle
            .core
            .pending_action()
            .and(result.as_ref().err())
            .map(ToString::to_string);
        self.conclude_attempt(reason);
        result
    }
}
