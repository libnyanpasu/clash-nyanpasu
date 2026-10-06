//! GUI package inputs for the frontend-independent directory resolver.
use nyanpasu_paths::{HostInputs, PathResolver};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

#[cfg(not(feature = "verge-dev"))]
pub(crate) const APP_NAME: &str = "clash-nyanpasu";
#[cfg(feature = "verge-dev")]
pub(crate) const APP_NAME: &str = "clash-nyanpasu-dev";

/// The one resolver of the process; `run()` calls this once and passes it down.
pub fn resolver() -> PathResolver {
    let executable = tauri::utils::platform::current_exe().map_err(Arc::new);
    // Only Windows has a portable layout. The marker is a package property, read from the
    // same install dir the resolver is given.
    #[cfg(windows)]
    let portable = executable
        .as_ref()
        .ok()
        .and_then(|executable| nyanpasu_paths::installation_dir(executable).ok())
        .is_some_and(|dir| crate::bundle::is_portable(&dir));
    #[cfg(not(windows))]
    let portable = false;
    PathResolver::new(HostInputs {
        app_name: APP_NAME.to_owned(),
        executable,
        portable,
        development_binaries: development_binaries(),
    })
}

#[cfg(debug_assertions)]
fn development_binaries() -> BTreeMap<String, PathBuf> {
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
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("sidecar")
                .join(format!("{stem}-{triple}{}", std::env::consts::EXE_SUFFIX));
            (name.to_owned(), path)
        })
        .collect()
}
#[cfg(not(debug_assertions))]
fn development_binaries() -> BTreeMap<String, PathBuf> {
    BTreeMap::new()
}
