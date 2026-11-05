//! The core's log store and the application's log files. Sessions belong to
//! the caller that opened them.

use nyanpasu_core::client::NyanpasuClient;

use super::Result;
use crate::{Caller, State};

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn get_core_log(
    client: State<'_, NyanpasuClient>,
    cursor: nyanpasu_core::logs::CoreLogCursor,
) -> nyanpasu_core::logs::CoreLogResult<nyanpasu_core::logs::CoreLogRecord> {
    client.get_core_log(cursor).await
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn clear_core_logs(
    client: State<'_, NyanpasuClient>,
) -> nyanpasu_core::logs::CoreLogResult<()> {
    client.clear_core_logs().await
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn list_log_files(
    client: State<'_, NyanpasuClient>,
    source: nyanpasu_core::client::logs::LogSource,
) -> nyanpasu_logging::LogResult<Vec<nyanpasu_logging::LogFileInfo>> {
    client.list_log_files(source).await
}

#[nyanpasu_macro::rpc(http, result, owner)]
#[specta::specta]
pub async fn open_log_session(
    caller: Caller,
    client: State<'_, NyanpasuClient>,
    source: nyanpasu_core::client::logs::LogSource,
    request: nyanpasu_logging::OpenLogs,
) -> nyanpasu_logging::LogResult<nyanpasu_logging::LogSession> {
    client
        .open_log_session(source, caller.label().to_string(), request)
        .await
}

#[nyanpasu_macro::rpc(http, result, owner)]
#[specta::specta]
pub async fn query_logs(
    caller: Caller,
    client: State<'_, NyanpasuClient>,
    source: nyanpasu_core::client::logs::LogSource,
    request: nyanpasu_logging::QueryLogs,
) -> nyanpasu_logging::LogResult<nyanpasu_logging::LogPage> {
    client
        .query_logs(source, caller.label().to_string(), request)
        .await
}

#[nyanpasu_macro::rpc(http, result, owner)]
#[specta::specta]
pub async fn close_log_session(
    caller: Caller,
    client: State<'_, NyanpasuClient>,
    source: nyanpasu_core::client::logs::LogSource,
    session: String,
) -> nyanpasu_logging::LogResult<()> {
    client
        .close_log_session(source, caller.label().to_string(), session)
        .await
}

#[nyanpasu_macro::rpc(http, owner)]
#[specta::specta]
pub fn report_frontend_events(
    caller: Caller,
    client: State<'_, NyanpasuClient>,
    batch: nyanpasu_core::client::frontend_events::FrontendEventBatch,
) -> Result {
    client.report_frontend_events(caller.label(), batch);
    Ok(())
}
