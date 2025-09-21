use super::*;

#[repr(transparent)]
pub struct SimpleStateManager<State: Clone + Send + Sync + 'static> {
    state_coordinator: StateCoordinator<State>,
}

impl<State: Clone + Send + Sync + 'static> SimpleStateManager<State> {
    pub fn new(state_coordinator: StateCoordinator<State>) -> Self {
        Self { state_coordinator }
    }

    pub fn current_state(&self) -> Option<State> {
        self.state_coordinator.current_state()
    }

    pub async fn upsert(&mut self, state: State) -> Result<(), StateChangedError> {
        self.state_coordinator.upsert_state(state).await
    }
}
