//! Shared traffic command adapters.
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::wire::{
    logs::{CoreLogCursor, CoreLogPage, CoreLogQuery, CoreLogRecord, CoreLogResult, CoreLogStatus},
    streams::{ClashWsKind, ClashWsRecording, ClashWsSnapshot},
};

use crate::{
    error::Result,
    projection::{IntoApplication, IntoWire},
    registry::RpcState,
};

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_clash_ws_snapshot(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<ClashWsSnapshot> {
    Ok((client.clash_ws_snapshot().await?).into_wire())
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn query_core_logs(
    client: RpcState<'_, NyanpasuClient>,
    query: CoreLogQuery,
) -> CoreLogResult<CoreLogPage> {
    client
        .query_core_logs(query.into_application())
        .await
        .map(IntoWire::into_wire)
        .map_err(IntoWire::into_wire)
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn get_core_log(
    client: RpcState<'_, NyanpasuClient>,
    cursor: CoreLogCursor,
) -> CoreLogResult<CoreLogRecord> {
    client
        .get_core_log(cursor.into_application())
        .await
        .map(IntoWire::into_wire)
        .map_err(IntoWire::into_wire)
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn get_core_log_status(
    client: RpcState<'_, NyanpasuClient>,
) -> CoreLogResult<CoreLogStatus> {
    client
        .get_core_log_status()
        .await
        .map(IntoWire::into_wire)
        .map_err(IntoWire::into_wire)
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn clear_core_logs(client: RpcState<'_, NyanpasuClient>) -> CoreLogResult<()> {
    client.clear_core_logs().await.map_err(IntoWire::into_wire)
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
    kind: ClashWsKind,
    enabled: bool,
) -> Result<ClashWsRecording> {
    let kind = kind.into_application();
    Ok((client.set_clash_ws_recording(kind, enabled).await?).into_wire())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clear_clash_ws_history(
    client: RpcState<'_, NyanpasuClient>,
    kind: ClashWsKind,
) -> Result {
    let kind = kind.into_application();
    client.clear_clash_ws_history(kind).await?;
    Ok(())
}
