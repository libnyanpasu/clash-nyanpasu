use arc_swap::ArcSwap;
use std::sync::Arc;

pub struct StateSnapshot<T: Clone + Send + Sync + 'static>(Arc<ArcSwap<T>>);

impl<T> StateSnapshot<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn new(value: Arc<ArcSwap<T>>) -> Self {
        Self(value)
    }

    pub fn load(&self) -> Arc<T> {
        self.0.load_full()
    }
}

impl<T: Clone + Send + Sync + 'static> From<Arc<ArcSwap<T>>> for StateSnapshot<T> {
    fn from(arc_swap: Arc<ArcSwap<T>>) -> Self {
        Self(arc_swap)
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
