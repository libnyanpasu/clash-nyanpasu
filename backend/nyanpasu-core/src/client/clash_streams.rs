use std::sync::Arc;

use nyanpasu_contracts::logs::{CoreLogCursor, CoreLogStatus, CoreLogStatusSummary};
use nyanpasu_observers::logs::{
    CoreLogPage, CoreLogPageSummary, CoreLogQuery, CoreLogRecord, CoreLogResult,
    CoreLogSummaryQuery,
};
use tokio::sync::{broadcast, watch};

use super::{ClashReadError, CoreLogReadError, NyanpasuClient, Result};
use crate::clash::ws::{
    ClashConnectionDetails, ClashConnectionsConnectorEvent, ClashWsKind, ClashWsPacket,
    ClashWsRecording, ClashWsSnapshot,
};

impl NyanpasuClient {
    pub async fn query_core_logs(&self, query: CoreLogQuery) -> CoreLogResult<CoreLogPage> {
        self.inner.core_logs.query(query).await
    }
    pub async fn query_core_logs_summary(
        &self,
        query: CoreLogSummaryQuery,
    ) -> std::result::Result<CoreLogPageSummary, CoreLogReadError> {
        let query = CoreLogQuery::try_from(query).map_err(CoreLogReadError::from)?;
        self.inner
            .core_logs
            .query(query)
            .await
            .map(CoreLogPageSummary::from)
            .map_err(CoreLogReadError::from)
    }
    pub async fn get_core_log(&self, cursor: CoreLogCursor) -> CoreLogResult<CoreLogRecord> {
        self.inner.core_logs.detail(cursor).await
    }
    pub async fn get_core_log_status(&self) -> CoreLogResult<CoreLogStatus> {
        self.inner.core_logs.status().await
    }
    pub async fn get_core_log_status_summary(
        &self,
    ) -> std::result::Result<CoreLogStatusSummary, CoreLogReadError> {
        self.inner
            .core_logs
            .status()
            .await
            .map(CoreLogStatusSummary::from)
            .map_err(CoreLogReadError::from)
    }
    pub async fn clear_core_logs(&self) -> CoreLogResult<()> {
        self.inner.core_logs.clear().await
    }
    pub fn subscribe_core_logs(&self) -> watch::Receiver<CoreLogStatus> {
        self.inner.core_logs.subscribe()
    }
    pub async fn start_clash_streams(&self) -> Result<()> {
        self.inner.streams.start().await?;
        Ok(())
    }
    pub async fn clash_ws_snapshot(&self) -> std::result::Result<ClashWsSnapshot, ClashReadError> {
        self.inner
            .streams
            .snapshot()
            .await
            .map_err(|_| ClashReadError::StreamsUnavailable)
    }
    pub async fn set_clash_ws_recording(
        &self,
        kind: ClashWsKind,
        enabled: bool,
    ) -> Result<ClashWsRecording> {
        Ok(self.inner.streams.set_recording(kind, enabled).await?)
    }
    pub async fn clear_clash_ws_history(&self, kind: ClashWsKind) -> Result<()> {
        self.inner.streams.clear_history(kind).await?;
        Ok(())
    }
    pub fn subscribe_clash_connections(
        &self,
    ) -> broadcast::Receiver<ClashConnectionsConnectorEvent> {
        self.inner.streams.subscribe()
    }
    pub fn subscribe_clash_ws(&self) -> broadcast::Receiver<ClashWsPacket> {
        self.inner.streams.subscribe_ws()
    }
    pub async fn subscribe_clash_connection_details(
        &self,
    ) -> std::result::Result<watch::Receiver<Option<Arc<ClashConnectionDetails>>>, ClashReadError>
    {
        self.inner
            .streams
            .subscribe_connection_details()
            .await
            .map_err(|_| ClashReadError::StreamsUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use nyanpasu_observers::logs::CoreLogError;

    use crate::client::CoreLogReadError;

    #[test]
    fn unavailable_core_log_error_drops_backend_message() {
        let converted = CoreLogReadError::from(CoreLogError::Unavailable(
            "/private/core-logs.redb: error-sentinel".into(),
        ));
        assert_eq!(converted, CoreLogReadError::Unavailable);
        for rendered in [converted.to_string(), format!("{converted:?}")] {
            assert!(!rendered.contains("sentinel"), "{rendered}");
            assert!(!rendered.contains("core-logs.redb"), "{rendered}");
        }
        assert!(std::error::Error::source(&converted).is_none());
    }
}
