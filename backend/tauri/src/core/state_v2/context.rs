//! a task local context for reading locked RwLock State safely for transactional operations

use std::sync::Arc;

use dashmap::DashMap;
use std::any::{Any, TypeId};
use tokio::task_local;

type AnyContextMap = DashMap<TypeId, Box<dyn Any + Send + Sync + 'static>>;

trait AnyContextMapExt {
    fn get_state<T: Any + Send + Sync + Clone + 'static>(&self) -> Option<T>;
    fn set_state<T: Any + Send + Sync + Clone + 'static>(&self, value: T);
}

impl AnyContextMapExt for AnyContextMap {
    fn get_state<T: Any + Send + Sync + Clone + 'static>(&self) -> Option<T> {
        self.get(&TypeId::of::<T>()).map(|state| {
            let value = state
                .value()
                .downcast_ref::<T>()
                .expect("state is not T, possible State was corrupted?")
                .clone();
            value
        })
    }

    fn set_state<T: Any + Send + Sync + Clone + 'static>(&self, value: T) {
        self.insert(TypeId::of::<T>(), Box::new(value));
    }
}

task_local! {
    static STATE_CONTEXT: Arc<AnyContextMap>;
}

pub struct Context;

impl Context {
    pub fn get<T: Any + Send + Sync + Clone + 'static>() -> Option<T> {
        STATE_CONTEXT
            .try_with(|context| context.get_state::<T>())
            .ok()
            .flatten()
    }

    fn set<T: Any + Send + Sync + Clone + 'static>(value: T) {
        STATE_CONTEXT.with(|context| context.set_state::<T>(value));
    }

    fn is_in_scoped_context() -> bool {
        STATE_CONTEXT.try_with(|_| true).unwrap_or_default()
    }

    fn default_context() -> Arc<AnyContextMap> {
        Arc::new(AnyContextMap::new())
    }

    fn get_or_create_context() -> Arc<AnyContextMap> {
        STATE_CONTEXT
            .try_with(|context| context.clone())
            .unwrap_or_else(|_| Self::default_context())
    }

    /// Run state context in a scoped manner
    pub async fn scope<T, F>(state: T, f: F) -> F::Output
    where
        F: Future,
        T: Any + Send + Sync + Clone + 'static,
    {
        if Self::is_in_scoped_context() {
            Self::set(state);
            f.await
        } else {
            let context = Self::default_context();
            context.set_state(state);
            STATE_CONTEXT.scope(context, f).await
        }
    }
}

// TODO: support `spawn_blocking`?
pub trait SpawnContextExt {
    async fn spawn_context<T, F>(state: T, f: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
        T: Any + Send + Sync + Clone + 'static,
    {
        let current_context = Context::get_or_create_context();
        current_context.set_state(state);
        tokio::spawn(STATE_CONTEXT.scope(current_context, f))
    }
}

impl<F> SpawnContextExt for F
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
}
