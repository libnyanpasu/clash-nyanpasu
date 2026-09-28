use std::sync::Arc;

use nyanpasu_config::{clash::config::ClashConfig, profile::Profiles};
use nyanpasu_core::state::StateSnapshot;
use nyanpasu_core_manager::{CoreError, OperationId};

use super::{
    Command, Output,
    attempt::{LifecycleCommand, LiveAttempt},
    mutation::{DeferredTarget, ReestablishCause, TargetOrigin},
    preparation::RuntimePreparation,
    startup::StartupReport,
};
use crate::client::{
    core_lifecycle::{Command as CoreCommand, CoreLifecycleWorkflow},
    runtime,
};

/// Applies already committed source config to the running core. The two state
/// handles are read-only and are sampled only once the actor has admitted the
/// command, so a candidate never fixes a domain it did not wait for.
pub(super) struct ApplicationWorkflow {
    pub notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
    pub profiles: StateSnapshot<Profiles>,
    pub clash: StateSnapshot<ClashConfig>,
    pub preparation: RuntimePreparation,
    /// The core's own verdict on a candidate document, asked before the Try
    /// submits it. An absent answer is never a passing one.
    pub validator: Arc<dyn super::ports::RuntimeValidatorPort>,
    pub lifecycle: CoreLifecycleWorkflow,
    /// A committed desired value the core is not running.
    pub deferred: Option<DeferredTarget>,
    pub pending_product: Option<(Arc<runtime::RuntimeSnapshot>, String)>,
    /// The daemon a confirmed move off service mode still has to release,
    /// and why the last attempt failed. An explicit retry releases it.
    pub pending_release: Option<String>,
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
    ) -> Result<Output, CoreError> {
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
        }
    }

    /// Runs one lifecycle command as an attempt of its own. Shutdown is not
    /// one: nothing follows it that a recovery could continue.
    async fn execute_lifecycle(
        &mut self,
        operation_id: OperationId,
        command: CoreCommand,
    ) -> Result<Output, CoreError> {
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
        let tracked = LifecycleCommand::of(&command);
        if let Some(command) = tracked {
            // The actor admits nothing but an explicit recovery or shutdown
            // while the domain is isolated, so no unresolved attempt is here.
            debug_assert!(
                self.live.is_none(),
                "an unresolved attempt is never replaced"
            );
            self.live = Some(LiveAttempt::lifecycle(operation_id, command));
        }
        let result = match self.lifecycle.execute(command, &mut self.preparation).await {
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
            && matches!(deferred.origin, TargetOrigin::Mutation { .. })
        {
            deferred.health = crate::client::convergence::ConvergenceHealth::WaitingDependency;
            deferred.next_attempt = None;
        }
        self.notify_bound(true);
        if tracked.is_some() {
            // Returning is the command's own conclusion; an action it left
            // pending is not, and the error it returned says why.
            let reason = self
                .lifecycle
                .core
                .pending_action()
                .and(result.as_ref().err())
                .map(ToString::to_string);
            self.conclude_attempt(reason);
        }
        result
    }
}
