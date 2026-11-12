//! Shared icons command adapters.

use crate::error::Result;

use base64::{Engine as _, prelude::BASE64_STANDARD};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc_protocol::values::TrayIcon;

use crate::{ports::HostIconFiles, registry::RpcState};

#[nyanpasu_macro::rpc(http, capabilities(presentation))]
#[specta::specta]
pub async fn set_tray_icon_from_bytes(
    client: RpcState<'_, NyanpasuClient>,
    icons: RpcState<'_, dyn HostIconFiles>,
    paths: RpcState<'_, PathResolver>,
    mode: TrayIcon,
    bytes_base64: String,
) -> Result {
    let bytes = decode_icon_upload(&bytes_base64, "Tray")?;
    icons.set_tray_icon_from_bytes(paths, mode, &bytes)?;
    client.request_tray_refresh()?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn is_tray_icon_set(
    icons: RpcState<'_, dyn HostIconFiles>,
    paths: RpcState<'_, PathResolver>,
    mode: TrayIcon,
) -> Result<bool> {
    Ok(icons.is_tray_icon_set(paths, mode).await)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn is_widget_icon_set(
    icons: RpcState<'_, dyn HostIconFiles>,
    paths: RpcState<'_, PathResolver>,
    mode: TrayIcon,
) -> Result<bool> {
    Ok(icons.is_widget_icon_set(paths, mode))
}

#[nyanpasu_macro::rpc(http, capabilities(presentation))]
#[specta::specta]
pub async fn set_widget_icon_from_bytes(
    client: RpcState<'_, NyanpasuClient>,
    icons: RpcState<'_, dyn HostIconFiles>,
    paths: RpcState<'_, PathResolver>,
    mode: TrayIcon,
    bytes_base64: String,
) -> Result {
    let bytes = decode_icon_upload(&bytes_base64, "Widget")?;
    icons.set_widget_icon_from_bytes(paths, mode, &bytes)?;
    client.request_widget_logo_refresh()?;
    Ok(())
}

/// Decodes an uploaded icon of at most 1 MiB. `kind` names the icon in the
/// error.
fn decode_icon_upload(bytes_base64: &str, kind: &str) -> Result<Vec<u8>> {
    const MAX_ICON_BYTES: usize = 1024 * 1024;
    let too_large = || Err(format!("{kind} icon image exceeds the 1 MiB limit").into());
    if bytes_base64.len() > MAX_ICON_BYTES.div_ceil(3) * 4 {
        return too_large();
    }
    let bytes = BASE64_STANDARD
        .decode(bytes_base64)
        .map_err(anyhow::Error::from)?;
    if bytes.len() > MAX_ICON_BYTES {
        return too_large();
    }
    Ok(bytes)
}
