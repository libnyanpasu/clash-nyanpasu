use std::path::PathBuf;

use nyanpasu_ipc::types::StatusInfo;
use once_cell::sync::Lazy;

use crate::{config::ConfigService, utils::dirs::app_install_dir};

pub mod control;
pub mod ipc;

const SERVICE_NAME: &str = "nyanpasu-service";
static SERVICE_PATH: Lazy<PathBuf> = Lazy::new(|| {
    let app_path = app_install_dir().unwrap();
    app_path.join(format!("{}{}", SERVICE_NAME, std::env::consts::EXE_SUFFIX))
});

pub async fn init_service() {
    let enable_service = {
        *ConfigService::verge()
            .latest()
            .enable_service_mode
            .as_ref()
            .unwrap_or(&false)
    };
    if let Ok(StatusInfo {
        status: nyanpasu_ipc::types::ServiceStatus::Running,
        ..
    }) = control::status().await
        && enable_service
    {
        ipc::spawn_health_check();
        while !ipc::HEALTH_CHECK_RUNNING.load(std::sync::atomic::Ordering::Acquire) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}
