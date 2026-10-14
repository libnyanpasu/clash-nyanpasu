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
    pub core: Box<dyn super::CoreLogStore>,
    pub files: Arc<dyn LogFiles>,
    pub clock: Arc<dyn Clock>,
    pub service: Arc<dyn ServiceLogsPort>,
    pub frontend: Arc<dyn super::frontend::FrontendLogSink>,
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
