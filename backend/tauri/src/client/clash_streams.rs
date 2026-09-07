use tokio::sync::broadcast;

use super::{NyanpasuClient, Result};
use crate::core::clash::ws::{
    ClashConnectionsConnectorEvent, ClashWsEvent, ClashWsKind, ClashWsRecording, ClashWsSnapshot,
};

impl NyanpasuClient {
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
}
