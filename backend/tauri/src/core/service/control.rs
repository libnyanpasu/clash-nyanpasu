use crate::utils::dirs::{app_config_dir, app_data_dir, app_install_dir};
use runas::Command as RunasCommand;
use serde::Serialize;
use snafu::{ResultExt, Snafu, ensure};
use std::{ffi::OsString, path::Path, process::ExitStatus};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

/// The elevated operations `nyanpasu-service` is asked to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServiceCommand {
    Install,
    Update,
    Uninstall,
    Start,
    Stop,
    Restart,
}

impl std::fmt::Display for ServiceCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Install => "install",
            Self::Update => "update",
            Self::Uninstall => "uninstall",
            Self::Start => "start",
            Self::Stop => "stop",
            Self::Restart => "restart",
        })
    }
}

fn boxed(error: anyhow::Error) -> Box<dyn std::error::Error + Send + Sync> {
    error.into()
}

#[cfg(windows)]
type UserError = std::io::Error;
#[cfg(not(windows))]
type UserError = whoami::Error;

/// A failure of running or querying the system service, including the bounds
/// the service actor puts on every call.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub(crate)))]
pub enum ServiceCommandError {
    #[snafu(display("could not resolve the current user: {source}"))]
    ResolveServiceUser {
        #[serde(skip)]
        source: UserError,
    },
    #[snafu(display("could not resolve the application directories: {source}"))]
    ResolveServiceDirs {
        #[snafu(source(from(anyhow::Error, boxed)))]
        #[serde(skip)]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("could not run the service {command} command: {source}"))]
    RunElevated {
        command: ServiceCommand,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display(
        "the service {command} command failed, exit code: {exit_code:?}, signal: {signal:?}"
    ))]
    ServiceCommandExit {
        command: ServiceCommand,
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
    #[snafu(display("could not query the service status: {source}"))]
    RunServiceStatus {
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display(
        "the service status query failed, exit code: {exit_code:?}, signal: {signal:?}"
    ))]
    ServiceStatusExit {
        exit_code: Option<i32>,
        signal: Option<i32>,
    },
    #[snafu(display("the service status is not valid UTF-8"))]
    ServiceStatusNotUtf8 {
        #[serde(skip)]
        source: std::string::FromUtf8Error,
    },
    #[snafu(display("could not parse the service status: {source}"))]
    ParseServiceStatus {
        #[serde(skip)]
        source: serde_json::Error,
    },
    #[snafu(display("timed out after {limit_ms} ms"))]
    TimedOut { limit_ms: u64 },
    #[snafu(display("timed out after {limit_ms} ms; the command is still running"))]
    StillRunning { limit_ms: u64 },
    #[snafu(display("the command's task was cancelled"))]
    TaskCancelled,
}

#[cfg(test)]
impl ServiceCommandError {
    /// A stand-in for whatever a mocked service host fails with.
    pub(crate) fn mock(reason: &'static str) -> Self {
        Self::RunElevated {
            command: ServiceCommand::Start,
            source: std::io::Error::other(reason),
        }
    }
}

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

pub async fn get_service_install_args() -> Result<Vec<OsString>, ServiceCommandError> {
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
    let data_dir = app_data_dir().context(ResolveServiceDirsSnafu)?;
    let config_dir = app_config_dir().context(ResolveServiceDirsSnafu)?;
    let app_dir = app_install_dir().context(ResolveServiceDirsSnafu)?;

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
async fn run_elevated(
    command: ServiceCommand,
    service_binary: &Path,
    args: Vec<OsString>,
) -> Result<(), ServiceCommandError> {
    let service_binary = service_binary.to_path_buf();
    let status = crate::utils::blocking::join(
        tokio::task::spawn_blocking(move || {
            #[cfg(not(target_os = "macos"))]
            {
                RunasCommand::new(service_binary.as_path())
                    .args(&args)
                    .gui(true)
                    .show(true)
                    .status()
            }
            #[cfg(target_os = "macos")]
            {
                use crate::utils::sudo::sudo;
                let args = args
                    .iter()
                    .map(|arg| format!("'{}'", arg.to_string_lossy().replace('\'', "'\\''")))
                    .collect::<Vec<_>>();
                match sudo(service_binary.to_string_lossy(), &args) {
                    Ok(()) => Ok(std::process::ExitStatus::from_raw(0)),
                    Err(e) => {
                        tracing::error!("failed to run the service {command} command: {}", e);
                        Err(e)
                    }
                }
            }
        })
        .await,
    )
    .context(RunElevatedSnafu { command })?;
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

pub async fn install_service(service_binary: &Path) -> Result<(), ServiceCommandError> {
    let args = get_service_install_args().await?;
    run_elevated(ServiceCommand::Install, service_binary, args).await
}

pub async fn update_service(service_binary: &Path) -> Result<(), ServiceCommandError> {
    #[cfg(unix)]
    let args = vec![
        "update".into(),
        "--user".into(),
        whoami::username().context(ResolveServiceUserSnafu)?.into(),
        "--nyanpasu-data-dir".into(),
        app_data_dir().context(ResolveServiceDirsSnafu)?.into(),
    ];
    #[cfg(windows)]
    let args = vec!["update".into()];
    run_elevated(ServiceCommand::Update, service_binary, args).await
}

pub async fn uninstall_service(service_binary: &Path) -> Result<(), ServiceCommandError> {
    run_elevated(
        ServiceCommand::Uninstall,
        service_binary,
        vec!["uninstall".into()],
    )
    .await
}

pub async fn start_service(service_binary: &Path) -> Result<(), ServiceCommandError> {
    run_elevated(ServiceCommand::Start, service_binary, vec!["start".into()]).await
}

pub async fn stop_service(service_binary: &Path) -> Result<(), ServiceCommandError> {
    run_elevated(ServiceCommand::Stop, service_binary, vec!["stop".into()]).await
}

pub async fn restart_service(service_binary: &Path) -> Result<(), ServiceCommandError> {
    run_elevated(
        ServiceCommand::Restart,
        service_binary,
        vec!["restart".into()],
    )
    .await
}

#[tracing::instrument]
pub async fn status<'a>(
    service_binary: &Path,
) -> Result<nyanpasu_ipc::types::StatusInfo<'a>, ServiceCommandError> {
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
    serde_json::from_str(&status).context(ParseServiceStatusSnafu)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_io_error_stays_out_of_the_wire_form_but_in_the_message() {
        let error = ServiceCommandError::mock("the user cancelled");

        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({ "kind": "run_elevated", "command": "start" })
        );
        assert!(error.to_string().contains("the user cancelled"), "{error}");
    }
}
