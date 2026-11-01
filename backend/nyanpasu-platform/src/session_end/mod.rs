//! Platform session-end handling and synchronous host cleanup acknowledgement.

mod protocol;

#[cfg(windows)]
pub mod windows;

pub use protocol::ShutdownCompletion;
