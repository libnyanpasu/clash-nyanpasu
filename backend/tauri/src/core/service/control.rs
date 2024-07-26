use crate::utils::dirs::{app_config_dir, app_data_dir, app_install_dir};
use runas::Command as RunasCommand;

use super::SERVICE_PATH;

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
            "failed to install service, exit code: {}",
            child.code().unwrap()
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
            "failed to start service, exit code: {}",
            child.code().unwrap()
        );
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
            "failed to stop service, exit code: {}",
            child.code().unwrap()
        );
    }
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
            "failed to restart service, exit code: {}",
            child.code().unwrap()
        );
    }
    Ok(())
}

pub async fn status() -> anyhow::Result<nyanpasu_ipc::types::ServiceStatus> {
    let child = tokio::process::Command::new(SERVICE_PATH.as_path())
        .args(["status", "--json"])
        .output()
        .await?;
    if !child.status.success() {
        anyhow::bail!(
            "failed to get service status, exit code: {}",
            child.status.code().unwrap()
        );
    }
    let mut status = String::from_utf8(child.stdout)?;
    let status: nyanpasu_ipc::types::ServiceStatus = unsafe { simd_json::from_str(&mut status)? };
    Ok(status)
}
