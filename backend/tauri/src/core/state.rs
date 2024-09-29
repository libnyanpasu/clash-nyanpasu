use parking_lot::{
    lock_api::{RwLockReadGuard, RwLockWriteGuard},
    RawRwLock, RwLock,
};
use std::{
    ops::Deref,
    sync::{atomic::AtomicBool, Arc},
};

/// State manager for the application
/// It provides a way to manage the application state, draft and persist it
/// Note: It is safe to clone the StateManager, as it is backed by an Arc
pub struct ManagedState<T>
where
    T: Clone + Sync + Send,
{
    inner: Arc<ManagedStateInner<T>>,
}

impl<T> Clone for ManagedState<T>
where
    T: Clone + Sync + Send,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Deref for ManagedState<T>
where
    T: Clone + Sync + Send,
{
    type Target = ManagedStateInner<T>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> From<T> for ManagedState<T>
where
    T: Clone + Sync + Send,
{
    fn from(state: T) -> Self {
        Self {
            inner: Arc::new(ManagedStateInner::new(state)),
        }
    }
}

/// Here is a lock to hold the state
pub enum ManagedStateLocker<'a, T>
where
    T: Clone + Sync + Send,
{
    StateLock(RwLockReadGuard<'a, RawRwLock, Arc<T>>),
    DraftLock(RwLockReadGuard<'a, RawRwLock, Option<Arc<T>>>),
}

impl<'a, T> ManagedStateLocker<'a, T>
where
    T: Clone + Sync + Send,
{
    pub fn is_state(&self) -> bool {
        match self {
            Self::StateLock(_) => true,
            _ => false,
        }
    }

    pub fn is_draft(&self) -> bool {
        match self {
            Self::DraftLock(_) => true,
            _ => false,
        }
    }

    pub fn as_state(self) -> Option<RwLockReadGuard<'a, RawRwLock, Arc<T>>> {
        match self {
            Self::StateLock(lock) => Some(lock),
            _ => None,
        }
    }

    pub fn as_draft(self) -> Option<RwLockReadGuard<'a, RawRwLock, Option<Arc<T>>>> {
        match self {
            Self::DraftLock(lock) => Some(lock),
            _ => None,
        }
    }
}

pub struct ManagedStateInner<T>
where
    T: Clone + Sync + Send,
{
    inner: RwLock<Arc<T>>,
    draft: RwLock<Option<Arc<T>>>,
    is_dirty: AtomicBool,
}

impl<T> ManagedStateInner<T>
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

    /// get the safe state
    /// this is a clone of the current state, and is safe to use
    pub fn safe_state(&self) -> Arc<T> {
        self.inner.read().clone()
    }

    /// get the current state
    /// Note: this operation will get the current state, and immediately drop the inner lock.
    /// It is not safe if you want to block the state from changing while you are using it.
    pub fn get_state(&self) -> Arc<T> {
        if self.is_dirty.load(std::sync::atomic::Ordering::Relaxed) {
            self.draft.read().clone().unwrap()
        } else {
            self.inner.read().clone()
        }
    }


    /// get the current state with a locker
    /// this will lock the state, and return a locker to hold the state
    /// this is useful if you want to block the state from changing while you are using it.
    pub fn get_state_with_locker(&self) -> ManagedStateLocker<'_, T> {
        if self.is_dirty.load(std::sync::atomic::Ordering::Relaxed) {
            let draft = self.draft.read();
            ManagedStateLocker::DraftLock(draft)
        } else {
            let state = self.inner.read();
            ManagedStateLocker::StateLock(state)
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
