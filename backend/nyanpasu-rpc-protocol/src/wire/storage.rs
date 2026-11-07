//! Storage revision and result values; mutation and revision allocation remain application-owned.

/// Orders the commits of one open store, every key alike. It is kept in
/// memory and starts at 0 each time the store opens.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    specta::Type,
)]
#[serde(transparent)]
pub struct StorageRevision(pub u64);

/// A stored value, or `None` if the key is absent, as of `revision`.
#[derive(serde::Deserialize, Debug, Clone, serde::Serialize, specta::Type)]
pub struct StorageItem<T> {
    pub value: Option<T>,
    pub revision: StorageRevision,
}
