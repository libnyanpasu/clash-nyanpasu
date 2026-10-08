// Legacy GUI app names; filesystem resolution lives in nyanpasu-paths.
#[cfg(not(feature = "verge-dev"))]
#[allow(unused)]
#[allow(dead_code)]
const PREVIOUS_APP_NAME: &str = "clash-verge";
#[cfg(feature = "verge-dev")]
#[allow(dead_code)]
const PREVIOUS_APP_NAME: &str = "clash-verge-dev";
