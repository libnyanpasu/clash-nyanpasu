macro_rules! impl_state_manager_delegates {
    ($state:ty) => {
        pub fn snapshot(&self) -> std::sync::Arc<$state> {
            self.state_coordinator.snapshot()
        }

        pub fn snapshot_handle(&self) -> $crate::state::StateSnapshot<$state> {
            self.state_coordinator.snapshot_handle()
        }

        pub fn add_subscriber(
            &mut self,
            subscriber: Box<dyn $crate::state::StateAckSubscriber<$state> + Send + Sync>,
        ) {
            self.state_coordinator.add_subscriber(subscriber);
        }

        pub fn remove_subscriber(
            &mut self,
            name: &str,
        ) -> Option<std::sync::Arc<dyn $crate::state::StateAckSubscriber<$state> + Send + Sync>> {
            self.state_coordinator.remove_subscriber(name)
        }
    };
}

pub(crate) use impl_state_manager_delegates;

mod persistent_builder;
mod persistent_state;
mod simple;
mod weak_persistent_state;

use super::{builder::*, coordinator::*};

pub use persistent_builder::*;
pub use persistent_state::{PersistentStateManager, PersistentStateManagerSetup};
pub use simple::{SimpleStateManager, SimpleStateManagerSetup};
pub use weak_persistent_state::{WeakPersistentStateManager, WeakPersistentStateManagerSetup};
