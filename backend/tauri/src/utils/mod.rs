pub mod blocking;
pub mod candy;
pub mod color;
pub mod config;
pub mod core_version;
pub mod dialog;
pub mod dirs;
pub mod exit;
pub mod help;
pub mod init;
pub mod main_thread;
pub mod path;
pub mod profiling;
pub mod proxy_env;
pub mod resolve;
// mod winhelp;
pub mod hwid;
#[cfg(windows)]
pub mod winreg;

pub mod collect;
pub mod net;

pub mod open;

pub mod dock;
pub mod sudo;

#[cfg(test)]
#[cfg(windows)]
mod winreg_test;
