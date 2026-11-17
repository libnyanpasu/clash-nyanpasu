//! OS integration: files, dialogs, the clipboard, notifications and shell actions.

use nyanpasu_core::client::NyanpasuClient;

use crate::{
    error::Result,
    ports::{AccentColorQuery, HostEnvironment},
    registry::RpcState,
};

// An empty deep-link notice; it never carries the link.
nyanpasu_rpc::resource!(event "scheme-request-received-event", targets = [desktop], capabilities(deep_link));

#[nyanpasu_rpc::rpc(capabilities(accent_color), query, expose = [http])]
#[specta::specta]
pub fn get_system_accent_color(
    accent: RpcState<'_, dyn AccentColorQuery>,
) -> Result<Option<String>> {
    Ok(accent.accent_color())
}

#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub fn is_appimage(environment: RpcState<'_, HostEnvironment>) -> Result<bool> {
    Ok(environment.is_appimage)
}

#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub fn is_portable(client: RpcState<'_, NyanpasuClient>) -> Result<bool> {
    Ok(client.is_portable())
}
