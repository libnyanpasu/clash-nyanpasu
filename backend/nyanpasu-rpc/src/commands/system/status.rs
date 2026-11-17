//! The configuration status and retrying effects that failed.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_runtime_values::observation::ConfigurationStatusSummary;

use crate::{error::Result, registry::RpcState};

nyanpasu_rpc::resource!(event "configuration-status-summary-changed", targets = [desktop, http]);

nyanpasu_rpc::resource!(event "configuration-status-changed", targets = [desktop, http, local], sensitive);

#[nyanpasu_rpc::rpc(operation = nyanpasu_rpc_protocol::operations::GetConfigurationStatus, query, sensitive)]
#[specta::specta]
pub fn get_configuration_status(
    client: RpcState<'_, NyanpasuClient>,
) -> nyanpasu_runtime_values::configuration::ConfigurationStatus {
    client.configuration_status()
}

/// Convergence health, retry state and operation and source IDs, without diagnostic text.
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub fn get_configuration_status_summary(
    client: RpcState<'_, NyanpasuClient>,
) -> ConfigurationStatusSummary {
    ConfigurationStatusSummary::from(&client.configuration_status())
}

#[nyanpasu_rpc::rpc(capabilities(http_server), query)]
#[specta::specta]
pub async fn get_http_server_status(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::http_server::HttpServerStatus> {
    Ok(client.http_server_status().await?)
}

#[nyanpasu_rpc::rpc(optional_capabilities(hotkey, os_integration, presentation), expose = [http])]
#[specta::specta]
pub fn retry_configuration_effect(
    client: RpcState<'_, NyanpasuClient>,
    kind: nyanpasu_runtime_values::configuration::EffectKind,
) -> Result<()> {
    Ok(client.retry_effect_now(kind)?)
}

#[nyanpasu_rpc::rpc(optional_capabilities(elevation, os_integration, presentation), expose = [http])]
#[specta::specta]
pub async fn retry_configuration_runtime(client: RpcState<'_, NyanpasuClient>) -> Result<()> {
    Ok(client.retry_runtime_now().await?)
}
