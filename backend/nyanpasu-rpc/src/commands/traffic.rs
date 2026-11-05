//! The live Clash streams and the recorded traffic history.

use nyanpasu_core::client::NyanpasuClient;

use super::Result;
use crate::State;

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_clash_ws_snapshot(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::ws::ClashWsSnapshot> {
    Ok(client.clash_ws_snapshot().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn set_clash_ws_recording(
    client: State<'_, NyanpasuClient>,
    kind: nyanpasu_core::clash::ws::ClashWsKind,
    enabled: bool,
) -> Result<nyanpasu_core::clash::ws::ClashWsRecording> {
    Ok(client.set_clash_ws_recording(kind, enabled).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clear_clash_ws_history(
    client: State<'_, NyanpasuClient>,
    kind: nyanpasu_core::clash::ws::ClashWsKind,
) -> Result {
    client.clear_clash_ws_history(kind).await?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_traffic_summary(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_traffic::TrafficSummary> {
    Ok(client.traffic_summary().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn query_traffic_report(
    client: State<'_, NyanpasuClient>,
    request: nyanpasu_traffic::ReportRequest,
) -> Result<nyanpasu_traffic::TrafficReport> {
    Ok(client.query_traffic_report(request).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn query_traffic_usage(
    client: State<'_, NyanpasuClient>,
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
    client: State<'_, NyanpasuClient>,
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
    client: State<'_, NyanpasuClient>,
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
    client: State<'_, NyanpasuClient>,
    filters: Vec<nyanpasu_traffic::TrafficFilter>,
) -> Result<Vec<String>> {
    Ok(client.query_traffic_active_connection_ids(filters).await?)
}
