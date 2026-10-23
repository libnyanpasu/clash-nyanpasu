use async_trait::async_trait;
use nyanpasu_config::application::ClashCore;
use snafu::{ResultExt, ensure};

use crate::client::core_version::{
    CoreVersionError, CoreVersionExitSnafu, CoreVersionReader, RunCoreVersionSnafu, parse_version,
};

pub struct SidecarCoreVersionReader;

#[async_trait]
impl CoreVersionReader for SidecarCoreVersionReader {
    async fn read(&self, core: ClashCore) -> Result<String, CoreVersionError> {
        let argument = match core {
            ClashCore::ClashRs | ClashCore::ClashRsAlpha => "-V",
            _ => "-v",
        };
        let executable = tauri::utils::platform::current_exe()
            .map_err(anyhow::Error::from)
            .context(RunCoreVersionSnafu { core })?;
        let directory = executable.parent().expect("the executable has a parent");
        let directory = if directory.ends_with("deps") {
            directory.parent().unwrap_or(directory)
        } else {
            directory
        };
        let mut binary = directory.join(core.binary_name());
        #[cfg(windows)]
        if binary.extension().is_none_or(|ext| ext != "exe") {
            binary.as_mut_os_string().push(".exe");
        }
        #[cfg(not(windows))]
        if binary.extension().is_some_and(|ext| ext == "exe") {
            binary.set_extension("");
        }
        let mut command = tokio::process::Command::new(binary);
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW, as tauri-plugin-shell does.
        let output = command
            .arg(argument)
            .output()
            .await
            .map_err(anyhow::Error::from)
            .context(RunCoreVersionSnafu { core })?;
        ensure!(output.status.success(), CoreVersionExitSnafu { core });
        parse_version(core, &String::from_utf8_lossy(&output.stdout))
    }
}
