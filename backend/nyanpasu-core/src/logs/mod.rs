//! Redb log storage and codec adapters for the observation owner.

mod codec;
mod redb;

pub use redb::RedbCoreLogStore;

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
