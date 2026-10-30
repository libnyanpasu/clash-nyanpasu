//! Maps typed startup failures once, then hands presentation to a separate process.
use std::{
    error::Error as StdError,
    io::{self, Write as _},
    path::PathBuf,
    process::{Command, Stdio},
};

use nyanpasu_paths::PathResolver;
use nyanpasu_utils::io::IoFailure;
use serde::{Deserialize, Serialize};
use snafu::ResultExt as _;

use crate::{
    core::{
        corrupt,
        migration::child_result::{Failure, FailureStage, RevisionFailure},
    },
    setup,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailureKind {
    Directories,
    MigrationFailed,
    NewerVersion,
    UserConfig,
    CoreRuntimeDirectory,
    JobsDatabase,
    WebView2,
    Logging,
    Permission,
    Busy,
    Internal,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct StartupFailure {
    pub kind: FailureKind,
    pub path: Option<PathBuf>,
    pub detail: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Report {
    pub failure: StartupFailure,
    pub version: String,
    pub logs_dir: PathBuf,
    pub backups_dir: Option<PathBuf>,
}

impl StartupFailure {
    pub(crate) fn from_setup(error: &setup::Error, paths: &PathResolver) -> Self {
        use crate::client::SetupError;
        use setup::Error;
        let (kind, path) = match error {
            Error::PrepareMigrations { source } | Error::RunMigrations { source } => {
                let report = Failure::from_error(source, FailureStage::Migration, paths, true);
                let mut failure = Self::from_migration(&report);
                failure.detail = snafu::Report::from_error(error).to_string();
                return failure;
            }
            Error::LocateInstallDirectory { source } => (
                FailureKind::Directories,
                match source {
                    nyanpasu_paths::InstallDirError::NotUtf8 { path } => Some(path.clone()),
                    _ => None,
                },
            ),
            Error::BuildCoreHost { source } => match source {
                crate::core::actor_v2::local_host::Error::InspectConfig { path, .. }
                | crate::core::actor_v2::local_host::Error::ReadCoreKind { path, .. } => {
                    (FailureKind::UserConfig, Some(path.clone()))
                }
                _ => (
                    FailureKind::CoreRuntimeDirectory,
                    Some(
                        paths
                            .app_config_dir()
                            .join("runtime/control")
                            .into_std_path_buf(),
                    ),
                ),
            },
            Error::StartJobs { .. } => (
                FailureKind::JobsDatabase,
                Some(paths.jobs_path().into_std_path_buf()),
            ),
            Error::OpenStorage { .. } => (
                FailureKind::Internal,
                Some(paths.storage_path().into_std_path_buf()),
            ),
            Error::BuildClient { source } => match source {
                SetupError::LoadApplication {
                    source:
                        crate::client::application::Error::LoadState { path, .. }
                        | crate::client::application::Error::InitializeState { path, .. },
                } => (
                    FailureKind::UserConfig,
                    Some(path.clone().into_std_path_buf()),
                ),
                SetupError::LoadApplication { .. } => (
                    FailureKind::UserConfig,
                    Some(paths.application_config_path().into_std_path_buf()),
                ),
                SetupError::LoadClashConfig { .. } => (
                    FailureKind::UserConfig,
                    Some(paths.clash_config_path().into_std_path_buf()),
                ),
                SetupError::LoadProfiles {
                    source: crate::client::profiles::Error::PrepareMaterialization { .. },
                } => (
                    FailureKind::Directories,
                    Some(
                        paths
                            .app_profiles_dir()
                            .join(".profile-materialization-v1")
                            .into_std_path_buf(),
                    ),
                ),
                SetupError::LoadProfiles { .. } => (
                    FailureKind::UserConfig,
                    Some(paths.profiles_path().into_std_path_buf()),
                ),
                SetupError::LoadSessionState { .. } => (
                    FailureKind::Internal,
                    Some(paths.session_state_path().into_std_path_buf()),
                ),
                SetupError::CleanupCandidates { .. } => (
                    FailureKind::CoreRuntimeDirectory,
                    Some(
                        paths
                            .app_config_dir()
                            .join("runtime/.candidates")
                            .into_std_path_buf(),
                    ),
                ),
                SetupError::StartAppLogs { .. } | SetupError::StartCoreLogs { .. } => (
                    FailureKind::Logging,
                    Some(paths.app_logs_dir().into_std_path_buf()),
                ),
                _ => (FailureKind::Internal, None),
            },
            _ => (FailureKind::Internal, None),
        };
        Self::from_error(kind, path, error)
    }

    pub(crate) fn from_rpc(error: &setup::UnifiedRpcError) -> Self {
        Self::from_error(FailureKind::Internal, None, error)
    }

    pub(crate) fn from_error(
        kind: FailureKind,
        path: Option<PathBuf>,
        error: &(dyn StdError + 'static),
    ) -> Self {
        let kind = io_kind(corrupt::error_io(error)).unwrap_or(kind);
        let mut path = path;
        let mut current = Some(error);
        while let Some(error) = current {
            if let Some(corrupt::Error::PreserveEvidence { path: original, .. }) =
                error.downcast_ref::<corrupt::Error>()
            {
                path = Some(original.clone());
            }
            if let Some(
                nyanpasu_core_manager::Error::UnsafeRuntimeArtifact(actual)
                | nyanpasu_core_manager::Error::RuntimeDirectoryOwned(actual),
            ) = error.downcast_ref::<nyanpasu_core_manager::Error>()
            {
                path = Some(actual.as_std_path().to_owned());
            }
            current = error.source();
        }
        Self {
            kind,
            path,
            detail: snafu::Report::from_error(error).to_string(),
        }
    }

    fn from_migration(report: &Failure) -> Self {
        let kind = match report.revision {
            Some(
                RevisionFailure::NewerStoreSchema { .. }
                | RevisionFailure::NewerDocument { .. }
                | RevisionFailure::NewerAppliedModule { .. },
            ) => FailureKind::NewerVersion,
            _ => FailureKind::MigrationFailed,
        };
        Self {
            kind: io_kind(report.io_failure).unwrap_or(kind),
            path: Some(report.paths.first().unwrap_or(&report.config_dir).clone()),
            detail: snafu::Report::from_error(report).to_string(),
        }
    }
}

fn io_kind(failure: Option<IoFailure>) -> Option<FailureKind> {
    match failure {
        Some(IoFailure::PermissionDenied) => Some(FailureKind::Permission),
        Some(IoFailure::Transient) => Some(FailureKind::Busy),
        _ => None,
    }
}

impl Report {
    pub(crate) fn new(failure: StartupFailure, paths: &PathResolver) -> Self {
        let backups_dir = matches!(
            failure.kind,
            FailureKind::MigrationFailed | FailureKind::NewerVersion | FailureKind::UserConfig
        )
        .then(|| paths.backups_dir().into_std_path_buf());
        Self {
            failure,
            version: format!(
                "Clash Nyanpasu {} ({})",
                crate::consts::BUILD_INFO.pkg_version,
                crate::consts::BUILD_INFO.commit_hash
            ),
            logs_dir: paths.app_logs_dir().into_std_path_buf(),
            backups_dir,
        }
    }

    pub(crate) fn migration(error: &crate::utils::init::Error, paths: &PathResolver) -> Self {
        let mut report = match error {
            crate::utils::init::Error::ReadChildFailure { source } => {
                let mut report = Self::new(StartupFailure::from_migration(source), paths);
                report.backups_dir = source.backup_available.then(|| source.backups_dir.clone());
                report
            }
            _ => {
                let mut report = Self::new(
                    StartupFailure::from_error(
                        FailureKind::MigrationFailed,
                        Some(paths.app_config_dir().as_std_path().to_owned()),
                        error,
                    ),
                    paths,
                );
                report.backups_dir = None;
                report
            }
        };
        // The pre-Tauri migration writes migration.log here, before the app logger exists.
        report.logs_dir = paths.app_data_dir().as_std_path().to_owned();
        report
    }

    pub(crate) fn input(&self) -> nyanpasu_crash_reporter::StartupFailureInput {
        let containing_dir = match self.failure.kind {
            FailureKind::Internal | FailureKind::Logging | FailureKind::WebView2 => None,
            _ => self
                .failure
                .path
                .as_ref()
                .and_then(|path| path.parent().map(ToOwned::to_owned)),
        };
        nyanpasu_crash_reporter::StartupFailureInput {
            version: self.version.clone(),
            path: self.failure.path.clone(),
            details: format!(
                "{}\n{}\n{}\n\n{}",
                self.version,
                self.failure
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                self.logs_dir.display(),
                self.failure.detail
            ),
            containing_dir,
            backups_dir: self.backups_dir.clone(),
            logs_dir: Some(self.logs_dir.clone()),
        }
    }
}

#[derive(Debug, snafu::Snafu)]
enum Error {
    #[snafu(display("failed to encode startup failure"))]
    Encode { source: serde_json::Error },
    #[snafu(display("failed to locate reporter executable"))]
    LocateExecutable { source: io::Error },
    #[snafu(display("failed to spawn startup reporter"))]
    Spawn { source: io::Error },
    #[snafu(display("failed to send startup failure"))]
    Write { source: io::Error },
    #[snafu(display("failed to wait for startup reporter"))]
    Wait { source: io::Error },
    #[snafu(display("startup reporter exited with {status}"))]
    ChildExited { status: std::process::ExitStatus },
}

/// Waits only for the dialog. Owner cleanup begins after setup returns to Tauri.
pub(crate) fn show(report: &Report) {
    show_with(
        report,
        || {
            let exe = tauri::utils::platform::current_exe().context(LocateExecutableSnafu)?;
            let mut command = Command::new(exe);
            command.args(["startup-failed", "--language", rust_i18n::locale().as_ref()]);
            run_child(command, report)
        },
        crate::utils::dialog::error_dialog,
    );
}

/// Records an internal bootstrap failure before showing A4's reporter and flushing logging.
pub(crate) fn show_internal(
    error: &(dyn StdError + 'static),
    paths: &PathResolver,
    logging: &crate::utils::init::logging::LoggingShutdown,
) {
    let failure = StartupFailure {
        kind: FailureKind::Internal,
        path: None,
        detail: snafu::Report::from_error(error).to_string(),
    };
    logging.record_startup_failure(failure.detail.clone());
    show(&Report::new(failure, paths));
    logging.finish_blocking();
}

/// Presents pre-Tauri failures without starting a reporter or event loop.
pub(crate) fn show_one_shot(
    kind: FailureKind,
    path: Option<PathBuf>,
    error: &(dyn StdError + 'static),
    recovery_key: Option<&str>,
) {
    let failure = StartupFailure {
        kind,
        path,
        detail: snafu::Report::from_error(error).to_string(),
    };
    tracing::error!(kind = ?failure.kind, path = ?failure.path, error = %failure.detail, "application startup failed");
    let message = one_shot_message(&failure, rust_i18n::locale().as_ref(), recovery_key);
    eprintln!("{message}");
    crate::utils::dialog::error_dialog(message);
}

fn one_shot_message(failure: &StartupFailure, locale: &str, recovery_key: Option<&str>) -> String {
    let text = crate::cmds::startup_failed::text(failure.kind, locale);
    let recovery = recovery_key.map_or(text.recovery, |key| {
        let key = format!("dialog.startup_failure.{key}");
        rust_i18n::t!(&key, locale = locale).into_owned()
    });
    let path = failure
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| {
            rust_i18n::t!("dialog.startup_failure.unresolved_path", locale = locale).into_owned()
        });
    format!(
        "{}\n\n{}\n\n{}\n\n{}: {}\n\n{}",
        text.heading, text.description, recovery, text.path, path, failure.detail
    )
}

fn show_with(
    report: &Report,
    child: impl FnOnce() -> Result<(), Error>,
    fallback: impl FnOnce(String),
) {
    if let Err(error) = child() {
        tracing::error!(error = %snafu::Report::from_error(&error), "startup reporter unavailable");
        let text =
            crate::cmds::startup_failed::text(report.failure.kind, rust_i18n::locale().as_ref());
        fallback(format!(
            "{}\n\n{}\n\n{}\n\n{}",
            text.heading,
            text.description,
            text.recovery,
            report.input().details
        ));
    }
}

fn run_child(mut command: Command, report: &Report) -> Result<(), Error> {
    let json = serde_json::to_vec(report).context(EncodeSnafu)?;
    let mut child = command.stdin(Stdio::piped()).spawn().context(SpawnSnafu)?;
    let mut stdin = child.stdin.take().expect("the reporter stdin is piped");
    let written = stdin.write_all(&json).context(WriteSnafu);
    drop(stdin); // The child reads until EOF; waiting with this open deadlocks.
    let status = child.wait().context(WaitSnafu)?;
    written?;
    snafu::ensure!(status.success(), ChildExitedSnafu { status });
    Ok(())
}

#[cfg(test)]
#[path = "startup_failure_tests.rs"]
mod tests;
