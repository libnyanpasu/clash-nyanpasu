//! Profiles domain state (PR-3). Tauri-free.

pub mod actor;
pub mod error;
pub(crate) mod jobs;
pub mod ports;
mod scheduler;
pub mod sources;

pub use actor::*;
pub use error::*;
