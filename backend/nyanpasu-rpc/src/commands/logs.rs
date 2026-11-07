//! Shared logs command adapters.
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::wire::logs::{FrontendEventBatch, LogSource};

use crate::{
    error::Result,
    projection::IntoApplication,
    registry::{RpcCaller, RpcState},
};

#[nyanpasu_macro::rpc(local, http, result)]
#[specta::specta]
pub async fn list_log_files(
    client: RpcState<'_, NyanpasuClient>,
    source: LogSource,
) -> nyanpasu_logging::LogResult<Vec<nyanpasu_logging::LogFileInfo>> {
    let source = source.into_application();
    client.list_log_files(source).await
}

#[nyanpasu_macro::rpc(local, http, result, owner)]
#[specta::specta]
pub async fn open_log_session(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    source: LogSource,
    request: nyanpasu_logging::OpenLogs,
) -> nyanpasu_logging::LogResult<nyanpasu_logging::LogSession> {
    let source = source.into_application();
    client
        .open_log_session(source, window.label().to_string(), request)
        .await
}

#[nyanpasu_macro::rpc(local, http, result, owner)]
#[specta::specta]
pub async fn query_logs(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    source: LogSource,
    request: nyanpasu_logging::QueryLogs,
) -> nyanpasu_logging::LogResult<nyanpasu_logging::LogPage> {
    let source = source.into_application();
    client
        .query_logs(source, window.label().to_string(), request)
        .await
}

#[nyanpasu_macro::rpc(local, http, result, owner)]
#[specta::specta]
pub async fn close_log_session(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    source: LogSource,
    session: String,
) -> nyanpasu_logging::LogResult<()> {
    let source = source.into_application();
    client
        .close_log_session(source, window.label().to_string(), session)
        .await
}

#[nyanpasu_macro::rpc(http, owner)]
#[specta::specta]
pub fn report_frontend_events(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    batch: FrontendEventBatch,
) -> Result {
    let batch = batch.into_application();
    client.report_frontend_events(window.label(), batch);
    Ok(())
}
