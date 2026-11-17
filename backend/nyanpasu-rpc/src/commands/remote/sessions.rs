//! Browser sessions: opening one from the desktop, listing and closing them.

use nyanpasu_contracts::auth::{BrowserSessionSummary, SessionId};
use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn list_remote_browser_sessions(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Vec<BrowserSessionSummary>> {
    Ok(client.list_remote_browser_sessions().await?)
}

#[nyanpasu_rpc::rpc]
#[specta::specta]
pub async fn revoke_remote_browser_session(
    client: RpcState<'_, NyanpasuClient>,
    id: SessionId,
) -> Result<()> {
    Ok(client.revoke_remote_browser_session(id).await?)
}

/// A URL that logs the caller into the running HTTP server. It is a credential, so it is a
/// desktop-only mutation: a query would keep it in a client cache, and no remote transport may
/// return it.
#[nyanpasu_rpc::rpc(capabilities(http_server), expose = [])]
#[specta::specta]
pub async fn create_http_launch_url(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<String>> {
    Ok(client.http_launch_url().await?)
}
