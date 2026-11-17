//! The rules and rule providers of the running configuration.

nyanpasu_rpc::namespace!(expose = [http]);

use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn clash_api_get_providers_rules(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ProvidersRulesRes> {
    Ok(client.clash_rule_providers().await?)
}

#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn clash_api_get_rules(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::RulesRes> {
    Ok(client.clash_rules().await?)
}
