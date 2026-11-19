//! Traffic usage and its history.

use nyanpasu_core::{
    clash::meow_traffic::{MeowStatistics, MeowTrafficLedger, MeowTrafficLedgerQuery},
    client::NyanpasuClient,
};

use crate::{error::Result, registry::RpcState};

/// Session totals and named proxy counters from the Meow controller.
#[nyanpasu_rpc::rpc(query, expose = [], sensitive)]
#[specta::specta]
pub async fn get_meow_statistics(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<MeowStatistics>> {
    Ok(client.meow_statistics().await?)
}

/// The engine's persisted traffic ledger can include host and app names.
#[nyanpasu_rpc::rpc(query, expose = [], sensitive)]
#[specta::specta]
pub async fn query_meow_traffic_ledger(
    client: RpcState<'_, NyanpasuClient>,
    query: MeowTrafficLedgerQuery,
) -> Result<Option<MeowTrafficLedger>> {
    Ok(client.meow_traffic_ledger(query).await?)
}

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
