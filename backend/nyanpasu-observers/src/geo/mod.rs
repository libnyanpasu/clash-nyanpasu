//! Owns the published country index and follows injected database and mode sources.
mod actor;
mod client;
#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;
mod ports;
#[cfg(test)]
mod tests;

pub use actor::GeoIndexArgs;
pub use client::{Error as SpawnError, GeoIndexClient};
#[cfg(test)]
pub use ports::MockCountryIndexSource;
#[cfg(any(test, feature = "test-support"))]
pub use ports::NoopCountryIndexSource;
pub use ports::{
    CountryIndexSource, GeoIndexError, GeodataMode, IndexDatabaseSnafu, IndexKey, ListHomeSnafu,
    Loaded, ModeReporter, ModeSource, OnChange, ReadDatabaseSnafu, WatchHomeSnafu,
};
