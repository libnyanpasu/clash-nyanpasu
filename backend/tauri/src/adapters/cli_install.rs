//! The app's "install the command-line tool" action: runs the bundled `nyanpasu-cli install`.
//!
//! The CLI owns how it is linked onto `PATH`, including the standalone copy an AppImage
//! needs because its mount disappears with the app, so the GUI starts the sidecar instead
//! of repeating that logic. This is a desktop adapter: it is reachable only from the
//! native desktop dispatcher.

use std::{
    io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use nyanpasu_paths::{InstallDirError, PathResolver};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt as _, Snafu};
use tokio::process::Command;

/// The sidecar's file name, without a platform extension.
const CLI_NAME: &str = "nyanpasu-cli";

/// What `nyanpasu-cli --json install` reports. The field names are the CLI's `Installed`.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CliInstallation {
    /// The link on `PATH`.
    pub link: String,
    /// The executable the link points at.
    pub target: String,
    /// The link already pointed there, so nothing changed.
    pub already_linked: bool,
    /// Whether the link's directory is on the app's `PATH`.
    pub on_path: bool,
    /// A command that links the executable for every user, which needs elevation.
    pub system_command: String,
}

#[derive(Debug, Snafu)]
pub enum InstallCliError {
    #[snafu(display("the Windows installer puts nyanpasu-cli on PATH"))]
    UnsupportedPlatform,
    #[snafu(display("cannot find the application's install directory"))]
    InstallDir { source: InstallDirError },
    #[snafu(display("cannot run {}", path.display()))]
    Spawn { path: PathBuf, source: io::Error },
    #[snafu(display("nyanpasu-cli could not install itself: {message}"))]
    Refused { message: String },
    #[snafu(display("nyanpasu-cli printed a result this version does not understand"))]
    Result { source: serde_json::Error },
}

/// Links the sidecar next to the application's executable onto the user's `PATH`.
///
/// # Errors
/// Fails when the sidecar cannot be started, or when it refuses; its message names the
/// conflicting entry or the failed filesystem operation.
pub async fn install_cli(paths: &PathResolver) -> Result<CliInstallation, InstallCliError> {
    if cfg!(windows) {
        return Err(InstallCliError::UnsupportedPlatform);
    }
    let cli = paths
        .app_install_dir()
        .context(InstallDirSnafu)?
        .join(CLI_NAME);
    run(cli.as_std_path()).await
}

async fn run(cli: &Path) -> Result<CliInstallation, InstallCliError> {
    let mut attempts = 0;
    let output = loop {
        match Command::new(cli)
            .args(["--json", "install"])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
        {
            Ok(output) => break output,
            Err(err)
                if (err.kind() == io::ErrorKind::ExecutableFileBusy
                    || err.raw_os_error() == Some(26))
                    && attempts < 10 =>
            {
                attempts += 1;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(err) => return Err(err).context(SpawnSnafu { path: cli }),
        }
    };
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(InstallCliError::Refused {
            message: message.trim().trim_start_matches("error: ").to_owned(),
        });
    }
    serde_json::from_slice(&output.stdout).context(ResultSnafu)
}

#[cfg(all(test, unix))]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt as _};

    use super::*;

    /// A stand-in for the sidecar: a script that prints `stdout` and `stderr` and exits.
    fn fake_cli(dir: &Path, stdout: &str, stderr: &str, code: u8) -> PathBuf {
        let path = dir.join("nyanpasu-cli");
        let script =
            format!("#!/bin/sh\nprintf '%s' '{stdout}'\nprintf '%s' '{stderr}' >&2\nexit {code}\n");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[tokio::test]
    async fn the_clis_result_is_decoded() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_cli(
            dir.path(),
            r#"{"link":"/home/u/.local/bin/nyanpasu-cli","target":"/Applications/App/nyanpasu-cli","already_linked":false,"on_path":true,"system_command":"sudo ln -sf x y"}"#,
            "",
            0,
        );

        assert_eq!(
            run(&cli).await.unwrap(),
            CliInstallation {
                link: "/home/u/.local/bin/nyanpasu-cli".to_owned(),
                target: "/Applications/App/nyanpasu-cli".to_owned(),
                already_linked: false,
                on_path: true,
                system_command: "sudo ln -sf x y".to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn a_refusal_carries_the_clis_message() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_cli(
            dir.path(),
            "",
            "error: /home/u/.local/bin/nyanpasu-cli already exists and is not a link; remove it first\n",
            1,
        );

        let error = run(&cli).await.unwrap_err();
        let InstallCliError::Refused { message } = &error else {
            panic!("expected a refusal, got {error:?}");
        };
        assert_eq!(
            message,
            "/home/u/.local/bin/nyanpasu-cli already exists and is not a link; remove it first"
        );
    }

    #[tokio::test]
    async fn an_unreadable_result_and_a_missing_sidecar_are_distinct_failures() {
        let dir = tempfile::tempdir().unwrap();
        let cli = fake_cli(dir.path(), "not json", "", 0);
        assert!(matches!(
            run(&cli).await.unwrap_err(),
            InstallCliError::Result { .. }
        ));

        assert!(matches!(
            run(&dir.path().join("missing")).await.unwrap_err(),
            InstallCliError::Spawn { .. }
        ));
    }
}
