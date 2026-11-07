//! Shared logs command adapters.

use nyanpasu_core::client::NyanpasuClient;

use super::error::Result;
use crate::unified_rpc::{RpcCaller, RpcState};

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn list_log_files(
    client: RpcState<'_, NyanpasuClient>,
    source: nyanpasu_core::client::logs::LogSource,
) -> nyanpasu_logging::LogResult<Vec<nyanpasu_logging::LogFileInfo>> {
    client.list_log_files(source).await
}

#[nyanpasu_macro::rpc(http, result, owner)]
#[specta::specta]
pub async fn open_log_session(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    source: nyanpasu_core::client::logs::LogSource,
    request: nyanpasu_logging::OpenLogs,
) -> nyanpasu_logging::LogResult<nyanpasu_logging::LogSession> {
    client
        .open_log_session(source, window.label().to_string(), request)
        .await
}

#[nyanpasu_macro::rpc(http, result, owner)]
#[specta::specta]
pub async fn query_logs(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    source: nyanpasu_core::client::logs::LogSource,
    request: nyanpasu_logging::QueryLogs,
) -> nyanpasu_logging::LogResult<nyanpasu_logging::LogPage> {
    client
        .query_logs(source, window.label().to_string(), request)
        .await
}

#[nyanpasu_macro::rpc(http, result, owner)]
#[specta::specta]
pub async fn close_log_session(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    source: nyanpasu_core::client::logs::LogSource,
    session: String,
) -> nyanpasu_logging::LogResult<()> {
    client
        .close_log_session(source, window.label().to_string(), session)
        .await
}

#[nyanpasu_macro::rpc(http, owner)]
#[specta::specta]
pub fn report_frontend_events(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    batch: nyanpasu_core::client::frontend_events::FrontendEventBatch,
) -> Result {
    client.report_frontend_events(window.label(), batch);
    Ok(())
}
