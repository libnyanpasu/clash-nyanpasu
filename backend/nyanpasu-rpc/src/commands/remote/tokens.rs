//! Named access tokens for HTTP callers.

use nyanpasu_contracts::auth::{
    AccessTokenSummary, CreateAccessTokenRequest as CoreCreateAccessTokenRequest, Grant, TokenId,
    TokenName,
};
use nyanpasu_core::client::NyanpasuClient;
use serde::{Deserialize, Serialize};

use crate::{error::Result, registry::RpcState};

/// Request payload for minting a named remote-access bearer token.
#[derive(Debug, Clone, Deserialize, specta::Type)]
pub struct CreateRemoteAccessTokenRequest {
    pub name: TokenName,
    /// A role (`viewer`, `operator` or `admin`) or a custom permission map without `remote`.
    pub grant: Grant,
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

#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn list_remote_access_tokens(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Vec<AccessTokenSummary>> {
    Ok(client.list_remote_access_tokens().await?)
}

#[nyanpasu_rpc::rpc]
#[specta::specta]
pub async fn create_remote_access_token(
    client: RpcState<'_, NyanpasuClient>,
    request: CreateRemoteAccessTokenRequest,
) -> Result<CreatedRemoteAccessToken> {
    let issued = client
        .create_remote_access_token(CoreCreateAccessTokenRequest {
            name: request.name,
            grant: request.grant,
            expires_at_ms: request.expires_at_ms,
        })
        .await?;
    Ok(CreatedRemoteAccessToken {
        summary: issued.summary,
        token: issued.token.expose_secret().to_owned(),
    })
}

#[nyanpasu_rpc::rpc]
#[specta::specta]
pub async fn revoke_remote_access_token(
    client: RpcState<'_, NyanpasuClient>,
    id: TokenId,
) -> Result<()> {
    Ok(client.revoke_remote_access_token(id).await?)
}
