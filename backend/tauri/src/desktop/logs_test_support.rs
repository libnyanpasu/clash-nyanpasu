use async_trait::async_trait;
use nyanpasu_core::logs::{
    RedbCoreLogStore,
    app::{LoggingSetup, ServiceLogsPort},
    frontend::TracingFrontendLogSink,
};
use nyanpasu_logging::*;
use std::sync::Arc;

pub(crate) fn test_setup(directory: std::path::PathBuf) -> LoggingSetup {
    struct Unavailable;
    #[async_trait]
    impl ServiceLogsPort for Unavailable {
        async fn catalog(&self) -> LogResult<Vec<LogFileInfo>> {
            Err(LogError::Unsupported)
        }
        async fn open(&self, _: String, _: OpenLogs) -> LogResult<LogSession> {
            Err(LogError::Unsupported)
        }
        async fn query(&self, _: String, _: QueryLogs) -> LogResult<LogPage> {
            Err(LogError::Unsupported)
        }
        async fn close(&self, _: String, _: String) -> LogResult<()> {
            Ok(())
        }
    }
    LoggingSetup {
        core: Box::new(RedbCoreLogStore::open(directory.join("core")).unwrap()),
        files: Arc::new(FsLogFiles::new(directory, "clash-nyanpasu".into())),
        clock: Arc::new(MonotonicClock::default()),
        service: Arc::new(Unavailable),
        frontend: Arc::new(TracingFrontendLogSink),
    }
}
