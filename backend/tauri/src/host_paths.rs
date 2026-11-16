//! GUI package inputs for the frontend-independent directory resolver.
use camino::Utf8PathBuf;
use nyanpasu_paths::{DiscoverError, HostInputs, InstallDirError, PathResolver};
use std::{collections::BTreeMap, sync::Arc};

#[cfg(any(windows, test))]
use std::{fs, path::Path};

#[cfg(not(feature = "verge-dev"))]
pub(crate) const APP_NAME: &str = "clash-nyanpasu";
#[cfg(feature = "verge-dev")]
pub(crate) const APP_NAME: &str = "clash-nyanpasu-dev";

/// Prefix the Windows installer gives a CLI moved aside while its old image is in use.
#[cfg(any(windows, test))]
const CLI_UPDATE_BACKUP_PREFIX: &str = "nyanpasu-cli.exe.old-";

/// The single-instance endpoints of this build, apart from the other build's.
#[cfg(not(feature = "verge-dev"))]
pub(crate) const INSTANCE_NAMESPACE: nyanpasu_single_instance::endpoint::Namespace =
    nyanpasu_single_instance::endpoint::Namespace::new("clash-nyanpasu-instance");
#[cfg(feature = "verge-dev")]
pub(crate) const INSTANCE_NAMESPACE: nyanpasu_single_instance::endpoint::Namespace =
    nyanpasu_single_instance::endpoint::Namespace::new("clash-nyanpasu-dev-instance");

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

/// Removes CLI images that the installer moved aside after their process exited.
///
/// Cleanup is best-effort: a running MCP server can still hold its backup, and a filesystem
/// error must not stop the application from starting.
#[cfg(any(windows, test))]
pub(crate) fn cleanup_cli_update_backups(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(CLI_UPDATE_BACKUP_PREFIX)
        {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        if let Err(error) = fs::remove_file(entry.path()) {
            tracing::debug!(
                path = %entry.path().display(),
                %error,
                "deferring cleanup of an old nyanpasu-cli copy"
            );
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_update_cleanup_removes_only_the_installers_regular_backup_files() {
        let directory = tempfile::tempdir().unwrap();
        let backup = directory
            .path()
            .join(format!("{CLI_UPDATE_BACKUP_PREFIX}1"));
        let current = directory.path().join("nyanpasu-cli.exe");
        let unrelated = directory.path().join("other.exe.old-1");
        fs::write(&backup, b"old").unwrap();
        fs::write(&current, b"current").unwrap();
        fs::write(&unrelated, b"other").unwrap();
        fs::create_dir(
            directory
                .path()
                .join(format!("{CLI_UPDATE_BACKUP_PREFIX}directory")),
        )
        .unwrap();

        cleanup_cli_update_backups(directory.path());

        assert!(!backup.exists());
        assert!(current.exists());
        assert!(unrelated.exists());
        assert!(
            directory
                .path()
                .join(format!("{CLI_UPDATE_BACKUP_PREFIX}directory"))
                .is_dir()
        );
    }
}
