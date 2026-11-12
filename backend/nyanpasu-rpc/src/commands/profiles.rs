//! Shared profiles command adapters.

use nyanpasu_config::profile::{
    FolderId, NewProfileRequest, ProfileDefinition, ProfileId, ProfileLayout, ProfileMetadataPatch,
    Profiles as DomainProfiles, ProxyGroupLayout, RemoteProfileOptionsPatch, TransformKind,
};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_runtime_values::profiles::InvalidSubscriptionUrlSnafu;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_macro::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::GetProfiles)]
#[specta::specta]
pub async fn get_profiles(client: RpcState<'_, NyanpasuClient>) -> Result<DomainProfiles> {
    Ok((*client.get_profiles().await?).clone())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn is_portable(client: RpcState<'_, NyanpasuClient>) -> Result<bool> {
    Ok(client.is_portable())
}

/// Rebuild-only command: there is no prior state commit, so a failure is a
/// plain error — the committed/degraded model (spec §6.2) does not apply.
#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn enhance_profiles(client: RpcState<'_, NyanpasuClient>) -> Result {
    client.reconcile_core().await?;
    Ok(())
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn import_profile(
    client: RpcState<'_, NyanpasuClient>,
    url: String,
    name: Option<String>,
    option: Option<RemoteProfileOptionsPatch>,
    transform: Option<TransformKind>,
    folder: Option<FolderId>,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<ProfileId>> {
    let url = snafu::ResultExt::context(
        url::Url::parse(&url),
        InvalidSubscriptionUrlSnafu { url: &url },
    )?;
    // `name` carries deep-link intent (e.g. an install-config `name=` param);
    // when absent the facade derives the name from the url server-side. Return
    // MutationOutcome so a degraded post-import rebuild still carries the uid.
    Ok(client
        .import_profile(url, name, option, transform, folder)
        .await?)
}

/// create a new profile
#[nyanpasu_macro::rpc(
    local,
    http,
    operation = nyanpasu_rpc_protocol::operations::CreateProfile,
    optional_capabilities(elevation, os_integration, presentation)
)]
#[specta::specta]
pub async fn create_profile(
    client: RpcState<'_, NyanpasuClient>,
    request: NewProfileRequest,
    file_data: Option<String>,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<ProfileId>> {
    // Must return the created ProfileId; never drop it on the wire.
    Ok(client.create_profile(request, file_data).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn reorder_profile(
    client: RpcState<'_, NyanpasuClient>,
    active_id: ProfileId,
    over_id: ProfileId,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.reorder_profile(active_id, over_id).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn reorder_profiles_by_list(
    client: RpcState<'_, NyanpasuClient>,
    list: Vec<ProfileId>,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.reorder_profiles_by_list(list).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn set_profile_layout(
    client: RpcState<'_, NyanpasuClient>,
    layout: ProfileLayout,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.set_profile_layout(layout).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn set_proxy_group_layout(
    client: RpcState<'_, NyanpasuClient>,
    profile: ProfileId,
    layout: ProxyGroupLayout,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.set_proxy_group_layout(profile, layout).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn create_proxy_group_folder(
    client: RpcState<'_, NyanpasuClient>,
    profile: ProfileId,
    name: String,
    members: Vec<String>,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<FolderId>> {
    Ok(client
        .create_proxy_group_folder(profile, name, members)
        .await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn create_profile_folder(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
    members: Vec<ProfileId>,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<FolderId>> {
    Ok(client.create_profile_folder(name, members).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_profile_sync_status(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<nyanpasu_core::client::jobs::ProfileSyncStatus> {
    Ok(client.profile_sync_status(uid).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_profile_sync_runs(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    after: Option<nyanpasu_jobs::dto::RunCursorDto>,
) -> Result<nyanpasu_jobs::dto::RunPageDto> {
    Ok(client.profile_sync_runs(uid, after).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_profile_sync_logs(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    run: String,
    after: Option<String>,
) -> Result<nyanpasu_jobs::dto::LogPageDto> {
    Ok(client.profile_sync_logs(uid, run, after).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn update_profile(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.refresh_profile(uid, option).await?)
}

#[nyanpasu_macro::rpc(
    local,
    http,
    operation = nyanpasu_rpc_protocol::operations::DeleteProfile,
    optional_capabilities(elevation, os_integration, presentation)
)]
#[specta::specta]
pub async fn delete_profile(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.delete_profile(uid).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn activate_profile(
    client: RpcState<'_, NyanpasuClient>,
    uid: Option<ProfileId>,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.activate_profile(uid).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn set_global_transforms(
    client: RpcState<'_, NyanpasuClient>,
    ids: Vec<ProfileId>,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.set_global_transforms(ids).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn set_profile_valid_fields(
    client: RpcState<'_, NyanpasuClient>,
    fields: Vec<String>,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.set_profile_valid_fields(fields).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn patch_profile_metadata(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: ProfileMetadataPatch,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.patch_profile_metadata(uid, patch).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn patch_remote_profile_options(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: RemoteProfileOptionsPatch,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.patch_remote_profile_options(uid, patch).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn replace_profile_definition(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    definition: ProfileDefinition,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.replace_profile_definition(uid, definition).await?)
}

#[nyanpasu_macro::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::ReadProfileFile)]
#[specta::specta]
pub async fn read_profile_file(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<String> {
    Ok(client.read_profile_file(uid).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn save_profile_file(
    client: RpcState<'_, NyanpasuClient>,
    uid: ProfileId,
    file_data: String,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.save_profile_file(uid, file_data).await?)
}
