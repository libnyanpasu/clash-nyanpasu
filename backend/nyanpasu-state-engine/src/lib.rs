//! Atomic document persistence and versioned read-only snapshots.

pub mod error;
pub mod manager;
mod snapshot;
mod version;

#[cfg(test)]
mod tests;

pub use manager::*;
pub use snapshot::*;
pub use version::*;

#[derive(Debug, Clone)]
pub struct VersionedState<T: Clone + Send + Sync + 'static> {
    pub version: Version,
    pub state: T,
}

impl<T> core::ops::Deref for VersionedState<T>
where
    T: Clone + Send + Sync + 'static,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}
