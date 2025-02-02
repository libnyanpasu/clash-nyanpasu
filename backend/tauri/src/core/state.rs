#[allow(dead_code)]
use parking_lot::{
    lock_api::{RwLockReadGuard, RwLockWriteGuard},
    MappedRwLockReadGuard, MappedRwLockWriteGuard, RwLock,
};
use std::{
    ops::Deref,
    sync::{atomic::AtomicBool, Arc},
};

/// State manager for the application
/// It provides a way to manage the application state, draft and persist it
/// Note: It is safe to clone the StateManager, as it is backed by an Arc
#[derive(Clone)]
pub struct ManagedState<T>
where
    T: Clone + Sync + Send,
{
    inner: Arc<ManagedStateInner<T>>,
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

impl<T> ManagedState<T>
where
    T: Clone + Sync + Send,
{
    /// to auto commit the state when it is dropped
    pub fn auto_commit(&self) -> ManagedStateAutoCommit<T> {
        ManagedStateAutoCommit(self)
    }
}

pub struct ManagedStateAutoCommit<'a, T: Clone + Send + Sync>(&'a ManagedState<T>);

impl<T> Deref for ManagedStateAutoCommit<'_, T>
where
    T: Clone + Send + Sync,
{
    type Target = ManagedState<T>;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<T: Clone + Send + Sync> Drop for ManagedStateAutoCommit<'_, T> {
    fn drop(&mut self) {
        if self.0.is_dirty() {
            self.0.apply();
        }
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

    /// Get the committed state
    pub fn data(&self) -> MappedRwLockReadGuard<'_, T> {
        RwLockReadGuard::map(self.inner.read(), |guard| guard)
    }

    /// get the current state, it will return the ManagedStateLocker for the state
    pub fn latest(&self) -> MappedRwLockReadGuard<'_, T> {
        if self.is_dirty() {
            let draft = self.draft.read();
            RwLockReadGuard::map(draft, |guard| guard.as_ref().unwrap())
        } else {
            let state = self.inner.read();
            RwLockReadGuard::map(state, |guard| guard)
        }
    }

    /// whether the state is dirty, i.e. a draft is present, and not yet committed or discarded
    pub fn is_dirty(&self) -> bool {
        self.is_dirty.load(std::sync::atomic::Ordering::Acquire)
    }

    /// You can modify the draft state, and then commit it
    pub fn draft(&self) -> MappedRwLockWriteGuard<'_, T> {
        if self.is_dirty() {
            return RwLockWriteGuard::map(self.draft.write(), |guard| guard.as_mut().unwrap());
        }

        let state = self.inner.read().clone();
        self.is_dirty
            .store(true, std::sync::atomic::Ordering::Release);

        RwLockWriteGuard::map(self.draft.write(), move |guard| {
            *guard = Some(state);
            guard.as_mut().unwrap()
        })
    }

    /// commit the draft state, and make it the new state
    pub fn apply(&self) -> Option<T> {
        if !self.is_dirty() {
            return None;
        }

        let mut draft = self.draft.write();
        let mut inner = self.inner.write();
        let old_value = inner.to_owned();
        *inner = draft.take().unwrap();
        self.is_dirty
            .store(false, std::sync::atomic::Ordering::Release);
        Some(old_value)
    }

    /// discard the draft state
    pub fn discard(&self) -> Option<T> {
        let v = self.draft.write().take();
        self.is_dirty
            .store(false, std::sync::atomic::Ordering::Release);
        v
    }
}

mod test {
    #![allow(unused)]
    use super::ManagedState;
    use crate::config::IVerge;

    #[test]
    fn test_managed_state() {
        let verge = IVerge {
            enable_auto_launch: Some(true),
            enable_tun_mode: Some(false),
            ..IVerge::default()
        };

        let draft = ManagedState::from(verge);

        assert_eq!(draft.data().enable_auto_launch, Some(true));
        assert_eq!(draft.data().enable_tun_mode, Some(false));

        assert_eq!(draft.draft().enable_auto_launch, Some(true));
        assert_eq!(draft.draft().enable_tun_mode, Some(false));

        let mut d = draft.draft();
        d.enable_auto_launch = Some(false);
        d.enable_tun_mode = Some(true);
        drop(d);

        assert_eq!(draft.data().enable_auto_launch, Some(true));
        assert_eq!(draft.data().enable_tun_mode, Some(false));

        assert_eq!(draft.draft().enable_auto_launch, Some(false));
        assert_eq!(draft.draft().enable_tun_mode, Some(true));

        assert_eq!(draft.latest().enable_auto_launch, Some(false));
        assert_eq!(draft.latest().enable_tun_mode, Some(true));

        assert!(draft.apply().is_some());
        assert!(draft.apply().is_none());

        assert_eq!(draft.data().enable_auto_launch, Some(false));
        assert_eq!(draft.data().enable_tun_mode, Some(true));

        assert_eq!(draft.draft().enable_auto_launch, Some(false));
        assert_eq!(draft.draft().enable_tun_mode, Some(true));

        let mut d = draft.draft();
        d.enable_auto_launch = Some(true);
        drop(d);

        assert_eq!(draft.data().enable_auto_launch, Some(false));

        assert_eq!(draft.draft().enable_auto_launch, Some(true));

        assert!(draft.discard().is_some());

        assert_eq!(draft.data().enable_auto_launch, Some(false));

        assert!(draft.discard().is_none());

        assert_eq!(draft.draft().enable_auto_launch, Some(false));
    }
}
