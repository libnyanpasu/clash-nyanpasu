//! Receiving values for the thirteen local operations and their full-value streams.
//!
//! Ordinary values come directly from their foundation owners. These error values
//! preserve serialized failures without reconstructing the host's operational sources.

mod connection_receive;
mod connections;
mod errors;

pub use connections::{ClashConnection, ClashConnectionDetails};
pub use errors::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CoreLogsChanged {
    pub status: nyanpasu_contracts::logs::CoreLogStatus,
}

#[cfg(test)]
mod tests;
