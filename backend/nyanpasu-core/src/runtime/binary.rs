use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use nyanpasu_config::application::ClashCore;
use serde::Serialize;
use snafu::Snafu;
use tempfile::TempDir;

use crate::error::ErrorPath;

/// Owns the staging directory until installation and its restart have finished.
pub struct PreparedCoreBinary {
    pub target: nyanpasu_config::application::ClashCore,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub staging: Arc<TempDir>,
    pub progress: Arc<dyn BinaryInstallProgress>,
}

pub trait BinaryInstallProgress: Send + Sync + 'static {
    fn restarting(&self);
    /// The actor delivers the terminal result even when the requester stopped waiting.
    fn finished(&self, error: Option<&str>);
}

/// A failure of installing a downloaded core binary over the installed one.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub(crate)))]
pub enum InstallCoreBinaryError {
    #[snafu(display("could not start the elevated copy of the {core} core to {destination}"))]
    StartElevatedCopy {
        #[specta(type = String)]
        core: ClashCore,
        destination: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display(
        "the elevated copy of the {core} core to {destination} failed (exit code {exit_code:?})"
    ))]
    ElevatedCopyFailed {
        #[specta(type = String)]
        core: ClashCore,
        destination: ErrorPath,
        exit_code: Option<i32>,
    },
    // Constructed by the windows-only elevated copy, which needs UTF-8 paths.
    #[cfg_attr(not(windows), allow(dead_code))]
    #[snafu(display("the path {path} is not valid UTF-8"))]
    PathNotUtf8 { path: ErrorPath },
}

#[async_trait]
pub trait BinaryInstaller: Send + Sync + 'static {
    async fn install(&self, artifact: &PreparedCoreBinary) -> Result<(), InstallCoreBinaryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::error::Error;

    #[test]
    fn install_errors_preserve_wire_shape_display_and_source() {
        let core = ClashCore::Mihomo;
        let destination = PathBuf::from("installed-core");
        let error = InstallCoreBinaryError::StartElevatedCopy {
            core,
            destination: (&destination).into(),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "copy denied"),
        };
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({"kind": "start_elevated_copy", "core": core, "destination": "installed-core"})
        );
        assert_eq!(
            error.to_string(),
            format!("could not start the elevated copy of the {core} core to installed-core")
        );
        let source = error
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap();
        assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(source.to_string(), "copy denied");

        for exit_code in [Some(1), None] {
            let error = InstallCoreBinaryError::ElevatedCopyFailed {
                core,
                destination: (&destination).into(),
                exit_code,
            };
            assert_eq!(
                serde_json::to_value(&error).unwrap(),
                json!({"kind": "elevated_copy_failed", "core": core,
                    "destination": "installed-core", "exit_code": exit_code})
            );
            assert_eq!(
                error.to_string(),
                format!(
                    "the elevated copy of the {core} core to installed-core failed (exit code {exit_code:?})"
                )
            );
            assert!(error.source().is_none());
        }
        let error = InstallCoreBinaryError::PathNotUtf8 {
            path: destination.into(),
        };
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            json!({"kind": "path_not_utf8", "path": "installed-core"})
        );
        assert_eq!(
            error.to_string(),
            "the path installed-core is not valid UTF-8"
        );
        assert!(error.source().is_none());
    }

    struct Progress;

    impl BinaryInstallProgress for Progress {
        fn restarting(&self) {}
        fn finished(&self, _: Option<&str>) {}
    }

    #[test]
    fn prepared_binary_staging_lives_until_the_last_owner_finishes() {
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let source = staging.path().join("new-core");
        std::fs::write(&source, b"new binary").unwrap();
        let artifact = PreparedCoreBinary {
            target: ClashCore::Mihomo,
            source: source.clone(),
            destination: staging.path().join("installed-core"),
            staging: staging.clone(),
            progress: Arc::new(Progress),
        };
        drop(artifact);
        assert_eq!(std::fs::read(&source).unwrap(), b"new binary");
        drop(staging);
        assert!(!source.exists());
    }
}
