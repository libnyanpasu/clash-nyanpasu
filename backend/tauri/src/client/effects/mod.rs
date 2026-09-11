//! Pure application-effect protocol: what side effects a committed config
//! change implies, and how their outcomes are reported back.
//!
//! Nothing here touches Tauri, the OS, or the filesystem. The dispatch seam
//! ([`ports::ApplicationEffectsPort`]) is the only place infrastructure enters,
//! and its implementations live with the owners of each effect.
//!
//! The whole module is delivered ahead of its callers, which are wired up by
//! the follow-up config-reconcile and actor tasks.
#![allow(dead_code)]

pub mod plan;
pub mod ports;
pub mod status;
