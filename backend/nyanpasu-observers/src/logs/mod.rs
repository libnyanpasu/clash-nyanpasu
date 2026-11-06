//! Serial log capture, buffering, and queries over an injected storage port.

mod actor;
mod model;
mod ports;

pub use actor::{CoreLogsClient, Error as SpawnError};
pub use model::*;
pub use ports::{CoreLogStore, UnavailableCoreLogStore};
