use async_trait::async_trait;
use nyanpasu_config::application::ClashCore;
use snafu::{ResultExt, ensure};
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use nyanpasu_core::runtime::version::{
    CoreVersionError, CoreVersionExitSnafu, CoreVersionReader, RunCoreVersionSnafu, parse_version,
};

pub struct TauriCoreVersionReader {
    app: AppHandle,
}

impl TauriCoreVersionReader {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait]
impl CoreVersionReader for TauriCoreVersionReader {
    async fn read(&self, core: ClashCore) -> Result<String, CoreVersionError> {
        let argument = match core {
            ClashCore::ClashRs | ClashCore::ClashRsAlpha => "-V",
            _ => "-v",
        };
        let command = self
            .app
            .shell()
            .sidecar(core.binary_name())
            .map_err(anyhow::Error::from)
            .context(RunCoreVersionSnafu { core })?;
        let output = command
            .args([argument])
            .output()
            .await
            .map_err(anyhow::Error::from)
            .context(RunCoreVersionSnafu { core })?;
        ensure!(output.status.success(), CoreVersionExitSnafu { core });
        parse_version(core, &String::from_utf8_lossy(&output.stdout))
    }
}
