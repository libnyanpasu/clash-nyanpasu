//! Connection lists, connection details and their history.

use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

nyanpasu_rpc::resource!(stream "connection_details", targets = [http, local]);

nyanpasu_rpc::resource!(event "clash-ws-event", targets = [desktop, http, local]);

#[nyanpasu_rpc::rpc(expose = [http])]
#[specta::specta]
pub async fn clear_clash_ws_history(
    client: RpcState<'_, NyanpasuClient>,
    kind: nyanpasu_core::clash::ws::ClashWsKind,
) -> Result {
    client.clear_clash_ws_history(kind).await?;
    Ok(())
}

#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub async fn get_clash_ws_snapshot(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::ws::ClashWsSnapshot> {
    Ok(client.clash_ws_snapshot().await?)
}

#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn query_traffic_active_connection_ids(
    client: RpcState<'_, NyanpasuClient>,
    filters: Vec<nyanpasu_traffic::TrafficFilter>,
) -> Result<Vec<String>> {
    Ok(client.query_traffic_active_connection_ids(filters).await?)
}

#[nyanpasu_rpc::rpc(query, expose = [http])]
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

#[nyanpasu_rpc::rpc(expose = [http])]
#[specta::specta]
pub async fn set_clash_ws_recording(
    client: RpcState<'_, NyanpasuClient>,
    kind: nyanpasu_core::clash::ws::ClashWsKind,
    enabled: bool,
) -> Result<nyanpasu_core::clash::ws::ClashWsRecording> {
    Ok(client.set_clash_ws_recording(kind, enabled).await?)
}
