//! Shared profiles command adapters.

use nyanpasu_config::profile::{
    FolderId, NewProfileRequest, ProfileDefinition, ProfileId, ProfileLayout, ProfileMetadataPatch,
    Profiles as DomainProfiles, ProfilesListing, ProxyGroupLayout, RemoteProfileOptionsPatch,
    TransformKind,
};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::types::{
    RpcFolderMutationOutcome, RpcProfileMutationOutcome, RpcUnitMutationOutcome,
};
use nyanpasu_runtime_values::profiles::InvalidSubscriptionUrlSnafu;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::GetProfiles, query, scope = sensitive_read)]
#[specta::specta]
pub async fn get_profiles(client: RpcState<'_, NyanpasuClient>) -> Result<DomainProfiles> {
    Ok((*client.get_profiles().await?).clone())
}

/// The profile structure without subscription URLs, request headers, external paths or
/// descriptions. The full document is [`get_profiles`].
#[nyanpasu_rpc::rpc(local, http, query)]
#[specta::specta]
pub fn get_profiles_listing(client: RpcState<'_, NyanpasuClient>) -> ProfilesListing {
    client.get_profiles_listing()
}

#[nyanpasu_rpc::rpc(http, query)]
#[specta::specta]
pub fn is_portable(client: RpcState<'_, NyanpasuClient>) -> Result<bool> {
    Ok(client.is_portable())
}

/// Rebuild-only command: there is no prior state commit, so a failure is a
/// plain error — the committed/degraded model (spec §6.2) does not apply.
#[nyanpasu_rpc::rpc(http, optional_capabilities(elevation, os_integration, presentation), scope = profile)]
#[specta::specta]
pub async fn enhance_profiles(client: RpcState<'_, NyanpasuClient>) -> Result {
    client.reconcile_core().await?;
    Ok(())
}

#[nyanpasu_rpc::rpc(local, http, privacy, optional_capabilities(elevation, os_integration, presentation), scope = profile)]
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

/// create a new profile
#[nyanpasu_rpc::rpc(local,
    http,
    privacy,
    operation = nyanpasu_rpc_protocol::operations::CreateProfile,
    optional_capabilities(elevation, os_integration, presentation), scope = profile)]
#[specta::specta]
pub async fn create_profile(
    client: RpcState<'_, NyanpasuClient>,
    request: NewProfileRequest,
    file_data: Option<String>,
) -> Result<RpcProfileMutationOutcome> {
    // Must return the created ProfileId; never drop it on the wire.
    Ok(client.create_profile(request, file_data).await?.into())
}

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(presentation), scope = profile)]
#[specta::specta]
pub async fn reorder_profile(
    client: RpcState<'_, NyanpasuClient>,
    active_id: ProfileId,
    over_id: ProfileId,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.reorder_profile(active_id, over_id).await?.into())
}

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(presentation), scope = profile)]
#[specta::specta]
pub async fn reorder_profiles_by_list(
    client: RpcState<'_, NyanpasuClient>,
    list: Vec<ProfileId>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.reorder_profiles_by_list(list).await?.into())
}

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(presentation), scope = profile)]
#[specta::specta]
pub async fn set_profile_layout(
    client: RpcState<'_, NyanpasuClient>,
    layout: ProfileLayout,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_profile_layout(layout).await?.into())
}

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(presentation), scope = profile)]
#[specta::specta]
pub async fn set_proxy_group_layout(
    client: RpcState<'_, NyanpasuClient>,
    profile: ProfileId,
    layout: ProxyGroupLayout,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_proxy_group_layout(profile, layout).await?.into())
}

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(presentation), scope = profile)]
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

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(presentation), scope = profile)]
#[specta::specta]
pub async fn create_profile_folder(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
    members: Vec<ProfileId>,
) -> Result<RpcFolderMutationOutcome> {
    Ok(client.create_profile_folder(name, members).await?.into())
}

// Run logs and errors carry the failing request's URL, which can hold a subscription token.
#[nyanpasu_rpc::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn get_profile_sync_status(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<nyanpasu_core::client::jobs::ProfileSyncStatus> {
    Ok(client.profile_sync_status(uid).await?)
}

/// The sync schedule and active runs without the registration error text. The full status is
/// [`get_profile_sync_status`].
#[nyanpasu_rpc::rpc(http, query)]
#[specta::specta]
pub async fn get_profile_sync_status_summary(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<nyanpasu_core::client::jobs::ProfileSyncSummary> {
    Ok(client.profile_sync_status_summary(uid).await?)
}

/// One page of sync runs with each outcome reduced to its kind. The full page is
/// [`get_profile_sync_runs`].
#[nyanpasu_rpc::rpc(http, query)]
#[specta::specta]
pub async fn get_profile_sync_runs_summary(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    after: Option<nyanpasu_jobs::dto::RunCursorDto>,
) -> Result<nyanpasu_core::client::jobs::SyncRunPageSummary> {
    Ok(client.profile_sync_runs_summary(uid, after).await?)
}

#[nyanpasu_rpc::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn get_profile_sync_runs(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    after: Option<nyanpasu_jobs::dto::RunCursorDto>,
) -> Result<nyanpasu_jobs::dto::RunPageDto> {
    Ok(client.profile_sync_runs(uid, after).await?)
}

#[nyanpasu_rpc::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn get_profile_sync_logs(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    run: String,
    after: Option<String>,
) -> Result<nyanpasu_jobs::dto::LogPageDto> {
    Ok(client.profile_sync_logs(uid, run, after).await?)
}

#[nyanpasu_rpc::rpc(local, http, privacy, optional_capabilities(elevation, os_integration, presentation), scope = profile)]
#[specta::specta]
pub async fn update_profile(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.refresh_profile(uid, option).await?.into())
}

#[nyanpasu_rpc::rpc(local,
    http,
    privacy,
    operation = nyanpasu_rpc_protocol::operations::DeleteProfile,
    optional_capabilities(elevation, os_integration, presentation), scope = profile)]
#[specta::specta]
pub async fn delete_profile(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.delete_profile(uid).await?.into())
}

#[nyanpasu_rpc::rpc(local, http, privacy, optional_capabilities(elevation, os_integration, presentation), scope = profile)]
#[specta::specta]
pub async fn activate_profile(
    client: RpcState<'_, NyanpasuClient>,
    uid: Option<ProfileId>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.activate_profile(uid).await?.into())
}

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(elevation, os_integration, presentation), scope = profile)]
#[specta::specta]
pub async fn set_global_transforms(
    client: RpcState<'_, NyanpasuClient>,
    ids: Vec<ProfileId>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_global_transforms(ids).await?.into())
}

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(elevation, os_integration, presentation), scope = profile)]
#[specta::specta]
pub async fn set_profile_valid_fields(
    client: RpcState<'_, NyanpasuClient>,
    fields: Vec<String>,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_profile_valid_fields(fields).await?.into())
}

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(presentation), scope = profile)]
#[specta::specta]
pub async fn patch_profile_metadata(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: ProfileMetadataPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.patch_profile_metadata(uid, patch).await?.into())
}

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(elevation, os_integration, presentation), scope = profile)]
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

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(elevation, os_integration, presentation), scope = profile)]
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

#[nyanpasu_rpc::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::ReadProfileFile, query, scope = sensitive_read)]
#[specta::specta]
pub async fn read_profile_file(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<String> {
    Ok(client.read_profile_file(uid).await?)
}

#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(elevation, os_integration, presentation), scope = profile)]
#[specta::specta]
pub async fn save_profile_file(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    file_data: String,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.save_profile_file(uid, file_data).await?.into())
}
