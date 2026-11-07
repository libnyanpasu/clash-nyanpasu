//! Locale, logger, widget and tray: the effects a committed configuration
//! change has on this process's own user interface.
//!
//! None of them owns long-lived state worth an actor, so each is a narrow
//! boundary implementation in [`adapters`]. The consumed effect traits live in
//! [`crate::desktop::effects::ports`]; [`ports`] owns the widget runtime contract.
//! Presentation execution lives in [`crate::desktop::effects::presentation`];
//! shared effects remain in [`nyanpasu_core::client::effects::executor`].

pub mod adapters;
pub mod ports;

#[cfg(test)]
mod tests;
