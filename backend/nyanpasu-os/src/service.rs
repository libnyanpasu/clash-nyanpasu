//! Service process commands and the OS implementation of process control.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::ExitStatus,
    sync::Arc,
};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

#[cfg(windows)]
use nyanpasu_contracts::service::SessionEndingSnafu;
use nyanpasu_contracts::{
    elevation::{ElevationConsent, ElevationOperation},
    service::{
        AuthorizeServiceCommandSnafu, ResolveServiceDirsSnafu, ResolveServiceUserSnafu,
        RunElevatedSnafu, RunServiceStatusSnafu, ServiceCommand, ServiceCommandError,
        ServiceCommandExitSnafu, ServiceProcessControl, ServiceStatusExitSnafu,
        ServiceStatusNotUtf8Snafu,
    },
};

#[cfg(not(target_os = "macos"))]
use runas::Command as RunasCommand;
use snafu::{ResultExt, ensure};

use crate::elevation::runner::{CommandRunner, OsCommandRunner};

fn signal(status: &ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        status.signal()
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

/// The service binary's name, without the platform's executable suffix.
const SERVICE_BINARY: &str = "nyanpasu-service";

/// The `nyanpasu-service` binary, shipped next to the executable.
pub fn service_binary(install_dir: &Path) -> PathBuf {
    install_dir.join(format!("{SERVICE_BINARY}{}", std::env::consts::EXE_SUFFIX))
}

/// Resolves the current user and builds the service installation command arguments.
pub async fn get_service_install_args(
    data_dir: &Path,
    config_dir: &Path,
    app_dir: &Path,
) -> Result<Vec<OsString>, ServiceCommandError> {
    let user = {
        #[cfg(windows)]
        {
            nyanpasu_utils::os::get_current_user_sid()
                .await
                .context(ResolveServiceUserSnafu)?
        }
        #[cfg(not(windows))]
        {
            whoami::username().context(ResolveServiceUserSnafu)?
        }
    };

    let args: Vec<OsString> = vec![
        "install".into(),
        "--user".into(),
        user.into(),
        "--nyanpasu-data-dir".into(),
        data_dir.into(),
        "--nyanpasu-config-dir".into(),
        config_dir.into(),
        "--nyanpasu-app-dir".into(),
        app_dir.into(),
    ];

    Ok(args)
}

/// Runs `nyanpasu-service` elevated and waits for it to exit successfully.
/// Arguments stay lazy so refusal also precedes install's user-lookup processes.
async fn run_elevated(
    command: ServiceCommand,
    service_binary: &Path,
    args: impl std::future::Future<Output = Result<Vec<OsString>, ServiceCommandError>>,
    consent: &dyn ElevationConsent,
    runner: Arc<dyn CommandRunner>,
) -> Result<(), ServiceCommandError> {
    consent
        .authorize(ElevationOperation::Service { command })
        .context(AuthorizeServiceCommandSnafu { command })?;
    let args = args.await?;
    #[cfg(windows)]
    ensure_session_active()?;
    let service_binary = service_binary.to_path_buf();
    let status = crate::blocking::join(
        tokio::task::spawn_blocking(move || {
            #[cfg(windows)]
            ensure_session_active()?;
            #[cfg(not(target_os = "macos"))]
            {
                runner
                    .runas(
                        RunasCommand::new(service_binary.as_path())
                            .args(&args)
                            .gui(true)
                            .show(true),
                    )
                    .context(RunElevatedSnafu { command })
            }
            #[cfg(target_os = "macos")]
            {
                use crate::elevation::macos;
                let args = args
                    .iter()
                    .map(|arg| format!("'{}'", arg.to_string_lossy().replace('\'', "'\\''")))
                    .collect::<Vec<_>>();
                match macos::sudo(service_binary.to_string_lossy(), &args, runner.as_ref()) {
                    Ok(()) => Ok(std::process::ExitStatus::from_raw(0)),
                    Err(e) => {
                        tracing::error!("failed to run the service {command} command: {}", e);
                        Err(e)
                    }
                }
                .context(RunElevatedSnafu { command })
            }
        })
        .await,
    )?;
    ensure!(
        status.success(),
        ServiceCommandExitSnafu {
            command,
            exit_code: status.code(),
            signal: signal(&status),
        }
    );
    Ok(())
}

/// Installs the service with the supplied application directories.
pub async fn install_service(
    consent: &dyn ElevationConsent,
    service_binary: &Path,
    data_dir: &Path,
    config_dir: &Path,
    app_dir: &Path,
) -> Result<(), ServiceCommandError> {
    run_elevated(
        ServiceCommand::Install,
        service_binary,
        get_service_install_args(data_dir, config_dir, app_dir),
        consent,
        Arc::new(OsCommandRunner),
    )
    .await
}

/// Updates the service using its existing platform command arguments.
pub async fn update_service(
    consent: &dyn ElevationConsent,
    service_binary: &Path,
    data_dir: &Path,
) -> Result<(), ServiceCommandError> {
    let args = async {
        #[cfg(unix)]
        let args = vec![
            "update".into(),
            "--user".into(),
            whoami::username().context(ResolveServiceUserSnafu)?.into(),
            "--nyanpasu-data-dir".into(),
            data_dir.into(),
        ];
        #[cfg(windows)]
        let args = {
            let _ = data_dir;
            vec!["update".into()]
        };
        Ok(args)
    };
    run_elevated(
        ServiceCommand::Update,
        service_binary,
        args,
        consent,
        Arc::new(OsCommandRunner),
    )
    .await
}

/// Removes the service through its elevated command.
pub async fn uninstall_service(
    consent: &dyn ElevationConsent,
    service_binary: &Path,
) -> Result<(), ServiceCommandError> {
    run_elevated(
        ServiceCommand::Uninstall,
        service_binary,
        async { Ok(vec!["uninstall".into()]) },
        consent,
        Arc::new(OsCommandRunner),
    )
    .await
}

/// Starts the service daemon through its elevated command.
pub async fn start_service(
    consent: &dyn ElevationConsent,
    service_binary: &Path,
) -> Result<(), ServiceCommandError> {
    run_elevated(
        ServiceCommand::Start,
        service_binary,
        async { Ok(vec!["start".into()]) },
        consent,
        Arc::new(OsCommandRunner),
    )
    .await
}

/// Stops the service daemon through its elevated command.
pub async fn stop_service(
    consent: &dyn ElevationConsent,
    service_binary: &Path,
) -> Result<(), ServiceCommandError> {
    run_elevated(
        ServiceCommand::Stop,
        service_binary,
        async { Ok(vec!["stop".into()]) },
        consent,
        Arc::new(OsCommandRunner),
    )
    .await
}

/// Restarts the service daemon through its elevated command.
pub async fn restart_service(
    consent: &dyn ElevationConsent,
    service_binary: &Path,
) -> Result<(), ServiceCommandError> {
    run_elevated(
        ServiceCommand::Restart,
        service_binary,
        async { Ok(vec!["restart".into()]) },
        consent,
        Arc::new(OsCommandRunner),
    )
    .await
}

/// Returns raw UTF-8 status output after checking the service process exit status.
#[tracing::instrument]
pub async fn status(service_binary: &Path) -> Result<String, ServiceCommandError> {
    #[cfg(windows)]
    ensure_session_active()?;
    let mut cmd = tokio::process::Command::new(service_binary);
    cmd.args(["status", "--json"]);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = cmd.output().await.context(RunServiceStatusSnafu)?;
    ensure!(
        output.status.success(),
        ServiceStatusExitSnafu {
            exit_code: output.status.code(),
            signal: signal(&output.status),
        }
    );
    let status = String::from_utf8(output.stdout).context(ServiceStatusNotUtf8Snafu)?;
    tracing::trace!("service status: {}", status);
    Ok(status)
}

#[cfg(windows)]
fn ensure_session_active() -> Result<(), ServiceCommandError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};

    // SAFETY: GetSystemMetrics reads an OS metric and takes no pointers.
    ensure!(
        unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) } == 0,
        SessionEndingSnafu
    );
    Ok(())
}

