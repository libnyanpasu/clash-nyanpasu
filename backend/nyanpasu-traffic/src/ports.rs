use crate::{accounting::FlushBatch, model::*};

/// Synchronous storage port; async callers go through `spawn_blocking`.
pub trait TrafficStore: Send + Sync + 'static {
    fn load(&self) -> TrafficResult<Option<(SessionMeta, Vec<ActiveConnection>)>>;

    /// Applies `batch` in one transaction; see `FlushBatch::reset`.
    fn flush(&self, batch: &FlushBatch) -> TrafficResult<()>;

    /// Newest first, strictly before `before`.
    fn closed_connections(
        &self,
        before: Option<&ClosedCursor>,
        limit: usize,
    ) -> TrafficResult<ClosedPage>;

    fn closed_count(&self) -> TrafficResult<u64>;

    fn totals(&self, group: Dimension) -> TrafficResult<Vec<(String, Bytes)>>;

    /// The stored totals of the distinct `keys`; keys without traffic are left out.
    fn totals_of(&self, group: Dimension, keys: &[String]) -> TrafficResult<Vec<(String, Bytes)>>;

    fn topology(&self) -> TrafficResult<Vec<(TopologyKey, Bytes)>>;
}
