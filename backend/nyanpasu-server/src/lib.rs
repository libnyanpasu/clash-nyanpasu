//! `nyanpasu-server`: the headless shell that serves the shared application.
//!
//! It composes the same `NyanpasuClient` as the desktop shell without any GUI,
//! keeps it alive as one long-lived process and serves it to the CLI over a
//! local socket. Findings against core's desktop-shaped ports are recorded as
//! `SPIKE` comments where the headless adapters answer for them.

pub mod adapters;
pub mod compose;
pub mod run;
