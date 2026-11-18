//! Profile and proxy group layouts and their folders.

nyanpasu_rpc::namespace!(expose = [http]);

use nyanpasu_config::profile::{ProfileId, ProfileLayout, ProxyGroupLayout};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::types::{RpcFolderMutationOutcome, RpcUnitMutationOutcome};

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(presentation))]
#[specta::specta]
pub async fn create_profile_folder(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
    members: Vec<ProfileId>,
) -> Result<RpcFolderMutationOutcome> {
    Ok(client.create_profile_folder(name, members).await?.into())
}

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(presentation))]
#[specta::specta]
pub async fn create_proxy_group_folder(
    client: RpcState<'_, NyanpasuClient>,
    profile: ProfileId,
    name: String,
    members: Vec<String>,
) -> Result<RpcFolderMutationOutcome> {
    Ok(client
        .create_proxy_group_folder(profile, name, members)
        .await?
        .into())
}

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(presentation))]
#[specta::specta]
pub async fn set_profile_layout(
    client: RpcState<'_, NyanpasuClient>,
    layout: ProfileLayout,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_profile_layout(layout).await?.into())
}

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(presentation))]
#[specta::specta]
pub async fn set_proxy_group_layout(
    client: RpcState<'_, NyanpasuClient>,
    profile: ProfileId,
    layout: ProxyGroupLayout,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_proxy_group_layout(profile, layout).await?.into())
}
