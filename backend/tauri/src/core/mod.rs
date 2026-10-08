pub mod backup;
pub mod clash;
pub mod download;
pub mod geo;
pub mod logs;
pub mod manager;
pub mod migration;
pub mod storage;
pub mod traffic;
pub mod tray;
pub mod updater;
#[cfg(windows)]
pub mod win_uwp;

pub(crate) mod proxies;

pub(crate) mod connections;

pub mod status_events;
#[cfg(test)]
pub(crate) mod test_support;
