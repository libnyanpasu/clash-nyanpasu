use super::NyanpasuClient;
use nyanpasu_core::logs::app::LogSource;
use nyanpasu_logging::*;
#[cfg(test)]
use {
    async_trait::async_trait,
    nyanpasu_core::logs::{
        RedbCoreLogStore,
        app::{LoggingSetup, ServiceLogsPort},
        frontend::TracingFrontendLogSink,
    },
    std::sync::Arc,
};

impl NyanpasuClient {
    pub async fn list_log_files(&self, source: LogSource) -> LogResult<Vec<LogFileInfo>> {
        match source {
            LogSource::App => self.inner.app_logs.catalog().await,
            LogSource::Service => self.inner.service_logs.catalog().await,
        }
    }
    pub async fn open_log_session(
        &self,
        source: LogSource,
        owner: String,
        request: OpenLogs,
    ) -> LogResult<LogSession> {
        match source {
            LogSource::App => self.inner.app_logs.open(owner, request).await,
            LogSource::Service => self.inner.service_logs.open(owner, request).await,
        }
    }
    pub async fn query_logs(
        &self,
        source: LogSource,
        owner: String,
        request: QueryLogs,
    ) -> LogResult<LogPage> {
        match source {
            LogSource::App => self.inner.app_logs.query(owner, request).await,
            LogSource::Service => self.inner.service_logs.query(owner, request).await,
        }
    }
    pub async fn close_log_session(
        &self,
        source: LogSource,
        owner: String,
        session: String,
    ) -> LogResult<()> {
        match source {
            LogSource::App => self.inner.app_logs.close(owner, session).await,
            LogSource::Service => self.inner.service_logs.close(owner, session).await,
        }
    }
}

#[cfg(test)]
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
