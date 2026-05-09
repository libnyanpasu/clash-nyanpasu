use std::sync::Arc;

use bon::Builder;

use super::{
    super::{StateSnapshot, error::*},
    *,
};

#[derive(Builder)]
#[builder(finish_fn = assemble)]
pub struct SimpleStateManagerSetup<State: Clone + Send + Sync + 'static> {
    initial_state: State,
    #[builder(default)]
    state_coordinator: StateCoordinatorBuilder<State>,
}

impl<State: Clone + Send + Sync + 'static> SimpleStateManagerSetup<State> {
    pub async fn initialize(self) -> Result<SimpleStateManager<State>, StateChangedError> {
        let coordinator = self
            .state_coordinator
            .build_initialized(self.initial_state)
            .await?;
        Ok(SimpleStateManager {
            state_coordinator: coordinator,
        })
    }
}

pub struct SimpleStateManager<State>
where
    State: Clone + Send + Sync + 'static,
{
    state_coordinator: StateCoordinator<State>,
}

impl<State> SimpleStateManager<State>
where
    State: Clone + Send + Sync + 'static,
{
    #[cfg(test)]
    pub(crate) fn from_coordinator(state_coordinator: StateCoordinator<State>) -> Self {
        Self { state_coordinator }
    }

    pub fn snapshot(&self) -> Arc<State> {
        self.state_coordinator.snapshot()
    }

    pub fn snapshot_handle(&self) -> StateSnapshot<State> {
        self.state_coordinator.snapshot_handle()
    }

    pub fn add_subscriber(&mut self, subscriber: Box<dyn StateAckSubscriber<State> + Send + Sync>) {
        self.state_coordinator.add_subscriber(subscriber);
    }

    pub fn remove_subscriber(
        &mut self,
        name: &str,
    ) -> Option<Box<dyn StateAckSubscriber<State> + Send + Sync>> {
        self.state_coordinator.remove_subscriber(name)
    }

    pub async fn upsert(&mut self, state: State) -> Result<(), StateChangedError> {
        self.state_coordinator.upsert_state(state).await?;
        Ok(())
    }
}
