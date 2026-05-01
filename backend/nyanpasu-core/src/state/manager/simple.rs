use std::sync::Arc;

use bon::Builder;

use super::{super::error::*, *};

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
    pub fn new(state_coordinator: StateCoordinator<State>) -> Self {
        Self { state_coordinator }
    }

    pub fn current_state(&self) -> Option<Arc<State>> {
        self.state_coordinator.current_state()
    }

    pub fn snapshot_handle(&self) -> StateSnapshot<State> {
        self.state_coordinator.snapshot_handle()
    }

    pub async fn upsert(&mut self, state: State) -> Result<(), StateChangedError> {
        self.state_coordinator.upsert_state(state).await
    }

    pub async fn upsert_state_with_context(&mut self, state: State) -> Result<(), UpsertError> {
        self.state_coordinator
            .upsert_state_with_context(state.clone())
            .await
            .map_err(UpsertError::State)?;

        Ok(())
    }
}
