//! Updating rule and proxy providers.

nyanpasu_rpc::namespace!(expose = [http]);

use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc]
#[specta::specta]
pub async fn clash_api_update_providers_rules(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_clash_rule_provider(name).await?)
}

#[nyanpasu_rpc::rpc(optional_capabilities(presentation))]
#[specta::specta]
pub async fn update_proxy_provider(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_proxy_provider(name).await?)
}
