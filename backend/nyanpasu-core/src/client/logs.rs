use super::NyanpasuClient;
use async_trait::async_trait;
use nyanpasu_contracts::logs::LogSource;
use nyanpasu_ipc::{api::log::OwnedLogRequest, client::Client as IpcClient};
use nyanpasu_logging::*;
use nyanpasu_observers::logs::{CoreLogExport, CoreLogResult, CoreLogStore};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{sync::Arc, time::Duration};

/// Public filter for app/service log summaries without text or target substring search.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LogFilterSummary {
    pub levels: Vec<Level>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
}

/// Public query parameters for [`NyanpasuClient::query_logs_summary`].
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct QueryLogSummary {
    pub session: String,
    pub filter: LogFilterSummary,
    pub direction: Direction,
    pub cursor: Option<LogCursor>,
    pub limit: u32,
}

impl From<QueryLogSummary> for QueryLogs {
    fn from(query: QueryLogSummary) -> Self {
        let QueryLogSummary {
            session,
            filter:
                LogFilterSummary {
                    levels,
                    from_ms,
                    to_ms,
                },
            direction,
            cursor,
            limit,
        } = query;
        Self {
            session,
            filter: Filter {
                levels,
                target: None,
                from_ms,
                to_ms,
                text: None,
            },
            direction,
            cursor,
            limit,
        }
    }
}

/// Public app/service log row without message text, raw line text, or target string.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct LogRowSummary {
    pub id: String,
    pub timestamp: Option<String>,
    pub level: Level,
    pub has_target: bool,
    pub unparsed: bool,
    pub truncated: bool,
}

impl From<&LogRow> for LogRowSummary {
    fn from(row: &LogRow) -> Self {
        let LogRow {
            id,
            timestamp,
            level,
            target,
            message: _,
            raw: _,
            unparsed,
            truncated,
        } = row;
        Self {
            id: id.clone(),
            timestamp: timestamp
                .as_deref()
                .and_then(|value| value.parse::<i64>().ok())
                .map(|value| value.to_string()),
            level: *level,
            has_target: !target.is_empty(),
            unparsed: *unparsed,
            truncated: *truncated,
        }
    }
}

/// Public app/service log page without raw line text, log messages, or target names.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LogPageSummary {
    pub rows: Vec<LogRowSummary>,
    pub cursor: LogCursor,
    pub head: LogCursor,
    pub start: String,
    pub file: String,
    pub building: bool,
    pub more: bool,
    pub partial: bool,
    pub malformed: String,
    pub truncated: String,
    pub indexed_bytes: String,
    pub file_bytes: String,
}

impl From<&LogPage> for LogPageSummary {
    fn from(page: &LogPage) -> Self {
        let LogPage {
            rows,
            cursor,
            head,
            start,
            file,
            building,
            more,
            partial,
            malformed,
            truncated,
            indexed_bytes,
            file_bytes,
        } = page;
        Self {
            rows: rows.iter().map(LogRowSummary::from).collect(),
            cursor: cursor.clone(),
            head: head.clone(),
            start: start.clone(),
            file: file.clone(),
            building: *building,
            more: *more,
            partial: *partial,
            malformed: malformed.clone(),
            truncated: truncated.clone(),
            indexed_bytes: indexed_bytes.clone(),
            file_bytes: file_bytes.clone(),
        }
    }
}

impl From<LogPage> for LogPageSummary {
    fn from(page: LogPage) -> Self {
        Self::from(&page)
    }
}

pub struct LoggingSetup {
    pub core: Box<dyn CoreLogStore>,
    pub files: Arc<dyn LogFiles>,
    pub clock: Arc<dyn Clock>,
    pub service: Arc<dyn ServiceLogsPort>,
    pub frontend: Arc<dyn super::frontend_events::FrontendLogSink>,
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
    pub async fn export_core_logs(&self, home: Option<String>) -> CoreLogResult<CoreLogExport> {
        self.inner.core_logs.export(home).await
    }

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
    pub async fn query_logs_summary(
        &self,
        source: LogSource,
        owner: String,
        request: QueryLogSummary,
    ) -> LogResult<LogPageSummary> {
        self.query_logs(source, owner, request.into())
            .await
            .map(LogPageSummary::from)
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

#[cfg(any(test, feature = "test-support"))]
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
        core: Box::new(crate::logs::RedbCoreLogStore::open(directory.join("core")).unwrap()),
        files: Arc::new(FsLogFiles::new(directory, "clash-nyanpasu".into())),
        clock: Arc::new(MonotonicClock::default()),
        service: Arc::new(Unavailable),
        frontend: Arc::new(super::frontend_events::TracingFrontendLogSink),
    }
}