/// Runs service commands against the binary and application directories supplied by the shell.
pub struct OsServiceProcessControl {
    service_binary: PathBuf,
    paths: nyanpasu_paths::PathResolver,
    consent: Arc<dyn ElevationConsent>,
}

impl OsServiceProcessControl {
    /// Binds service process control to the supplied executable and path resolver;
    /// `consent` is checked before every elevated service command.
    pub fn new(
        service_binary: PathBuf,
        paths: nyanpasu_paths::PathResolver,
        consent: Arc<dyn ElevationConsent>,
    ) -> Self {
        Self {
            service_binary,
            paths,
            consent,
        }
    }
}

#[async_trait::async_trait]
impl ServiceProcessControl for OsServiceProcessControl {
    async fn probe(&self) -> Result<String, ServiceCommandError> {
        status(&self.service_binary).await
    }

    async fn install(&self) -> Result<(), ServiceCommandError> {
        let app_dir = self
            .paths
            .app_install_dir()
            .context(ResolveServiceDirsSnafu)?;
        install_service(
            self.consent.as_ref(),
            &self.service_binary,
            self.paths.app_data_dir().as_std_path(),
            self.paths.app_config_dir().as_std_path(),
            app_dir.as_std_path(),
        )
        .await
    }

    async fn uninstall(&self) -> Result<(), ServiceCommandError> {
        uninstall_service(self.consent.as_ref(), &self.service_binary).await
    }

