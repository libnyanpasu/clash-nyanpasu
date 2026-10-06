//! GUI package inputs for the frontend-independent directory resolver.
use nyanpasu_paths::{HostInputs, PathResolver};
use std::{collections::BTreeMap, path::PathBuf};

#[cfg(not(feature = "verge-dev"))]
pub(crate) const APP_NAME: &str = "clash-nyanpasu";
#[cfg(feature = "verge-dev")]
pub(crate) const APP_NAME: &str = "clash-nyanpasu-dev";

pub fn resolver() -> PathResolver {
    PathResolver::new(HostInputs {
        app_name: APP_NAME.to_owned(),
        executable: tauri::utils::platform::current_exe().map_err(std::sync::Arc::new),
        portable: portable_flag(),
        development_binaries: development_binaries(),
    })
}

/// The portable marker is a GUI package property, sampled once per process.
#[cfg(windows)]
fn portable_flag() -> bool {
    *crate::consts::IS_PORTABLE
}
#[cfg(not(windows))]
fn portable_flag() -> bool {
    false
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
