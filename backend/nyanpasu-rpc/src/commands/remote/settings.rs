//! The remote-access settings, the HTTPS certificate and the local socket's state.

use nyanpasu_config::application::{LocalNetwork, RemoteAccessConfig};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome;

use crate::{error::Result, registry::RpcState};

/// Remote access widens who can reach the application, so it has its own command, reachable from
/// the desktop and the same-user local socket but never over HTTP; `patch_app_config` rejects the
/// section.
#[nyanpasu_rpc::rpc(privacy, capabilities(http_server))]
#[specta::specta]
pub async fn set_remote_access(
    client: RpcState<'_, NyanpasuClient>,
    config: RemoteAccessConfig,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_remote_access(config).await?.into())
}

/// Replaces the HTTPS certificate and makes the listener present the new one. Clients that pinned
/// the old fingerprint refuse the server until they pin the new one, so it is a remote
/// administration command, never reachable over HTTP.
#[nyanpasu_rpc::rpc(privacy, capabilities(http_server))]
#[specta::specta]
pub async fn reset_remote_access_tls(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.reset_remote_access_tls().await?.into())
}

/// The host's IPv4 networks the LAN listen scope may allow, for the settings page. Only the
/// desktop offers it: the local socket allowlist is reviewed separately.
#[nyanpasu_rpc::rpc(query, expose = [])]
#[specta::specta]
pub async fn list_local_networks(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Vec<LocalNetwork>> {
    Ok(client.local_networks().await)
}

/// The local socket's state. Its endpoint is a filesystem path, so only a remote administrator
/// reads it, and it is never offered over HTTP.
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn get_local_socket_status(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::local_socket::LocalSocketStatus> {
    Ok(client.local_socket_status().await?)
}
