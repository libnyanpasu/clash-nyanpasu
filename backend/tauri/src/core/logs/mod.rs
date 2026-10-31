mod actor;
mod codec;
mod model;
mod ports;
mod redb;

pub use actor::CoreLogsClient;
pub use model::*;
pub use ports::{CoreLogStore, UnavailableCoreLogStore};
pub use redb::RedbCoreLogStore;

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

pub(crate) use actor::Error as SpawnError;
