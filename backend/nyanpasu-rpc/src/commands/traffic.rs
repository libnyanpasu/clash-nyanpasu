//! Shared traffic command adapters.

use nyanpasu_contracts::logs::{CoreLogCursor, CoreLogStatus};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_observers::logs::{CoreLogPage, CoreLogQuery, CoreLogRecord, CoreLogResult};

use crate::{error::Result, registry::RpcState};

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_clash_ws_snapshot(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::ws::ClashWsSnapshot> {
    Ok(client.clash_ws_snapshot().await?)
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn query_core_logs(
    client: RpcState<'_, NyanpasuClient>,
    query: CoreLogQuery,
) -> CoreLogResult<CoreLogPage> {
    client.query_core_logs(query).await
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn get_core_log(
    client: RpcState<'_, NyanpasuClient>,
    cursor: CoreLogCursor,
) -> CoreLogResult<CoreLogRecord> {
    client.get_core_log(cursor).await
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn get_core_log_status(
    client: RpcState<'_, NyanpasuClient>,
) -> CoreLogResult<CoreLogStatus> {
    client.get_core_log_status().await
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn clear_core_logs(client: RpcState<'_, NyanpasuClient>) -> CoreLogResult<()> {
    client.clear_core_logs().await
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_traffic_summary(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_traffic::TrafficSummary> {
    Ok(client.traffic_summary().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn query_traffic_report(
    client: RpcState<'_, NyanpasuClient>,
    request: nyanpasu_traffic::ReportRequest,
) -> Result<nyanpasu_traffic::TrafficReport> {
    Ok(client.query_traffic_report(request).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn query_traffic_usage(
    client: RpcState<'_, NyanpasuClient>,
    query: nyanpasu_traffic::TrafficQuery,
    group_by: nyanpasu_traffic::Dimension,
    metric: nyanpasu_traffic::Metric,
    after: Option<nyanpasu_traffic::UsageCursor>,
    limit: usize,
) -> Result<nyanpasu_traffic::UsagePage> {
    Ok(client
        .query_traffic_usage(query, group_by, metric, after, limit)
        .await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn query_traffic_usage_by_keys(
    client: RpcState<'_, NyanpasuClient>,
    query: nyanpasu_traffic::TrafficQuery,
    group_by: nyanpasu_traffic::Dimension,
    keys: Vec<String>,
) -> Result<Vec<nyanpasu_traffic::UsageGroup>> {
    Ok(client
        .query_traffic_usage_by_keys(query, group_by, keys)
        .await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn query_traffic_closed_connections(
    client: RpcState<'_, NyanpasuClient>,
    range: nyanpasu_traffic::TrafficRange,
    filters: Vec<nyanpasu_traffic::TrafficFilter>,
    before: Option<nyanpasu_traffic::ClosedCursor>,
    limit: usize,
) -> Result<nyanpasu_traffic::ClosedPage> {
    Ok(client
        .query_traffic_closed_connections(range, filters, before, limit)
        .await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn query_traffic_active_connection_ids(
    client: RpcState<'_, NyanpasuClient>,
    filters: Vec<nyanpasu_traffic::TrafficFilter>,
) -> Result<Vec<String>> {
    Ok(client.query_traffic_active_connection_ids(filters).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn set_clash_ws_recording(
    client: RpcState<'_, NyanpasuClient>,
    kind: nyanpasu_core::clash::ws::ClashWsKind,
    enabled: bool,
) -> Result<nyanpasu_core::clash::ws::ClashWsRecording> {
    Ok(client.set_clash_ws_recording(kind, enabled).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clear_clash_ws_history(
    client: RpcState<'_, NyanpasuClient>,
    kind: nyanpasu_core::clash::ws::ClashWsKind,
) -> Result {
    client.clear_clash_ws_history(kind).await?;
    Ok(())
}
