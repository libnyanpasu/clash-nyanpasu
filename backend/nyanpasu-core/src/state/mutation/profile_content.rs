//! Keeps build inputs consistent across managed profile file and document writes.

use std::sync::Arc;

use nyanpasu_config::profile::Profiles;
use nyanpasu_state_engine::{StateSnapshot, VersionedState};

/// Held by the Profiles owner through document publication or compensation: a
/// profile write promotes its file before the document is published, so the
/// files do not match the committed document in between. Validation and runtime
/// application never hold it. Readers go through [`ProfileContentReader`].
#[derive(Clone, Default)]
pub struct ProfileContentLock(Arc<tokio::sync::RwLock<()>>);

impl ProfileContentLock {
    pub(crate) async fn write(&self) -> tokio::sync::RwLockWriteGuard<'_, ()> {
        self.0.write().await
    }
}

/// The only way to read managed profile files together with the document they
/// belong to. A reader that loads the document and the files separately can
/// pair a promoted file with the document that preceded it, or read content a
/// failed write is about to compensate.
#[derive(Clone)]
pub struct ProfileContentReader {
    committed: StateSnapshot<Profiles>,
    lock: ProfileContentLock,
}

impl ProfileContentReader {
    pub(crate) fn new(committed: StateSnapshot<Profiles>, lock: ProfileContentLock) -> Self {
        Self { committed, lock }
    }

    /// Runs `read` on the committed document. No profile write replaces a file
    /// or publishes a document until the returned future completes, so `read`
    /// must stay local: it holds every profile write back.
    pub(crate) async fn committed<T, Fut>(
        &self,
        read: impl FnOnce(Arc<VersionedState<Profiles>>) -> Fut,
    ) -> T
    where
        Fut: Future<Output = T>,
    {
        let _guard = self.lock.0.read().await;
        read(self.committed.load()).await
    }

    /// Reads the files a candidate document refers to, under the same guarantee.
    /// The caller brings the candidate; the committed document is not involved.
    pub(crate) async fn candidate<T>(&self, read: impl Future<Output = T>) -> T {
        let _guard = self.lock.0.read().await;
        read.await
    }
}
