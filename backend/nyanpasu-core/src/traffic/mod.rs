//! Traffic recording. A downstream consumer of the raw Clash connections feed:
//! it never affects the live display, and a storage failure only disables
//! recording.
mod actor;
mod client;
mod geo;
mod ports;
mod source;
mod storage;
#[cfg(test)]
mod tests;

pub use actor::TrafficArgs;
pub use client::{Error as SpawnError, TrafficClient};
pub use ports::{
    Clock, LocalSourceIps, LocalSourceLocation, ProfileSelection, RetentionPolicy,
    TrafficOwnerFactory, TrafficStartArgs, TrafficStorage,
};
pub use storage::TrafficFileStorage;
