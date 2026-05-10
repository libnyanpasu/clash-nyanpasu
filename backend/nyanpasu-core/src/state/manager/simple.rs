use bon::Builder;

use crate::state::PrepareReport;

use super::{super::error::*, *};

#[derive(Builder)]
#[builder(finish_fn = assemble)]
pub struct SimpleStateManagerSetup<State: Clone + Send + Sync + 'static> {
    initial_state: State,
    #[builder(default)]
    state_coordinator: StateCoordinatorBuilder<State>,
    #[builder(default)]
    force_build: bool,
}

impl<State: Clone + Send + Sync + 'static> SimpleStateManagerSetup<State> {
    pub async fn initialize(
        self,
    ) -> Result<SimpleStateManager<State>, ManagerInitError<SimpleStateManager<State>>> {
        let Self {
            initial_state,
            state_coordinator,
            force_build,
        } = self;

        match state_coordinator.build_initialized(initial_state).await {
            Ok(coordinator) => Ok(SimpleStateManager {
                state_coordinator: coordinator,
            }),
            Err(error) => {
                let (coordinator, report) = error.into_parts();
                let manager = SimpleStateManager {
                    state_coordinator: coordinator,
                };
                if force_build {
                    Ok(manager)
                } else {
                    Err(ManagerInitError::new(manager, report))
                }
            }
        }
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

    pub async fn upsert(&mut self, state: State) -> Result<PrepareReport, StateChangedError> {
        self.state_coordinator.upsert_state(state).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Ack, StateAckSubscriber, StateChange, SubscriberName};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Clone, Debug, PartialEq)]
    struct TestState {
        value: i32,
    }

    struct FailingInitSubscriber {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<TestState> for FailingInitSubscriber {
        fn name(&self) -> SubscriberName<'_> {
            "failing_init".into()
        }

        async fn on_prepare(&self, _change: StateChange<TestState>) -> Ack {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ack::Failed(anyhow::anyhow!("init ACK failed"))
        }
    }

    fn failing_builder(calls: Arc<AtomicUsize>) -> StateCoordinatorBuilder<TestState> {
        StateCoordinatorBuilder::default()
            .with_subscriber(Box::new(FailingInitSubscriber { calls }))
    }

    #[tokio::test]
    async fn test_initialize_ack_failure_returns_recoverable_manager() {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = TestState { value: 42 };

        let result = SimpleStateManagerSetup::builder()
            .initial_state(state.clone())
            .state_coordinator(failing_builder(Arc::clone(&calls)))
            .assemble()
            .initialize()
            .await;

        match result {
            Err(error) => {
                let (manager, report) = error.into_parts();
                assert!(report.has_required_failures());
                assert_eq!(&*manager.snapshot(), &state);
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            }
            Ok(_) => panic!("expected recoverable init ACK error"),
        }
    }

    #[tokio::test]
    async fn test_force_build_returns_manager_after_ack_failure() {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = TestState { value: 7 };

        let manager = SimpleStateManagerSetup::builder()
            .initial_state(state.clone())
            .state_coordinator(failing_builder(Arc::clone(&calls)))
            .force_build(true)
            .assemble()
            .initialize()
            .await
            .unwrap();

        assert_eq!(&*manager.snapshot(), &state);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
