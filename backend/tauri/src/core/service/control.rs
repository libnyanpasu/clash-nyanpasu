use crate::utils::dirs::{app_config_dir, app_data_dir, app_install_dir};
use runas::Command as RunasCommand;

use super::SERVICE_PATH;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

pub async fn install_service() -> anyhow::Result<()> {
    let user = {
        #[cfg(windows)]
        {
            nyanpasu_utils::os::get_current_user_sid().await?
        }
        #[cfg(not(windows))]
        {
            whoami::username()
        }
    };
    let data_dir = app_data_dir()?;
    let config_dir = app_config_dir()?;
    let app_dir = app_install_dir()?;
    let child = tokio::task::spawn_blocking(move || {
        RunasCommand::new(SERVICE_PATH.as_path())
            .args(&[
                "install",
                "--user",
                &user,
                "--nyanpasu-data-dir",
                data_dir.to_str().unwrap(),
                "--nyanpasu-config-dir",
                config_dir.to_str().unwrap(),
                "--nyanpasu-app-dir",
                app_dir.to_str().unwrap(),
            ])
            .gui(true)
            .show(true)
            .status()
    })
    .await??;
    if !child.success() {
        anyhow::bail!(
            "failed to install service, exit code: {}, signal: {:?}",
            child.code().unwrap_or(-1),
            {
                #[cfg(unix)]
                {
                    child.signal().unwrap_or(0)
                }
                #[cfg(not(unix))]
                {
                    0
                }
            }
        );
    }
    // Due to most platform, the service will be started automatically after installed
    if !super::ipc::HEALTH_CHECK_RUNNING.load(std::sync::atomic::Ordering::Relaxed) {
        super::ipc::spawn_health_check();
    }
    Ok(())
}

pub async fn update_service() -> anyhow::Result<()> {
    let child = tokio::task::spawn_blocking(move || {
        RunasCommand::new(SERVICE_PATH.as_path())
            .args(&["update"])
            .gui(true)
            .show(true)
            .status()
    })
    .await??;
    if !child.success() {
        anyhow::bail!(
            "failed to install service, exit code: {}, signal: {:?}",
            child.code().unwrap_or(-1),
            {
                #[cfg(unix)]
                {
                    child.signal().unwrap_or(0)
                }
                #[cfg(not(unix))]
                {
                    0
                }
            }
        );
    }
    Ok(())
}

pub async fn uninstall_service() -> anyhow::Result<()> {
    let child = tokio::task::spawn_blocking(move || {
        RunasCommand::new(SERVICE_PATH.as_path())
            .args(&["uninstall"])
            .gui(true)
            .show(true)
            .status()
    })
    .await??;
    if !child.success() {
        anyhow::bail!(
            "failed to uninstall service, exit code: {}",
            child.code().unwrap()
        );
    }
    let _ = super::ipc::KILL_FLAG.compare_exchange(
        false,
        true,
        std::sync::atomic::Ordering::Acquire,
        std::sync::atomic::Ordering::Relaxed,
    );
    Ok(())
}

pub async fn start_service() -> anyhow::Result<()> {
    let child = tokio::task::spawn_blocking(move || {
        RunasCommand::new(SERVICE_PATH.as_path())
            .args(&["start"])
            .gui(true)
            .show(true)
            .status()
    })
    .await??;
    if !child.success() {
        anyhow::bail!(
            "failed to install service, exit code: {}, signal: {:?}",
            child.code().unwrap_or(-1),
            {
                #[cfg(unix)]
                {
                    child.signal().unwrap_or(0)
                }
                #[cfg(not(unix))]
                {
                    0
                }
            }
        );
    }
    if !super::ipc::HEALTH_CHECK_RUNNING.load(std::sync::atomic::Ordering::Acquire) {
        super::ipc::spawn_health_check();
    }
    Ok(())
}

pub async fn stop_service() -> anyhow::Result<()> {
    let child = tokio::task::spawn_blocking(move || {
        RunasCommand::new(SERVICE_PATH.as_path())
            .args(&["stop"])
            .gui(true)
            .show(true)
            .status()
    })
    .await??;
    if !child.success() {
        anyhow::bail!(
            "failed to install service, exit code: {}, signal: {:?}",
            child.code().unwrap_or(-1),
            {
                #[cfg(unix)]
                {
                    child.signal().unwrap_or(0)
                }
                #[cfg(not(unix))]
                {
                    0
                }
            }
        );
    }
    let _ = super::ipc::KILL_FLAG.compare_exchange_weak(
        false,
        true,
        std::sync::atomic::Ordering::Acquire,
        std::sync::atomic::Ordering::Relaxed,
    );
    Ok(())
}

pub async fn restart_service() -> anyhow::Result<()> {
    let child = tokio::task::spawn_blocking(move || {
        RunasCommand::new(SERVICE_PATH.as_path())
            .args(&["restart"])
            .gui(true)
            .show(true)
            .status()
    })
    .await??;
    if !child.success() {
        anyhow::bail!(
            "failed to install service, exit code: {}, signal: {:?}",
            child.code().unwrap_or(-1),
            {
                #[cfg(unix)]
                {
                    child.signal().unwrap_or(0)
                }
                #[cfg(not(unix))]
                {
                    0
                }
            }
        );
    }
    if !super::ipc::HEALTH_CHECK_RUNNING.load(std::sync::atomic::Ordering::Acquire) {
        super::ipc::spawn_health_check();
    }
    Ok(())
}

pub async fn status<'a>() -> anyhow::Result<nyanpasu_ipc::types::StatusInfo<'a>> {
    let mut cmd = tokio::process::Command::new(SERVICE_PATH.as_path());
    cmd.args(["status", "--json"]);
    #[cfg(windows)]
    cmd.creation_flags(0x8000000); // CREATE_NO_WINDOW
    let output = cmd.output().await?;
    if !output.status.success() {
        anyhow::bail!(
            "failed to install service, exit code: {}, signal: {:?}",
            output.status.code().unwrap_or(-1),
            {
                #[cfg(unix)]
                {
                    output.status.signal().unwrap_or(0)
                }
                #[cfg(not(unix))]
                {
                    0
                }
            }
        );
    }
    let mut status = String::from_utf8(output.stdout)?;
    tracing::debug!("service status: {}", status);
    Ok(unsafe { simd_json::serde::from_str(&mut status)? })
}
