//! The running core's resolver: a lookup through it and flushing its caches. They act through
//! the core's own controller, need no elevation and fetch no URL a caller supplies.

nyanpasu_rpc::namespace!(expose = [http]);

use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

/// Resolves `name` through the running core's resolver.
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn clash_api_query_dns(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
    record_type: String,
) -> Result<clash_api::DnsResponse> {
    Ok(client.query_core_dns(name, record_type).await?)
}

#[nyanpasu_rpc::rpc]
#[specta::specta]
pub async fn clash_api_flush_fake_ip_cache(client: RpcState<'_, NyanpasuClient>) -> Result<()> {
    Ok(client.flush_core_fake_ip_cache().await?)
}

#[nyanpasu_rpc::rpc]
#[specta::specta]
pub async fn clash_api_flush_dns_cache(client: RpcState<'_, NyanpasuClient>) -> Result<()> {
    Ok(client.flush_core_dns_cache().await?)
}
