//! Traffic recording. A downstream consumer of the raw Clash connections feed:
//! it never affects the live display, and a storage failure only disables
//! recording.
mod actor;
mod client;
mod ports;
#[cfg(test)]
mod tests;

pub use actor::TrafficArgs;
pub use client::{Error as SpawnError, TrafficClient};
pub use ports::{
    Clock, FrameNormalizer, LocalSourceIps, LocalSourceLocation, ProfileSelection, RetentionPolicy,
    TrafficFeed, TrafficOwnerFactory, TrafficStartArgs, TrafficStorage,
};
