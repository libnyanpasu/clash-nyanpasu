//! Profile-scoped traffic recording. A downstream consumer of the raw Clash
//! connections feed: it never affects the live display, and a storage failure
//! only disables recording.
mod actor;
mod client;
mod ports;
mod source;
#[cfg(test)]
mod tests;

pub use actor::TrafficArgs;
pub use client::TrafficClient;
pub use ports::ProfileSelection;