#[cfg(test)]
mod tests {
    use super::{LogFilterSummary, LogPageSummary, QueryLogSummary};
    use nyanpasu_logging::{Direction, Level, LogCursor, LogPage, LogRow, QueryLogs};

    #[test]
    fn query_log_summary_omits_target_and_text_filters() {
        let query = QueryLogSummary {
            session: "s1".into(),
            filter: LogFilterSummary {
                levels: vec![Level::Warn, Level::Error],
                from_ms: Some(100),
                to_ms: Some(200),
            },
            direction: Direction::Before,
            cursor: Some(LogCursor {
                generation: "g1".into(),
                offset: "64".into(),
            }),
            limit: 50,
        };
        let wire: QueryLogs = query.into();
        assert_eq!(wire.session, "s1");
        assert_eq!(wire.filter.levels, vec![Level::Warn, Level::Error]);
        assert_eq!(wire.filter.from_ms, Some(100));
        assert_eq!(wire.filter.to_ms, Some(200));
        assert!(wire.filter.target.is_none());
        assert!(wire.filter.text.is_none());
        assert_eq!(wire.direction, Direction::Before);
    }

    #[test]
    fn log_page_summary_drops_message_raw_and_target_string() {
        let page = LogPage {
            rows: vec![
                LogRow {
                    id: "g1:0".into(),
                    timestamp: Some("1700000000000".into()),
                    level: Level::Error,
                    target: "target-secret-sentinel".into(),
                    message: "message-secret-sentinel".into(),
                    raw: "{\"raw\":\"raw-secret-sentinel\"}".into(),
                    unparsed: true,
                    truncated: true,
                },
                LogRow {
                    id: "g1:100".into(),
                    timestamp: None,
                    level: Level::Info,
                    target: String::new(),
                    message: "another-secret-sentinel".into(),
                    raw: "another-raw-sentinel".into(),
                    unparsed: false,
                    truncated: false,
                },
            ],
            cursor: LogCursor {
                generation: "g1".into(),
                offset: "0".into(),
            },
            head: LogCursor {
                generation: "g1".into(),
                offset: "200".into(),
            },
            start: "0".into(),
            file: "audit.app.log".into(),
            building: false,
            more: true,
            partial: false,
            malformed: "1".into(),
            truncated: "1".into(),
            indexed_bytes: "200".into(),
            file_bytes: "256".into(),
        };

        let summary = LogPageSummary::from(&page);
        assert_eq!(summary.rows.len(), 2);
        assert_eq!(summary.rows[0].timestamp.as_deref(), Some("1700000000000"));
        assert!(summary.rows[0].has_target);
        assert!(summary.rows[0].unparsed);
        assert!(summary.rows[0].truncated);
        assert!(!summary.rows[1].has_target);
        assert_eq!(summary.file, "audit.app.log");

        let non_numeric = LogRow {
            id: "g1:200".into(),
            timestamp: Some("2026-01-01 Bearer timestamp-secret-sentinel".into()),
            level: Level::Warn,
            target: String::new(),
            message: String::new(),
            raw: String::new(),
            unparsed: false,
            truncated: false,
        };
        assert_eq!(super::LogRowSummary::from(&non_numeric).timestamp, None);

        assert!(
            serde_json::from_str::<QueryLogSummary>(
                r#"{"session":"s1","filter":{"levels":[],"text":"oracle"},"direction":"before","cursor":null,"limit":10}"#,
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<QueryLogSummary>(
                r#"{"session":"s1","filter":{"levels":[],"target":"oracle"},"direction":"before","cursor":null,"limit":10}"#,
            )
            .is_err()
        );

        let serialized = serde_json::to_string(&summary).unwrap();
        for forbidden in [
            "target-secret-sentinel",
            "message-secret-sentinel",
            "raw-secret-sentinel",
            "another-secret-sentinel",
            "another-raw-sentinel",
        ] {
            assert!(!serialized.contains(forbidden), "{serialized}");
        }
    }
}
