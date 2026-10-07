// GUI package version metadata; filesystem resolution lives in nyanpasu-paths.
#[cfg(not(feature = "verge-dev"))]
#[allow(unused)]
#[allow(dead_code)]
const PREVIOUS_APP_NAME: &str = "clash-verge";
#[cfg(feature = "verge-dev")]
#[allow(dead_code)]
const PREVIOUS_APP_NAME: &str = "clash-verge-dev";

pub static APP_VERSION: &str = env!("NYANPASU_VERSION");

pub fn get_app_version() -> &'static str {
    APP_VERSION
}
