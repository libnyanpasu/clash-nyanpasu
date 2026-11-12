//! Storage values and opaque commit sequence numbers.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Orders the commits of one open store, every key alike. It is kept in
/// memory and starts at 0 each time the store opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct StorageRevision(u64);

impl StorageRevision {
    /// The revision of a newly opened store.
    pub const INITIAL: Self = Self(0);

    /// Advances the counter without wrapping; the store owns exhaustion handling.
    pub fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// A stored value, or `None` if the key is absent, as of `revision`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StorageItem<T> {
    pub value: Option<T>,
    pub revision: StorageRevision,
}
