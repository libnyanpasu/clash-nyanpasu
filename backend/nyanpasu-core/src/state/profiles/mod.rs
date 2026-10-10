//! Profiles domain state (PR-3). Tauri-free.

pub mod actor;
pub mod error;
pub(crate) mod jobs;
mod scheduler;
pub mod sources;
#[cfg(test)]
pub(crate) mod test_support;

pub use actor::*;
pub use error::*;
