//! The profile store: listing, importing, creating, updating, activating and deleting
//! profiles.

use nyanpasu_config::profile::{
    FolderId, NewProfileRequest, ProfileDefinition, ProfileId, ProfileMetadataPatch,
    Profiles as DomainProfiles, ProfilesListing, RemoteProfileOptionsPatch, TransformKind,
};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::types::{RpcProfileMutationOutcome, RpcUnitMutationOutcome};
use nyanpasu_runtime_values::profiles::InvalidSubscriptionUrlSnafu;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(
    privacy,
    optional_capabilities(elevation, os_integration, presentation)
)]
#[specta::specta]
pub async fn activate_profile(
    client: RpcState<'_, NyanpasuClient>,
    uid: Option<ProfileId>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.activate_profile(uid).await?.into())
}

/// create a new profile
#[nyanpasu_rpc::rpc(privacy, operation = nyanpasu_rpc_protocol::operations::CreateProfile, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn create_profile(
    client: RpcState<'_, NyanpasuClient>,
    request: NewProfileRequest,
    file_data: Option<String>,
) -> Result<RpcProfileMutationOutcome> {
    // Must return the created ProfileId; never drop it on the wire.
    Ok(client.create_profile(request, file_data).await?.into())
}

#[nyanpasu_rpc::rpc(privacy, operation = nyanpasu_rpc_protocol::operations::DeleteProfile, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn delete_profile(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.delete_profile(uid).await?.into())
}

/// Rebuild-only command: there is no prior state commit, so a failure is a
/// plain error — the committed/degraded model (spec §6.2) does not apply.
#[nyanpasu_rpc::rpc(optional_capabilities(elevation, os_integration, presentation), expose = [http])]
#[specta::specta]
pub async fn enhance_profiles(client: RpcState<'_, NyanpasuClient>) -> Result {
    client.reconcile_core().await?;
    Ok(())
}

#[nyanpasu_rpc::rpc(operation = nyanpasu_rpc_protocol::operations::GetProfiles, query, sensitive)]
#[specta::specta]
pub async fn get_profiles(client: RpcState<'_, NyanpasuClient>) -> Result<DomainProfiles> {
    Ok((*client.get_profiles().await?).clone())
}

/// The profile structure without subscription URLs, request headers, external paths or
/// descriptions. The full document is [`get_profiles`].
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub fn get_profiles_listing(client: RpcState<'_, NyanpasuClient>) -> ProfilesListing {
    client.get_profiles_listing()
}

#[nyanpasu_rpc::rpc(
    privacy,
    optional_capabilities(elevation, os_integration, presentation)
)]
#[specta::specta]
pub async fn import_profile(
    client: RpcState<'_, NyanpasuClient>,
    url: String,
    name: Option<String>,
    option: Option<RemoteProfileOptionsPatch>,
    transform: Option<TransformKind>,
    folder: Option<FolderId>,
) -> Result<RpcProfileMutationOutcome> {
    let url = snafu::ResultExt::context(
        url::Url::parse(&url),
        InvalidSubscriptionUrlSnafu { url: &url },
    )?;
    // `name` carries deep-link intent (e.g. an install-config `name=` param);
    // when absent the facade derives the name from the url server-side. Return
    // MutationOutcome so a degraded post-import rebuild still carries the uid.
    Ok(client
        .import_profile(url, name, option, transform, folder)
        .await?
        .into())
}

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(presentation), expose = [http])]
#[specta::specta]
pub async fn patch_profile_metadata(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: ProfileMetadataPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.patch_profile_metadata(uid, patch).await?.into())
}

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
#[specta::specta]
pub async fn patch_remote_profile_options(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: RemoteProfileOptionsPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client
        .patch_remote_profile_options(uid, patch)
        .await?
        .into())
}

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(presentation), expose = [http])]
#[specta::specta]
pub async fn reorder_profile(
    client: RpcState<'_, NyanpasuClient>,
    active_id: ProfileId,
    over_id: ProfileId,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.reorder_profile(active_id, over_id).await?.into())
}

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(presentation), expose = [http])]
#[specta::specta]
pub async fn reorder_profiles_by_list(
    client: RpcState<'_, NyanpasuClient>,
    list: Vec<ProfileId>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.reorder_profiles_by_list(list).await?.into())
}

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
#[specta::specta]
pub async fn replace_profile_definition(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    definition: ProfileDefinition,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client
        .replace_profile_definition(uid, definition)
        .await?
        .into())
}

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
#[specta::specta]
pub async fn set_global_transforms(
    client: RpcState<'_, NyanpasuClient>,
    ids: Vec<ProfileId>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_global_transforms(ids).await?.into())
}

#[nyanpasu_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
#[specta::specta]
pub async fn set_profile_valid_fields(
    client: RpcState<'_, NyanpasuClient>,
    fields: Vec<String>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_profile_valid_fields(fields).await?.into())
}

#[nyanpasu_rpc::rpc(
    privacy,
    optional_capabilities(elevation, os_integration, presentation)
)]
#[specta::specta]
pub async fn update_profile(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.refresh_profile(uid, option).await?.into())
}
