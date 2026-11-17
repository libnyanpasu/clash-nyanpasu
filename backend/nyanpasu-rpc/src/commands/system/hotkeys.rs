//! Global hotkeys.

use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub fn get_hotkey_functions() -> Vec<&'static str> {
    nyanpasu_runtime_values::hotkey::HotkeyAction::all()
        .iter()
        .filter(|action| {
            cfg!(target_os = "linux")
                || !matches!(
                    action,
                    nyanpasu_runtime_values::hotkey::HotkeyAction::ToggleTransparentProxy
                        | nyanpasu_runtime_values::hotkey::HotkeyAction::EnableTransparentProxy
                        | nyanpasu_runtime_values::hotkey::HotkeyAction::DisableTransparentProxy
                )
        })
        .map(|action| action.as_str())
        .collect()
}

#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub async fn get_hotkeys(client: RpcState<'_, NyanpasuClient>) -> Result<Vec<String>> {
    Ok(client.get_app_config().await?.hotkeys)
}

// See nyanpasu_config::application::ApplicationPreferencesPatch for the command-scope and domain-split rule.
#[nyanpasu_rpc::rpc(optional_capabilities(hotkey), privacy, expose = [])]
#[specta::specta]
pub async fn set_hotkeys(
    client: RpcState<'_, NyanpasuClient>,
    hotkeys: Vec<String>,
) -> Result<nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome> {
    // An unparsable list is rejected before anything is written; a shortcut the
    // OS refuses lands as a degradation on a committed config.
    Ok(client
        .patch_app_config(nyanpasu_config::application::NyanpasuAppConfigPatch {
            hotkeys: Some(hotkeys),
            ..Default::default()
        })
        .await?
        .into())
}
