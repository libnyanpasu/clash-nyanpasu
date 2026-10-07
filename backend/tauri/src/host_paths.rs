//! GUI package inputs for the frontend-independent directory resolver.
use camino::Utf8PathBuf;
use nyanpasu_paths::{DiscoverError, HostInputs, InstallDirError, PathResolver};
use std::{collections::BTreeMap, sync::Arc};

#[cfg(not(feature = "verge-dev"))]
pub(crate) const APP_NAME: &str = "clash-nyanpasu";
#[cfg(feature = "verge-dev")]
pub(crate) const APP_NAME: &str = "clash-nyanpasu-dev";

/// Collects the host inputs once and resolves the process's directories from them. `run()`
/// calls this once and passes the resolver down.
pub fn discover() -> Result<PathResolver, DiscoverError> {
    let install_dir = tauri::utils::platform::current_exe()
        .map_err(|error| InstallDirError::Executable {
            source: Arc::new(error),
        })
        .and_then(|executable| nyanpasu_paths::installation_dir(&executable));
    // Only Windows has a portable layout. The marker is a package property, read from the
    // same install dir the resolver is given.
    #[cfg(windows)]
    let portable = install_dir
        .as_ref()
        .is_ok_and(|dir| crate::bundle::is_portable(dir.as_std_path()));
    #[cfg(not(windows))]
    let portable = false;
    PathResolver::discover(HostInputs {
        app_name: APP_NAME.to_owned(),
        install_dir,
        portable,
        development_binaries: development_binaries(),
    })
}

#[cfg(debug_assertions)]
fn development_binaries() -> BTreeMap<String, Utf8PathBuf> {
    let Ok(triple) = tauri::utils::platform::target_triple() else {
        return BTreeMap::new();
    };
    nyanpasu_utils::core::CoreType::get_supported_cores()
        .iter()
        .map(|core| {
            let name = core.get_executable_name();
            let stem = name
                .strip_suffix(std::env::consts::EXE_SUFFIX)
                .unwrap_or(name);
            let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("sidecar")
                .join(format!("{stem}-{triple}{}", std::env::consts::EXE_SUFFIX));
            (name.to_owned(), path)
        })
        .collect()
}
#[cfg(not(debug_assertions))]
fn development_binaries() -> BTreeMap<String, Utf8PathBuf> {
    BTreeMap::new()
}
