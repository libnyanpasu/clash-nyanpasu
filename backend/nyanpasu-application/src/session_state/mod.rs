//! Actor-owned persistent session/window state.
//!
//! This is application state rather than a desktop-window service: the host
//! supplies its persistence path and the label whose geometry it restores.

mod actor;
mod client;
mod error;

pub use client::{SessionStateClient, SessionStateSnapshot};
pub use error::SessionStateError;
