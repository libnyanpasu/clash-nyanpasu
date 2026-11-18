//! The desktop-only `remote` commands. The shared ones are in `nyanpasu_rpc::commands::remote`.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc::error::Result;
use tauri::State;

use crate::adapters::certificate_viewer;

/// Shows the saved HTTPS certificate in the platform's read-only certificate viewer. It opens a
/// window on this computer, so only the desktop offers it.
#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn open_remote_access_certificate(
    client: State<'_, NyanpasuClient>,
    paths: State<'_, PathResolver>,
) -> Result<()> {
    let certificate = client
        .remote_access_certificate_der()
        .await?
        .ok_or_else(|| "no certificate has been generated yet".to_owned())?;
    certificate_viewer::show(certificate, paths.cache_dir().as_std_path())
        .map_err(|error| error.to_string())?;
    Ok(())
}
