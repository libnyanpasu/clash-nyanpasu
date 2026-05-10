use arc_swap::ArcSwap;
use std::sync::Arc;

use crate::state::VersionedState;

pub struct StateSnapshot<T: Clone + Send + Sync + 'static>(Arc<ArcSwap<VersionedState<T>>>);

impl<T> StateSnapshot<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub(crate) fn from_store(value: Arc<ArcSwap<VersionedState<T>>>) -> Self {
        Self(value)
    }

    pub fn load(&self) -> Arc<VersionedState<T>> {
        self.0.load_full()
    }
}

impl<T: Clone + Send + Sync + 'static> Clone for StateSnapshot<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: Clone + Send + Sync + 'static> std::fmt::Debug for StateSnapshot<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("StateSnapshot")
            .field(&format_args!("..."))
            .finish()
    }
}
