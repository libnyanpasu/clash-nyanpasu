//! Network preferences and their probes.

use nyanpasu_contracts::net::{DirectEgress, RouteEgress};
use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(expose = [http])]
#[specta::specta]
pub async fn probe_direct_egress(client: RpcState<'_, NyanpasuClient>) -> Result<DirectEgress> {
    Ok(client.probe_direct_egress().await?)
}

#[nyanpasu_rpc::rpc(expose = [http])]
#[specta::specta]
pub async fn probe_route_egress(client: RpcState<'_, NyanpasuClient>) -> Result<RouteEgress> {
    Ok(client.probe_route_egress().await?)
}
