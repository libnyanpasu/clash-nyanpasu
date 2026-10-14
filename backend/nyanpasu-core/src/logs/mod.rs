mod actor;
pub mod app;
pub mod archive;
mod codec;
pub mod frontend;
pub mod logging;
mod model;
mod ports;
mod redb;

pub use actor::CoreLogsClient;
pub use model::*;
pub use ports::{CoreLogStore, UnavailableCoreLogStore};
pub use redb::RedbCoreLogStore;

#[cfg(test)]
mod tests;
