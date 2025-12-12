use super::{super::error::*, *};

#[repr(transparent)]
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

    pub fn current_state(&self) -> Option<State> {
        self.state_coordinator.current_state()
    }

    pub async fn upsert(&mut self, state: State) -> Result<(), StateChangedError> {
        self.state_coordinator.upsert_state(state).await
    }

    pub async fn upsert_state_with_context(&mut self, state: State) -> Result<(), UpsertError> {
        self.state_coordinator
            .upsert_state_with_context::<State>(state.clone())
            .await
            .map_err(UpsertError::State)?;

        Ok(())
    }
}