    async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
        start_service(self.consent.as_ref(), &self.service_binary).await
    }

    async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
        stop_service(self.consent.as_ref(), &self.service_binary).await
    }

    async fn update(&self) -> Result<(), ServiceCommandError> {
        update_service(
            self.consent.as_ref(),
            &self.service_binary,
            self.paths.app_data_dir().as_std_path(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refused_service_command_never_spawns() {
        use crate::elevation::{RefuseElevation, runner::CountingRunner};
        use nyanpasu_contracts::elevation::ElevationConsentError;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let runner = Arc::new(CountingRunner::default());
        let prepared = AtomicUsize::new(0);
        for command in [
            ServiceCommand::Install,
            ServiceCommand::Update,
            ServiceCommand::Uninstall,
            ServiceCommand::Start,
            ServiceCommand::Stop,
            ServiceCommand::Restart,
        ] {
            let error = run_elevated(
                command,
                Path::new("unused-service"),
                async {
                    prepared.fetch_add(1, Ordering::SeqCst);
                    Ok(vec![command.to_string().into()])
                },
                &RefuseElevation,
                runner.clone(),
            )
            .await
            .unwrap_err();
            assert_eq!(
                runner.0.load(Ordering::SeqCst),
                0,
                "{command} launched a process"
            );
            assert_eq!(
                prepared.load(Ordering::SeqCst),
                0,
                "{command} prepared its helper"
            );
            assert!(
                matches!(error, ServiceCommandError::AuthorizeServiceCommand {
                command: actual,
                source: ElevationConsentError::AuthorizeElevation {
                    operation: ElevationOperation::Service { command: requested }
                }
            } if actual == command && requested == command)
            );
        }
    }

    #[test]
    fn a_failed_command_names_the_command_and_how_it_ended() {
        let error = ServiceCommandError::ServiceCommandExit {
            command: ServiceCommand::Install,
            exit_code: Some(5),
            signal: None,
        };

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "kind": "service_command_exit",
                "command": "install",
                "exit_code": 5,
                "signal": null,
            })
        );
        assert!(error.to_string().contains("install"), "{error}");
    }

    #[test]
    fn the_service_binary_sits_next_to_the_executable() {
        let dir = tempfile::tempdir().unwrap();

        let binary = service_binary(dir.path());

        assert_eq!(binary.parent(), Some(dir.path()));
        assert_eq!(
            binary.file_name().unwrap().to_str(),
            Some(format!("nyanpasu-service{}", std::env::consts::EXE_SUFFIX).as_str())
        );
    }

    #[test]
    fn the_io_error_stays_out_of_the_wire_form_but_in_the_message() {
        let error = ServiceCommandError::RunElevated {
            command: ServiceCommand::Start,
            source: std::io::Error::other("the user cancelled"),
        };

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({ "kind": "run_elevated", "command": "start" })
        );
        assert!(error.to_string().contains("the user cancelled"), "{error}");
    }
}
