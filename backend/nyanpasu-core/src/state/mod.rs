pub mod ack;
pub mod builder;
pub mod coordinator;
pub mod error;
pub mod manager;
mod snapshot;
mod transaction;
mod version;

#[cfg(test)]
mod tests;

pub use ack::*;
pub use builder::*;
pub use coordinator::*;
pub use manager::*;
pub use snapshot::*;
pub use version::*;

#[derive(Debug, Clone)]
pub struct VersionedState<T: Clone + Send + Sync + 'static> {
    pub version: Version,
    pub state: T,
}

#[cfg(test)]
impl<T> PartialEq<T> for VersionedState<T>
where
    T: Clone + Send + Sync + PartialEq + 'static,
{
    fn eq(&self, other: &T) -> bool {
        self.state.eq(other)
    }
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
