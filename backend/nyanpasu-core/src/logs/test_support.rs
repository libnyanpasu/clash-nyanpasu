//! Temporary-directory-backed log storage and actor fixtures shared by backend tests.

use tempfile::TempDir;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    CoreLogCursor, CoreLogExport, CoreLogPage, CoreLogQuery, CoreLogRecord, CoreLogResult,
    CoreLogStatus, CoreLogStore, CoreLogsClient, PreparedCoreLog, RedbCoreLogStore,
};

pub(super) struct TestStore {
    // Field drop order keeps the directory until the database has closed.
    pub(super) inner: RedbCoreLogStore,
    pub(super) _directory: TempDir,
}
impl CoreLogStore for TestStore {
    fn configure(
        &mut self,
        settings: nyanpasu_config::application::CoreLogSettings,
    ) -> CoreLogResult<()> {
        self.inner.configure(settings)
    }
    fn append(&mut self, records: &[PreparedCoreLog]) -> CoreLogResult<()> {
        self.inner.append(records)
    }
    fn query(&mut self, request: CoreLogQuery) -> CoreLogResult<CoreLogPage> {
        self.inner.query(request)
    }
    fn detail(&mut self, cursor: CoreLogCursor) -> CoreLogResult<CoreLogRecord> {
        self.inner.detail(cursor)
    }
    fn export(&mut self, home: Option<String>) -> CoreLogResult<CoreLogExport> {
        self.inner.export(home)
    }
    fn clear(&mut self) -> CoreLogResult<()> {
        self.inner.clear()
    }
    fn status(&mut self) -> CoreLogResult<CoreLogStatus> {
        self.inner.status()
    }
}
impl CoreLogsClient {
    pub async fn test_client() -> Self {
        let directory = TempDir::new().unwrap();
        let store = TestStore {
            inner: RedbCoreLogStore::open(directory.path().into()).unwrap(),
            _directory: directory,
        };
        let client = Self::spawn(
            Box::new(store),
            Default::default(),
            CancellationToken::new(),
            &TaskTracker::new(),
        )
        .await
        .unwrap();
        client.set_instance(Some("instance".into())).await.unwrap();
        client
    }
}
