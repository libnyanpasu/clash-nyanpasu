//! Discovery of the local RPC endpoint.
//!
//! Desktop, server and CLI find the listener of one data directory through the same
//! `nyanpasu-single-instance` discovery, so the CLI needs no discovery file. Connecting
//! goes through the validated endpoint, which refuses a socket in an untrusted
//! directory before any byte is sent.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use nyanpasu_paths::{HostInputs, InstallDirError, PathResolver};
use nyanpasu_single_instance::endpoint::{Endpoint, Namespace};

use crate::error::CliError;

/// The namespace of the RPC listener. It must equal `nyanpasu_rpc::local::ENDPOINT_NAMESPACE`,
/// which the CLI cannot depend on; the integration tests against a real shell guard it.
pub const RPC_NAMESPACE: Namespace = Namespace::new("nyanpasu-rpc");

/// The application name whose data directory identifies the desktop instance.
const APP_NAME: &str = "clash-nyanpasu";

/// Marker file of a portable Windows install, next to the executable.
#[cfg(windows)]
const PORTABLE_MARKER: &str = ".config/PORTABLE";

fn default_data_directory() -> Result<PathBuf, CliError> {
    let install_dir = std::env::current_exe()
        .map_err(|source| InstallDirError::Executable {
            source: Arc::new(source),
        })
        .and_then(|executable| nyanpasu_paths::installation_dir(&executable));
    #[cfg(windows)]
    let portable = install_dir
        .as_ref()
        .is_ok_and(|dir| dir.as_std_path().join(PORTABLE_MARKER).exists());
    #[cfg(not(windows))]
    let portable = false;
    let resolver = PathResolver::discover(HostInputs {
        app_name: APP_NAME.to_owned(),
        install_dir,
        portable,
        development_binaries: BTreeMap::new(),
    })
    .map_err(|error| CliError::Unreachable {
        target: "the local socket".to_owned(),
        detail: format!("cannot resolve the app data directory: {error}"),
    })?;
    Ok(resolver.app_data_dir().as_std_path().to_owned())
}

fn discover_blocking(location: Option<PathBuf>) -> Result<Endpoint, CliError> {
    let directory = match &location {
        Some(path) => path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| CliError::usage("the socket endpoint has no parent directory"))?,
        None => default_data_directory()?,
    };
    Endpoint::discover(RPC_NAMESPACE, &directory, location.as_deref()).map_err(|error| {
        CliError::Unreachable {
            target: "the local socket".to_owned(),
            detail: crate::transport::error_chain(&error),
        }
    })
}

/// Locates the local RPC endpoint: the explicit `location`, or the default one of the
/// app data directory. Runs the blocking filesystem inspection off the async workers.
///
/// # Errors
/// An unresolvable data directory or an insecure location is unreachable.
pub async fn locate(location: Option<PathBuf>) -> Result<Endpoint, CliError> {
    tokio::task::spawn_blocking(move || discover_blocking(location))
        .await
        .map_err(|error| CliError::failed(format!("endpoint discovery failed: {error}")))?
}

/// A `0700` directory inside a temporary one, as the endpoint policy requires.
#[cfg(all(test, unix))]
pub(crate) fn private_test_directory(parent: &Path) -> PathBuf {
    use std::os::unix::fs::DirBuilderExt as _;

    let directory = parent.canonicalize().unwrap().join("run");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .unwrap();
    directory
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_explicit_location_is_used_verbatim() {
        let directory = tempfile::tempdir().unwrap();
        let path = private_test_directory(directory.path()).join("rpc.sock");
        let endpoint = locate(Some(path.clone())).await.unwrap();
        assert_eq!(endpoint.path(), path);
    }

    #[tokio::test]
    async fn a_relative_location_is_refused() {
        let error = locate(Some(PathBuf::from("rpc.sock")))
            .await
            .expect_err("relative sockets are refused");
        assert!(
            matches!(error, CliError::Unreachable { .. } | CliError::Usage { .. }),
            "{error}"
        );
    }
}
