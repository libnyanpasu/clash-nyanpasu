//! Network automation and network probes.

use nyanpasu_config::application::IpLookupProviderInfo;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::types::RpcNetworkAutomationStatus;
use nyanpasu_runtime_values::network_automation::NetworkAutomationStatus;

use crate::{error::Result, registry::RpcState};

/// Applies the network rules to the current network again, overwriting
/// capture switches changed by hand since. Switching the capture mechanisms
/// reaches the same optional effects as a configuration patch. A redacted response holds the
/// status without the network reading and the matched rule.
#[nyanpasu_rpc::rpc(privacy, optional_capabilities(os_integration, presentation, elevation), expose = [])]
#[specta::specta]
pub async fn apply_network_automation(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<RpcNetworkAutomationStatus> {
    Ok(client.apply_network_automation().await.into())
}

#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub async fn get_ip_lookup_providers(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Vec<IpLookupProviderInfo>> {
    Ok(client.ip_lookup_providers())
}

/// The network the host is on and what the network rules last did about it.
/// Desktop-only: the status holds the live Wi-Fi name and gateway address.
#[nyanpasu_rpc::rpc(query, expose = [], sensitive)]
#[specta::specta]
pub async fn get_network_automation_status(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<NetworkAutomationStatus> {
    Ok(client.get_network_automation_status().await)
}

/// Reads the Wi-Fi name once, which the network rules do not until a rule
/// matches on it. Where the platform gates the name behind location access,
/// this asks the user for it. The status holds that reading, or why there is
/// none, except that a redacted response holds neither the name nor the gateway.
/// Desktop-only: the status holds the live Wi-Fi name and gateway.
#[nyanpasu_rpc::rpc(privacy, expose = [])]
#[specta::specta]
pub async fn request_wifi_name_access(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<RpcNetworkAutomationStatus> {
    Ok(client.request_wifi_name_access().await.into())
}
