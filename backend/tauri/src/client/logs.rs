use super::NyanpasuClient;
use async_trait::async_trait;
use nyanpasu_ipc::{api::log::OwnedLogRequest, client::Client as IpcClient};
use nyanpasu_logging::*;
use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    App,
    Service,
}

pub struct LoggingSetup {
    pub files: Arc<dyn LogFiles>,
    pub clock: Arc<dyn Clock>,
    pub service: Arc<dyn ServiceLogsPort>,
}
#[async_trait]
pub trait ServiceLogsPort: Send + Sync + 'static {
    async fn catalog(&self) -> LogResult<Vec<LogFileInfo>>;
    async fn open(&self, owner: String, request: OpenLogs) -> LogResult<LogSession>;
    async fn query(&self, owner: String, request: QueryLogs) -> LogResult<LogPage>;
    async fn close(&self, owner: String, session: String) -> LogResult<()>;
}
pub struct IpcServiceLogs {
    client: IpcClient,
    instance: String,
}
impl IpcServiceLogs {
    pub fn new(client: IpcClient) -> Self {
        Self {
            client,
            instance: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn owner(&self, window: String) -> String {
        format!("{}/{}", self.instance, window)
    }
    async fn bounded<T>(
        &self,
        future: impl std::future::Future<Output = nyanpasu_ipc::client::Result<T>>,
    ) -> LogResult<T> {
        tokio::time::timeout(Duration::from_secs(5), future)
            .await
            .map_err(|_| LogError::Unavailable)?
            .map_err(|_| LogError::Unavailable)
    }
}
#[async_trait]
impl ServiceLogsPort for IpcServiceLogs {
    async fn catalog(&self) -> LogResult<Vec<LogFileInfo>> {
        let status = self.bounded(self.client.status()).await?;
        if status.log_query_version != Some(nyanpasu_ipc::api::log::LOG_QUERY_VERSION) {
            return Err(LogError::Unsupported);
        }
        self.bounded(self.client.log_files()).await?
    }
    async fn open(&self, owner: String, request: OpenLogs) -> LogResult<LogSession> {
        self.bounded(self.client.open_logs(&OwnedLogRequest {
            owner: self.owner(owner),
            request,
        }))
        .await?
    }
    async fn query(&self, owner: String, request: QueryLogs) -> LogResult<LogPage> {
        self.bounded(self.client.query_logs(&OwnedLogRequest {
            owner: self.owner(owner),
            request,
        }))
        .await?
    }
    async fn close(&self, owner: String, session: String) -> LogResult<()> {
        self.bounded(self.client.close_logs(&OwnedLogRequest {
            owner: self.owner(owner),
            request: session,
        }))
        .await?
    }
}
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
    pub async fn shutdown_logs(&self) -> LogResult<()> {
        self.inner.app_logs.shutdown().await
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
        files: Arc::new(FsLogFiles::new(directory, "clash-nyanpasu".into())),
        clock: Arc::new(MonotonicClock::default()),
        service: Arc::new(Unavailable),
    }
}
