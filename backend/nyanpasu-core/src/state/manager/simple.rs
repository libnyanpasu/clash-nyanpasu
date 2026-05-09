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
    #[cfg(test)]
    pub(crate) fn from_coordinator(state_coordinator: StateCoordinator<State>) -> Self {
        Self { state_coordinator }
    }

    super::impl_state_manager_delegates!(State);

    pub async fn upsert(&mut self, state: State) -> Result<(), StateChangedError> {
        self.state_coordinator.upsert_state(state).await?;
        Ok(())
    }
}
