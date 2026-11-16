//! Remote-access administration commands.
//!
//! They are reachable from the desktop shell and the same-user local socket, never over HTTP:
//! `remote_admin` is not a scope any bearer token or browser session can hold.

use nyanpasu_contracts::auth::{
    AccessTokenSummary, BrowserSessionSummary,
    CreateAccessTokenRequest as CoreCreateAccessTokenRequest, ScopeSet, SessionId, TokenId,
    TokenName,
};
use nyanpasu_core::client::NyanpasuClient;
use serde::{Deserialize, Serialize};

use crate::{error::Result, registry::RpcState};

/// Request payload for minting a named remote-access bearer token.
#[derive(Debug, Clone, Deserialize, specta::Type)]
pub struct CreateRemoteAccessTokenRequest {
    pub name: TokenName,
    pub scopes: ScopeSet,
    #[serde(default)]
    #[specta(optional)]
    pub expires_at_ms: Option<i64>,
}

/// Newly minted access token returned once at creation time.
///
/// Subsequent `list_remote_access_tokens` calls return only non-secret metadata.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct CreatedRemoteAccessToken {
    pub summary: AccessTokenSummary,
    pub token: String,
}

#[nyanpasu_macro::rpc(local, query, permission = "remote_admin.token:list")]
#[specta::specta]
pub async fn list_remote_access_tokens(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Vec<AccessTokenSummary>> {
    Ok(client.list_remote_access_tokens().await?)
}

#[nyanpasu_macro::rpc(local, permission = "remote_admin.token:create")]
#[specta::specta]
pub async fn create_remote_access_token(
    client: RpcState<'_, NyanpasuClient>,
    request: CreateRemoteAccessTokenRequest,
) -> Result<CreatedRemoteAccessToken> {
    let issued = client
        .create_remote_access_token(CoreCreateAccessTokenRequest {
            name: request.name,
            scopes: request.scopes,
            expires_at_ms: request.expires_at_ms,
        })
        .await?;
    Ok(CreatedRemoteAccessToken {
        summary: issued.summary,
        token: issued.token.expose_secret().to_owned(),
    })
}

#[nyanpasu_macro::rpc(local, permission = "remote_admin.token:revoke")]
#[specta::specta]
pub async fn revoke_remote_access_token(
    client: RpcState<'_, NyanpasuClient>,
    id: TokenId,
) -> Result<()> {
    Ok(client.revoke_remote_access_token(id).await?)
}

#[nyanpasu_macro::rpc(local, query, permission = "remote_admin.browser_session:list")]
#[specta::specta]
pub async fn list_remote_browser_sessions(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Vec<BrowserSessionSummary>> {
    Ok(client.list_remote_browser_sessions().await?)
}

#[nyanpasu_macro::rpc(local, permission = "remote_admin.browser_session:revoke")]
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
#[nyanpasu_macro::rpc(
    capabilities(http_server),
    permission = "remote_admin.http_launch_url:create"
)]
#[specta::specta]
pub async fn create_http_launch_url(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<String>> {
    Ok(client.http_launch_url().await?)
}
