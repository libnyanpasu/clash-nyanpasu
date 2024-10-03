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

type StateReadLock<'a, T> = RwLockReadGuard<'a, RawRwLock, T>;
type DraftReadLock<'a, T> = RwLockReadGuard<'a, RawRwLock, Option<T>>;
type DraftWriteLock<'a, T> = RwLockWriteGuard<'a, RawRwLock, Option<T>>;

/// Here is a lock to hold the state
pub enum ManagedStateReadLocker<'a, T>
where
    T: Clone + Sync + Send,
{
    StateLock(StateReadLock<'a, T>),
    DraftLock(DraftReadLock<'a, T>),
}

impl<'a, T> ManagedStateReadLocker<'a, T>
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

    pub fn as_state(self) -> Option<StateReadLock<'a, T>> {
        match self {
            Self::StateLock(lock) => Some(lock),
            _ => None,
        }
    }

    pub fn as_draft(self) -> Option<DraftReadLock<'a, T>> {
        match self {
            Self::DraftLock(lock) => Some(lock),
            _ => None,
        }
    }

    pub fn unwrap(&self) -> &T {
        match self {
            Self::StateLock(lock) => lock,
            Self::DraftLock(lock) => lock.as_ref().unwrap(),
        }
    }
}

pub struct ManagedStateWriteLocker<'a, T>(DraftWriteLock<'a, T>);

impl<'a, T> ManagedStateWriteLocker<'a, T>
where
    T: Clone + Sync + Send,
{
    pub fn unwrap_mut(&mut self) -> &mut T {
        self.0.as_mut().unwrap()
    }

    pub fn unwrap(&self) -> &T {
        self.0.as_ref().unwrap()
    }
}

pub struct ManagedStateInner<T>
where
    T: Clone + Sync + Send,
{
    inner: RwLock<T>,
    draft: RwLock<Option<T>>,
    is_dirty: AtomicBool,
}

impl<T> ManagedStateInner<T>
where
    T: Clone + Sync + Send,
{
    /// create a new managed state
    pub fn new(state: T) -> Self {
        Self {
            inner: RwLock::new(state),
            draft: RwLock::new(None),
            is_dirty: AtomicBool::new(false),
        }
    }

    pub fn get_committed_state(&self) -> StateReadLock<'_, T> {
        self.inner.read()
    }

    /// get the current state, it will return the ManagedStateLocker for the state
    /// NOTE: you should call the `locker.unwrap()` to get the actual state
    pub fn get_state(&self) -> ManagedStateReadLocker<'_, T> {
        if self.is_dirty.load(std::sync::atomic::Ordering::Relaxed) {
            let draft = self.draft.read();
            ManagedStateReadLocker::DraftLock(draft)
        } else {
            let state = self.inner.read();
            ManagedStateReadLocker::StateLock(state)
        }
    }

    /// whether the state is dirty, i.e. a draft is present, and not yet committed or discarded
    pub fn is_dirty(&self) -> bool {
        self.is_dirty.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// You can modify the draft state, and then commit it
    pub fn draft(&self) -> ManagedStateWriteLocker<'_, T> {
        if self.is_dirty.load(std::sync::atomic::Ordering::Relaxed) {
            let draft = self.draft.write();
            return ManagedStateWriteLocker(draft);
        }

        let state = self.inner.read().clone();
        let mut draft = self.draft.write();
        draft.replace(state);
        self.is_dirty
            .store(true, std::sync::atomic::Ordering::Relaxed);
        ManagedStateWriteLocker(draft)
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
