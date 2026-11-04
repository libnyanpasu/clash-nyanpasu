//! `nyanpasu-server`: a GUI-free shell over the shared application.
//!
//! This crate is a feasibility spike for the OpenWrt MVP. It constructs the same
//! `NyanpasuClient` as the desktop shell and runs the MVP loop against a real core.
//! Findings against core are recorded as `SPIKE` comments.

pub mod adapters;
pub mod compose;
pub mod scenario;
