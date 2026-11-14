//! Keeps build inputs consistent across managed profile file and document writes.

use std::sync::Arc;

/// Shared by the Profiles owner and build capture. Readers hold one guard while
/// loading the committed document and its files; writers hold it through document
/// publication or compensation. Validation and runtime application never hold it.
#[derive(Clone, Default)]
pub struct ProfileContentLock(Arc<tokio::sync::RwLock<()>>);

impl ProfileContentLock {
    pub(crate) async fn read(&self) -> tokio::sync::RwLockReadGuard<'_, ()> {
        self.0.read().await
    }

    pub(crate) async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, ()> {
        self.0.write().await
    }
}
