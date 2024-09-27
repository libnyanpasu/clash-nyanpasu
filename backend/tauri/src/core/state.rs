use parking_lot::{lock_api::RwLockWriteGuard, RawRwLock, RwLock};
use std::{
    ops::Deref,
    sync::{atomic::AtomicBool, Arc},
};

/// State manager for the application
/// It provides a way to manage the application state, draft and persist it
/// Note: It is safe to clone the StateManager, as it is backed by an Arc
pub struct StateManager<T>
where
    T: Clone + Sync + Send,
{
    inner: Arc<StateManagerInner<T>>,
}

impl<T> Clone for StateManager<T>
where
    T: Clone + Sync + Send,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Deref for StateManager<T>
where
    T: Clone + Sync + Send,
{
    type Target = StateManagerInner<T>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub struct StateManagerInner<T>
where
    T: Clone + Sync + Send,
{
    inner: RwLock<Arc<T>>,
    draft: RwLock<Option<Arc<T>>>,
    is_dirty: AtomicBool,
}

impl<T> StateManagerInner<T>
where
    T: Clone + Sync + Send,
{
    /// create a new managed state
    pub fn new(state: T) -> Self {
        Self {
            inner: RwLock::new(Arc::new(state)),
            draft: RwLock::new(None),
            is_dirty: AtomicBool::new(false),
        }
    }

    /// get the current state
    pub fn get_state(&self) -> Arc<T> {
        if self.is_dirty.load(std::sync::atomic::Ordering::Relaxed) {
            self.draft.read().clone().unwrap()
        } else {
            self.inner.read().clone()
        }
    }

    /// whether the state is dirty, i.e. a draft is present, and not yet committed or discarded
    pub fn is_dirty(&self) -> bool {
        self.is_dirty.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// you can modify the draft state, and then commit it
    /// NOTE: it is not safe to modify the Arc<T> directly, if you drop the write guard, the state might be corrupted.
    pub fn draft(&self) -> (RwLockWriteGuard<'_, RawRwLock, Option<Arc<T>>>, Arc<T>) {
        if self.is_dirty.load(std::sync::atomic::Ordering::Relaxed) {
            let draft = self.draft.write();
            let state = draft.clone().unwrap();
            return (draft, state);
        }

        let state = self.inner.read().clone();
        let state = Arc::new(state.as_ref().clone());
        let mut draft = self.draft.write();
        draft.replace(state.clone());
        self.is_dirty
            .store(true, std::sync::atomic::Ordering::Relaxed);
        (draft, state)
    }

    /// commit the draft state, and make it the new state
    pub fn commit(&self) {
        let mut draft = self.draft.write();
        let mut inner = self.inner.write();
        *inner = draft.take().unwrap();
        self.is_dirty
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// discard the draft state
    pub fn discard(&self) {
        self.draft.write().take();
        self.is_dirty
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}
