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
    CoreLocal,
    CoreService,
}

pub struct LoggingSetup {
    pub core_files: Arc<dyn LogFiles>,
    pub core_service: Arc<dyn ServiceLogsPort>,
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
pub struct IpcCoreLogs {
    client: IpcClient,
    instance: String,
}
impl IpcCoreLogs {
    pub fn new(client: IpcClient) -> Self {
        Self {
            client,
            instance: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn owner(&self, window: String) -> String {
        format!("{}/{}", self.instance, window)
    }
    async fn supported(&self) -> LogResult<()> {
        let status = self.bounded(self.client.status()).await?;
        if status.core_log_query_version != Some(nyanpasu_ipc::api::log::CORE_LOG_QUERY_VERSION) {
            return Err(LogError::Unsupported);
        }
        Ok(())
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
impl ServiceLogsPort for IpcCoreLogs {
    async fn catalog(&self) -> LogResult<Vec<LogFileInfo>> {
        self.supported().await?;
        self.bounded(self.client.core_log_files()).await?
    }
    async fn open(&self, owner: String, request: OpenLogs) -> LogResult<LogSession> {
        self.supported().await?;
        self.bounded(self.client.open_core_logs(&OwnedLogRequest {
            owner: self.owner(owner),
            request,
        }))
        .await?
    }
    async fn query(&self, owner: String, request: QueryLogs) -> LogResult<LogPage> {
        self.supported().await?;
        self.bounded(self.client.query_core_logs(&OwnedLogRequest {
            owner: self.owner(owner),
            request,
        }))
        .await?
    }
    async fn close(&self, owner: String, session: String) -> LogResult<()> {
        self.supported().await?;
        self.bounded(self.client.close_core_logs(&OwnedLogRequest {
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
            LogSource::CoreLocal => self.inner.core_logs.catalog().await,
            LogSource::CoreService => self.inner.core_service_logs.catalog().await,
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
            LogSource::CoreLocal => self.inner.core_logs.open(owner, request).await,
            LogSource::CoreService => self.inner.core_service_logs.open(owner, request).await,
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
            LogSource::CoreLocal => self.inner.core_logs.query(owner, request).await,
            LogSource::CoreService => self.inner.core_service_logs.query(owner, request).await,
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
            LogSource::CoreLocal => self.inner.core_logs.close(owner, session).await,
            LogSource::CoreService => self.inner.core_service_logs.close(owner, session).await,
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
        core_files: Arc::new(FsCoreLogFiles::new(directory.join("core"))),
        core_service: Arc::new(Unavailable),
        files: Arc::new(FsLogFiles::new(directory, "clash-nyanpasu".into())),
        clock: Arc::new(MonotonicClock::default()),
        service: Arc::new(Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestServiceLogs(LogsClient);

    #[async_trait]
    impl ServiceLogsPort for TestServiceLogs {
        async fn catalog(&self) -> LogResult<Vec<LogFileInfo>> {
            self.0.catalog().await
        }
        async fn open(&self, owner: String, request: OpenLogs) -> LogResult<LogSession> {
            self.0.open(owner, request).await
        }
        async fn query(&self, owner: String, request: QueryLogs) -> LogResult<LogPage> {
            self.0.query(owner, request).await
        }
        async fn close(&self, owner: String, session: String) -> LogResult<()> {
            self.0.close(owner, session).await
        }
    }

    fn archive(directory: &std::path::Path, message: &str) {
        std::fs::create_dir_all(directory).unwrap();
        let record = serde_json::json!({
            "t":"log", "at":1700000000000i64, "epoch":1, "kind":"mihomo",
            "stream":"stderr", "level":"error", "timestamp":null,
            "target":"startup", "message":message, "fields":[],
            "raw":message, "truncated":false,
        });
        std::fs::write(directory.join("core-000001.jsonl"), format!("{record}\n")).unwrap();
    }

    #[test]
    fn core_sources_read_stopped_archives_and_keep_sessions_isolated() {
        let directory = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        endpoint.set_status(
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
            None,
        );
        let mut args = crate::client::tests::test_client_args_with_endpoint(&directory, endpoint);
        archive(
            &args.paths.app_logs_dir().join("core"),
            "local startup failure",
        );
        let legacy = args.paths.app_logs_dir().join("core/current.redb");
        std::fs::write(&legacy, b"obsolete database").unwrap();
        let service_directory = directory.path().join("service-core");
        archive(&service_directory, "service startup failure");
        let remote = tauri::async_runtime::block_on(LogsClient::start(
            Arc::new(FsCoreLogFiles::new(service_directory)),
            Arc::new(MonotonicClock::default()),
        ))
        .unwrap();
        args.logging.core_service = Arc::new(TestServiceLogs(remote.clone()));
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        tauri::async_runtime::block_on(async {
            for (source, other_source, message) in [
                (
                    LogSource::CoreLocal,
                    LogSource::CoreService,
                    "local startup failure",
                ),
                (
                    LogSource::CoreService,
                    LogSource::CoreLocal,
                    "service startup failure",
                ),
            ] {
                assert_eq!(client.list_log_files(source).await.unwrap().len(), 1);
                let session = client
                    .open_log_session(
                        source,
                        "owner".into(),
                        OpenLogs {
                            request_id: "same-request".into(),
                            file: None,
                        },
                    )
                    .await
                    .unwrap();
                let target_host = match source {
                    LogSource::CoreLocal => crate::core::actor_v2::endpoint::ExecutionHost::Service,
                    LogSource::CoreService => crate::core::actor_v2::endpoint::ExecutionHost::Local,
                    _ => unreachable!(),
                };
                let target = crate::client::tests::TestControlEndpoint::succeeding_on(target_host);
                target.set_status(
                    Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
                    None,
                );
                client.inner.core_api.change_host(target).await.unwrap();
                assert_eq!(client.inner.core_api.status().host, target_host);
                let query = QueryLogs {
                    session: session.id.clone(),
                    filter: Filter::default(),
                    direction: Direction::Latest,
                    cursor: None,
                    limit: 200,
                };
                assert_eq!(
                    client
                        .query_logs(source, "other-owner".into(), query.clone())
                        .await
                        .unwrap_err(),
                    LogError::SessionExpired
                );
                assert_eq!(
                    client
                        .query_logs(other_source, "owner".into(), query.clone())
                        .await
                        .unwrap_err(),
                    LogError::SessionExpired
                );
                let page = tokio::time::timeout(Duration::from_secs(3), async {
                    loop {
                        let page = client
                            .query_logs(source, "owner".into(), query.clone())
                            .await
                            .unwrap();
                        if !page.building {
                            break page;
                        }
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                assert_eq!(page.rows[0].message, message);
                assert_eq!(page.rows[0].level, Level::Error);
                assert!(page.rows[0].raw.contains("\"stream\":\"stderr\""));
                assert_eq!(
                    client
                        .close_log_session(source, "other-owner".into(), session.id.clone())
                        .await
                        .unwrap_err(),
                    LogError::SessionExpired
                );
                assert!(
                    client
                        .query_logs(source, "owner".into(), query.clone())
                        .await
                        .is_ok()
                );
                client
                    .close_log_session(source, "owner".into(), session.id)
                    .await
                    .unwrap();
                assert_eq!(
                    client
                        .query_logs(source, "owner".into(), query)
                        .await
                        .unwrap_err(),
                    LogError::SessionExpired
                );
            }
            client.request_shutdown();
            client.wait_shutdown().await;
            remote.shutdown().await.unwrap();
        });
        assert_eq!(std::fs::read(legacy).unwrap(), b"obsolete database");
    }
}
