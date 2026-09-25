use std::sync::Arc;

use nyanpasu_config::{clash::config::ClashConfig, profile::Profiles};
use nyanpasu_core::state::StateSnapshot;
use nyanpasu_core_manager::{CoreError, OperationId};

use super::{
    Command, Output,
    attempt::{LifecycleCommand, LiveAttempt},
    mutation::{DeferredTarget, MutationBudgets, MutationCommand},
    preparation::RuntimePreparation,
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
    /// The separate budgets of one mutation (v2 §5.5), injected rather than
    /// read from a constant so a test can reach an elapse path without waiting
    /// out the production one.
    pub budgets: MutationBudgets,
    /// A committed desired value the core is not running.
    pub deferred: Option<DeferredTarget>,
    pub pending_product: Option<(Arc<runtime::RuntimeSnapshot>, String)>,
    /// The daemon a confirmed move off service mode still has to release,
    /// and why the last attempt failed. An explicit retry releases it.
    pub pending_release: Option<String>,
    /// The attempt running now, or the latest one that did not settle
    /// (T10 §1.11). With the facade's pending action it is all that an
    /// explicit recovery reads, and it survives a panic unchanged.
    pub live: Option<LiveAttempt>,
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

    pub(super) fn notify_committed(&self, refresh: bool) {
        self.notify_requested(refresh, Vec::new());
    }
    pub(super) fn notify_requested(
        &self,
        refresh: bool,
        requested: Vec<crate::client::effects::plan::EffectKind>,
    ) {
        self.notifications.committed(
            crate::client::effects::plan::ApplicationEffectInputs::project(
                &self.lifecycle.application.load().state,
                &self.clash.load().state,
                self.lifecycle.ports.confirmed(),
            ),
            refresh,
            requested,
        );
    }

    pub async fn execute(
        &mut self,
        operation_id: OperationId,
        command: Command,
    ) -> Result<Output, CoreError> {
        // Observe the old host before any operation can replace its projection.
        self.lifecycle.capture_core_intent();
        match command {
            Command::Core(command) => self.execute_lifecycle(operation_id, command).await,
            Command::RetryRuntime { explicit } => self
                .retry_runtime(operation_id, explicit)
                .await
                .map(|_| Output::Unit),
            Command::Mutation(command) => {
                let MutationCommand { request } = *command;
                Ok(Output::Settled(Box::new(self.run_mutation(request).await)))
            }
        }
    }

    /// Runs one lifecycle command as an attempt of its own. Shutdown is not
    /// one: nothing follows it that a recovery could continue.
    async fn execute_lifecycle(
        &mut self,
        operation_id: OperationId,
        command: CoreCommand,
    ) -> Result<Output, CoreError> {
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
        let result = self.lifecycle.execute(command, &mut self.preparation).await;
        if matches!(&result, Ok(Output::Reconcile(_))) {
            self.deferred = None;
        }
        if matches!(&result, Ok(Output::Stop(_)))
            && let Some(deferred) = &mut self.deferred
        {
            deferred.health = crate::client::convergence::ConvergenceHealth::WaitingDependency;
            deferred.next_attempt = None;
        }
        self.notify_committed(true);
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
