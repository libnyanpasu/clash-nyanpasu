//! Traffic usage and its history.

use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn get_traffic_summary(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_traffic::TrafficSummary> {
    Ok(client.traffic_summary().await?)
}

#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub async fn query_traffic_report(
    client: RpcState<'_, NyanpasuClient>,
    request: nyanpasu_traffic::ReportRequest,
) -> Result<nyanpasu_traffic::TrafficReport> {
    Ok(client.query_traffic_report(request).await?)
}

#[nyanpasu_rpc::rpc(query, expose = [http])]
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

#[nyanpasu_rpc::rpc(query, expose = [http])]
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
