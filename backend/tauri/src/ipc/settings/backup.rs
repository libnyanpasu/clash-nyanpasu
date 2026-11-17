//! The desktop-only `settings.backup` commands: configuration backups.

nyanpasu_rpc::namespace!(expose = []);

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc::error::Result;
use tauri::State;

#[derive(specta::Type, serde::Serialize)]
pub struct ConfigBackupInfo {
    pub name: String,
    pub path: String,
}

#[nyanpasu_rpc::rpc]
#[tauri::command]
#[specta::specta]
pub async fn create_config_backup(client: State<'_, NyanpasuClient>) -> Result<ConfigBackupInfo> {
    let backup = client.create_config_backup().await?;
    Ok(ConfigBackupInfo {
        name: backup.name,
        path: backup.path.to_string_lossy().into_owned(),
    })
}
