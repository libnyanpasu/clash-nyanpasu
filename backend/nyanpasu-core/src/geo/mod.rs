//! The country index traffic regions are looked up in, built from the database
//! the running core reads and kept in step with it.
mod actor;
mod adapters;
mod client;
#[cfg(test)]
pub(crate) mod fixtures;
mod ports;
#[cfg(test)]
mod tests;

pub use actor::GeoIndexArgs;
pub use adapters::FsCountryIndexSource;
pub use client::GeoIndexClient;
#[cfg(test)]
pub use ports::MockCountryIndexSource;
pub use ports::{CountryIndexSource, GeoIndexError, GeodataMode, IndexKey, Loaded, OnChange};
