use std::sync::Arc;

use nyanpasu_config::{clash::config::ClashConfig, profile::Profiles};
use nyanpasu_core::state::StateSnapshot;
use nyanpasu_core_manager::CoreError;

use super::{
    Command, Output,
    mutation::{DeferredTarget, MutationBudgets, MutationCommand, RecoveryContext},
    preparation::RuntimePreparation,
};
use crate::client::{core_lifecycle::CoreLifecycleWorkflow, runtime};

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
    /// Why the execution domain is isolated, when it is. Survives the
    /// operation that caused it so an explicit recovery has something to read.
    pub recovery: Option<Box<RecoveryContext>>,
}

impl ApplicationWorkflow {
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

    pub async fn execute(&mut self, command: Command) -> Result<Output, CoreError> {
        // Observe the old host before any operation can replace its projection.
        self.lifecycle.capture_core_intent();
        let lifecycle_command = matches!(&command, Command::Core(_));
        let result = match command {
            Command::Core(command) => self.lifecycle.execute(command, &mut self.preparation).await,
            Command::RetryRuntime { explicit } => {
                self.retry_runtime(explicit).await.map(|_| Output::Unit)
            }
            Command::Mutation(command) => {
                let MutationCommand { request } = *command;
                Ok(Output::Settled(Box::new(self.run_mutation(request).await)))
            }
        };
        if lifecycle_command {
            if matches!(&result, Ok(Output::Reconcile(_))) {
                self.deferred = None;
            }
            if matches!(&result, Ok(Output::Stop(_))) {
                if let Some(deferred) = &mut self.deferred {
                    deferred.health =
                        crate::client::convergence::ConvergenceHealth::WaitingDependency;
                    deferred.next_attempt = None;
                }
            }
            self.notify_committed(true);
        }
        self.lifecycle.uncertain |= self.lifecycle.core.outcome_uncertain();
        result
    }
}
