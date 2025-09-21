use super::*;

pub struct SimpleStateManager<State: Clone + Send + Sync + 'static> {
    state_coordinator: StateCoordinator<State>,
}

impl<State: Clone + Send + Sync + 'static> SimpleStateManager<State> {
    pub fn new(state_coordinator: StateCoordinator<State>) -> Self {
        Self { state_coordinator }
    }

    pub async fn current_state(&self) -> Option<State> {
        self.state_coordinator.current_state().await
    }

    pub async fn upsert(&self, state: State) -> Result<(), StateChangedError> {
        self.state_coordinator.upsert_state(state).await
    }
}
