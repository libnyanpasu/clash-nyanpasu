use std::path::PathBuf;

use once_cell::sync::Lazy;

use crate::utils::dirs::app_install_dir;

pub mod control;
pub mod ipc;

const SERVICE_NAME: &str = "nyanpasu-service";
static SERVICE_PATH: Lazy<PathBuf> = Lazy::new(|| {
    let app_path = app_install_dir().unwrap();
    app_path.join(format!("{}{}", SERVICE_NAME, std::env::consts::EXE_SUFFIX))
});
