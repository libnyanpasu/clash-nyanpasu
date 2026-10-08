use std::sync::Arc;

use tokio::sync::{broadcast, watch};

use super::{NyanpasuClient, Result};
use crate::core::clash::ws::{
    ClashConnectionDetails, ClashConnectionsConnectorEvent, ClashWsEvent, ClashWsKind,
    ClashWsRecording, ClashWsSnapshot,
};

impl NyanpasuClient {
    pub async fn query_core_logs(
        &self,
        query: nyanpasu_core::logs::CoreLogQuery,
    ) -> nyanpasu_core::logs::CoreLogResult<nyanpasu_core::logs::CoreLogPage> {
        self.inner.core_logs.query(query).await
    }
    pub async fn get_core_log(
        &self,
        cursor: nyanpasu_core::logs::CoreLogCursor,
    ) -> nyanpasu_core::logs::CoreLogResult<nyanpasu_core::logs::CoreLogRecord> {
        self.inner.core_logs.detail(cursor).await
    }
    pub async fn get_core_log_status(
        &self,
    ) -> nyanpasu_core::logs::CoreLogResult<nyanpasu_core::logs::CoreLogStatus> {
        self.inner.core_logs.status().await
    }
    pub async fn clear_core_logs(&self) -> nyanpasu_core::logs::CoreLogResult<()> {
        self.inner.core_logs.clear().await
    }
    pub fn subscribe_core_logs(&self) -> watch::Receiver<nyanpasu_core::logs::CoreLogStatus> {
        self.inner.core_logs.subscribe()
    }
    pub async fn start_clash_streams(&self) -> Result<()> {
        self.inner.streams.start().await?;
        Ok(())
    }
    pub async fn clash_ws_snapshot(&self) -> Result<ClashWsSnapshot> {
        Ok(self.inner.streams.snapshot().await?)
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
    pub fn subscribe_clash_ws(&self) -> broadcast::Receiver<ClashWsEvent> {
        self.inner.streams.subscribe_ws()
    }
    pub fn subscribe_clash_connection_details(
        &self,
    ) -> watch::Receiver<Option<Arc<ClashConnectionDetails>>> {
        self.inner.streams.subscribe_connection_details()
    }
}
