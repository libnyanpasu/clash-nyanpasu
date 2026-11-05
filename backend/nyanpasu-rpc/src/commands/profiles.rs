//! Profile import, ordering, layout, sync and runtime inspection beyond the
//! operations in the parent module.

use nyanpasu_config::profile::{
    FolderId, ProfileId, ProfileLayout, ProfileMetadataPatch, ProxyGroupLayout,
    RemoteProfileOptionsPatch, TransformKind,
};
use nyanpasu_core::{client::NyanpasuClient, state::profiles::InvalidSubscriptionUrlSnafu};

use super::Result;
use crate::State;

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn import_profile(
    client: State<'_, NyanpasuClient>,
    url: String,
    name: Option<String>,
    option: Option<RemoteProfileOptionsPatch>,
    transform: Option<TransformKind>,
    folder: Option<FolderId>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<ProfileId>> {
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

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn reorder_profiles_by_list(
    client: State<'_, NyanpasuClient>,
    list: Vec<ProfileId>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.reorder_profiles_by_list(list).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn set_profile_layout(
    client: State<'_, NyanpasuClient>,
    layout: ProfileLayout,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.set_profile_layout(layout).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn set_proxy_group_layout(
    client: State<'_, NyanpasuClient>,
    profile: ProfileId,
    layout: ProxyGroupLayout,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.set_proxy_group_layout(profile, layout).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn create_proxy_group_folder(
    client: State<'_, NyanpasuClient>,
    profile: ProfileId,
    name: String,
    members: Vec<String>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<FolderId>> {
    Ok(client
        .create_proxy_group_folder(profile, name, members)
        .await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn create_profile_folder(
    client: State<'_, NyanpasuClient>,
    name: String,
    members: Vec<ProfileId>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<FolderId>> {
    Ok(client.create_profile_folder(name, members).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_profile_sync_status(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<nyanpasu_core::client::jobs::ProfileSyncStatus> {
    Ok(client.profile_sync_status(uid).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_profile_sync_runs(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    after: Option<nyanpasu_jobs::dto::RunCursorDto>,
) -> Result<nyanpasu_jobs::dto::RunPageDto> {
    Ok(client.profile_sync_runs(uid, after).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_profile_sync_logs(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    run: String,
    after: Option<String>,
) -> Result<nyanpasu_jobs::dto::LogPageDto> {
    Ok(client.profile_sync_logs(uid, run, after).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn update_profile(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.refresh_profile(uid, option).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn set_profile_valid_fields(
    client: State<'_, NyanpasuClient>,
    fields: Vec<String>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.set_profile_valid_fields(fields).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn patch_profile_metadata(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: ProfileMetadataPatch,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.patch_profile_metadata(uid, patch).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn patch_remote_profile_options(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: RemoteProfileOptionsPatch,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.patch_remote_profile_options(uid, patch).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn inspect_applied_runtime(
    client: State<'_, NyanpasuClient>,
) -> Result<Option<nyanpasu_core::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_applied_runtime().await)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn inspect_runtime_node(
    client: State<'_, NyanpasuClient>,
    snapshot_id: String,
    node_id: u32,
) -> Result<nyanpasu_core::client::runtime_inspection::RuntimeInspectionContent> {
    Ok(client.inspect_runtime_node(&snapshot_id, node_id).await?)
}
