//! Core logs.

use nyanpasu_contracts::logs::{CoreLogCursor, CoreLogStatus, CoreLogStatusSummary};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_observers::logs::{
    CoreLogPage, CoreLogPageSummary, CoreLogQuery, CoreLogRecord, CoreLogResult,
    CoreLogSummaryQuery,
};

use crate::{error::Result, registry::RpcState};

nyanpasu_rpc::resource!(event "core-logs-summary-changed", targets = [desktop, http]);

nyanpasu_rpc::resource!(event "core-logs-changed", targets = [desktop, http, local], sensitive);

#[nyanpasu_rpc::rpc(result, expose = [http])]
#[specta::specta]
pub async fn clear_core_logs(
    client: RpcState<'_, NyanpasuClient>,
) -> std::result::Result<(), crate::error::CoreLogCommandError> {
    client.clear_core_logs().await.map_err(Into::into)
}

#[nyanpasu_rpc::rpc(result, query, expose = [http], sensitive)]
#[specta::specta]
pub async fn get_core_log(
    client: RpcState<'_, NyanpasuClient>,
    cursor: CoreLogCursor,
) -> CoreLogResult<CoreLogRecord> {
    client.get_core_log(cursor).await
}

#[nyanpasu_rpc::rpc(result, query, expose = [http], sensitive)]
#[specta::specta]
pub async fn get_core_log_status(
    client: RpcState<'_, NyanpasuClient>,
) -> CoreLogResult<CoreLogStatus> {
    client.get_core_log_status().await
}

/// Public core-log status summary without raw storage error messages.
#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub async fn get_core_log_status_summary(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<CoreLogStatusSummary> {
    Ok(client.get_core_log_status_summary().await?)
}

#[nyanpasu_rpc::rpc(result, query, sensitive)]
#[specta::specta]
pub async fn query_core_logs(
    client: RpcState<'_, NyanpasuClient>,
    query: CoreLogQuery,
) -> CoreLogResult<CoreLogPage> {
    client.query_core_logs(query).await
}

/// Public core-log page summary without payloads, source labels, raw timestamps, keyword search, or storage error text.
#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub async fn query_core_logs_summary(
    client: RpcState<'_, NyanpasuClient>,
    query: CoreLogSummaryQuery,
) -> Result<CoreLogPageSummary> {
    Ok(client.query_core_logs_summary(query).await?)
}
