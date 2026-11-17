//! Ordinary application preferences.

use nyanpasu_config::application::{
    ApplicationPreferencesPatch, ApplicationSettings, NyanpasuAppConfig,
};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome;

use crate::{error::Result, registry::RpcState};

nyanpasu_rpc::resource!(event "state-changed-event", targets = [desktop, http]);

#[nyanpasu_rpc::rpc(query, expose = [http], sensitive)]
#[specta::specta]
pub async fn get_app_config(client: RpcState<'_, NyanpasuClient>) -> Result<NyanpasuAppConfig> {
    Ok(client.get_app_config().await?)
}

/// The application settings without the URLs that can carry credentials. The full config is
/// [`get_app_config`].
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn get_app_settings(client: RpcState<'_, NyanpasuClient>) -> Result<ApplicationSettings> {
    Ok(client.get_app_settings().await?)
}

/// Patch the ordinary application preferences.
#[nyanpasu_rpc::rpc(
    privacy,
    optional_capabilities(os_integration, presentation, elevation)
)]
#[specta::specta]
pub async fn patch_app_config(
    client: RpcState<'_, NyanpasuClient>,
    patch: ApplicationPreferencesPatch,
) -> Result<RpcUnitMutationOutcome> {
    // The request type holds only ordinary preferences: fields of another class are refused while
    // decoding, before this handler is admitted. Each class has its own command.
    Ok(client.patch_app_config(patch.into()).await?.into())
}
