//! Profiles domain state (PR-3). Tauri-free.

pub mod actor;
pub(crate) mod jobs;
mod scheduler;
pub mod sources;

pub use actor::*;
