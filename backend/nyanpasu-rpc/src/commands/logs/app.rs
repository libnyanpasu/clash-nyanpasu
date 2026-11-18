//! Application logs: log files, log sessions and frontend event reports.

use nyanpasu_core::client::NyanpasuClient;

use crate::{
    error::Result,
    registry::{RpcCaller, RpcState},
};

nyanpasu_rpc::resource!(route "logs_archive", targets = [http], sensitive);

#[nyanpasu_rpc::rpc(result, owner, operation = nyanpasu_rpc_protocol::operations::CloseLogSession, access = read)]
#[specta::specta]
pub async fn close_log_session(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    source: nyanpasu_contracts::logs::LogSource,
    session: String,
) -> nyanpasu_logging::LogResult<()> {
    client
        .close_log_session(source, window.label().to_string(), session)
        .await
}

#[nyanpasu_rpc::rpc(result, operation = nyanpasu_rpc_protocol::operations::ListLogFiles, query)]
#[specta::specta]
pub async fn list_log_files(
    client: RpcState<'_, NyanpasuClient>,
    source: nyanpasu_contracts::logs::LogSource,
) -> nyanpasu_logging::LogResult<Vec<nyanpasu_logging::LogFileInfo>> {
    client.list_log_files(source).await
}

#[nyanpasu_rpc::rpc(result, owner, operation = nyanpasu_rpc_protocol::operations::OpenLogSession, access = read)]
#[specta::specta]
pub async fn open_log_session(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    source: nyanpasu_contracts::logs::LogSource,
    request: nyanpasu_logging::OpenLogs,
) -> nyanpasu_logging::LogResult<nyanpasu_logging::LogSession> {
    client
        .open_log_session(source, window.label().to_string(), request)
        .await
}

#[nyanpasu_rpc::rpc(result, owner, operation = nyanpasu_rpc_protocol::operations::QueryLogs, access = read, sensitive)]
#[specta::specta]
pub async fn query_logs(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    source: nyanpasu_contracts::logs::LogSource,
    request: nyanpasu_logging::QueryLogs,
) -> nyanpasu_logging::LogResult<nyanpasu_logging::LogPage> {
    client
        .query_logs(source, window.label().to_string(), request)
        .await
}

/// Public app and service log page summary without message text, raw lines, target strings, or text filters.
#[nyanpasu_rpc::rpc(result, owner, access = read, expose = [http])]
#[specta::specta]
pub async fn query_logs_summary(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    source: nyanpasu_contracts::logs::LogSource,
    request: nyanpasu_core::client::logs::QueryLogSummary,
) -> nyanpasu_logging::LogResult<nyanpasu_core::client::logs::LogPageSummary> {
    client
        .query_logs_summary(source, window.label().to_string(), request)
        .await
}

#[nyanpasu_rpc::rpc(owner, expose = [http])]
#[specta::specta]
pub fn report_frontend_events(
    window: RpcCaller,
    client: RpcState<'_, NyanpasuClient>,
    batch: nyanpasu_core::client::frontend_events::FrontendEventBatch,
) -> Result {
    client.report_frontend_events(window.label(), batch);
    Ok(())
}
