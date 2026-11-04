//! Shared application commands: thin adapters over [`NyanpasuClient`] that
//! need no shell context, so every shell registers them and the registry
//! serves them on each target they opt into.
//!
//! Their doc comments and signatures are exported into the generated
//! frontend bindings by the desktop shell.

use nyanpasu_config::profile::{ProfileDefinition, ProfileId, Profiles as DomainProfiles};
use nyanpasu_core::{client::NyanpasuClient, state::profiles::actor::NewProfileRequest};

use crate::{IpcError, State};

type Result<T = ()> = std::result::Result<T, IpcError>;

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_profiles(client: State<'_, NyanpasuClient>) -> Result<DomainProfiles> {
    Ok((*client.get_profiles().await?).clone())
}

/// create a new profile
#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn create_profile(
    client: State<'_, NyanpasuClient>,
    request: NewProfileRequest,
    file_data: Option<String>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<ProfileId>> {
    // Must return the created ProfileId; never drop it on the wire.
    Ok(client.create_profile(request, file_data).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn delete_profile(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.delete_profile(uid).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn activate_profile(
    client: State<'_, NyanpasuClient>,
    uid: Option<ProfileId>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.activate_profile(uid).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn set_global_transforms(
    client: State<'_, NyanpasuClient>,
    ids: Vec<ProfileId>,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.set_global_transforms(ids).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn replace_profile_definition(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    definition: ProfileDefinition,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.replace_profile_definition(uid, definition).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn read_profile_file(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<String> {
    Ok(client.read_profile_file(uid).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn save_profile_file(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    file_data: String,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.save_profile_file(uid, file_data).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_runtime_yaml(client: State<'_, NyanpasuClient>) -> Result<String> {
    Ok(client.runtime_yaml().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn inspect_runtime(
    client: State<'_, NyanpasuClient>,
) -> Result<Option<nyanpasu_core::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_runtime().await)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_core_status(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::actor_v2::CoreStatusInfo> {
    Ok(client.core_status().into())
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn query_core_logs(
    client: State<'_, NyanpasuClient>,
    query: nyanpasu_core::logs::CoreLogQuery,
) -> nyanpasu_core::logs::CoreLogResult<nyanpasu_core::logs::CoreLogPage> {
    client.query_core_logs(query).await
}

#[nyanpasu_macro::rpc(http, result)]
#[specta::specta]
pub async fn get_core_log_status(
    client: State<'_, NyanpasuClient>,
) -> nyanpasu_core::logs::CoreLogResult<nyanpasu_core::logs::CoreLogStatus> {
    client.get_core_log_status().await
}
