//! Locale, logger, widget and tray: the effects a committed configuration
//! change has on this process's own user interface.
//!
//! None of them owns long-lived state worth an actor, so each is a narrow
//! boundary implementation in [`adapters`]. The consumed effect traits live in
//! [`nyanpasu_core::client::effects::ports`]; [`ports`] owns the widget runtime contract.
//! The fan-out lives in [`nyanpasu_core::client::effects::executor`], beside the actor
//! clients that own the system-facing effects.

pub mod adapters;
pub mod ports;

#[cfg(test)]
mod tests;
