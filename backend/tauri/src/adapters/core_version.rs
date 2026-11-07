//! Tauri executable discovery, evaluated at each version read.

use std::path::PathBuf;

use nyanpasu_config::application::ClashCore;
use nyanpasu_contracts::core_version::{
    CoreVersionError, CoreVersionExecutableSource, RunCoreVersionSnafu,
};
use snafu::ResultExt as _;

pub struct TauriCoreVersionExecutableSource;

impl CoreVersionExecutableSource for TauriCoreVersionExecutableSource {
    fn current_exe(&self, core: ClashCore) -> Result<PathBuf, CoreVersionError> {
        tauri::utils::platform::current_exe()
            .map_err(anyhow::Error::from)
            .context(RunCoreVersionSnafu { core })
    }
}
