mod actor;
mod codec;
mod model;
mod ports;
mod redb;

pub use actor::CoreLogsClient;
pub use model::*;
pub use ports::{CoreLogStore, UnavailableCoreLogStore};
pub use redb::RedbCoreLogStore;

#[cfg(any(test, feature = "test-support"))]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

pub use actor::Error as SpawnError;
