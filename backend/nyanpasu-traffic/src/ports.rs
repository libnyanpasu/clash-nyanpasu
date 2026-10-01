use std::sync::Arc;

use crate::{accounting::FlushBatch, model::*, query::Usage};

/// What a flush did beyond writing the batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flushed {
    /// Hour-tier rows were deleted, so dimension combinations may have become unreferenced.
    pub hours_pruned: bool,
}

/// Synchronous storage port; async callers go through `spawn_blocking`.
pub trait TrafficStore: Send + Sync + 'static {
    fn load(&self) -> TrafficResult<Option<(SessionMeta, Vec<ActiveConnection>)>>;

    /// Applies `batch` in one transaction: writes the meta, removes and upserts the active
    /// connections, adds the usage of closed ones and their details, then deletes what
    /// `batch.prune` names.
    fn flush(&self, batch: &FlushBatch) -> TrafficResult<Flushed>;

    /// Newest first, strictly before `before`.
    fn closed_connections(
        &self,
        before: Option<&ClosedCursor>,
        limit: usize,
    ) -> TrafficResult<ClosedPage>;

    fn closed_count(&self) -> TrafficResult<u64>;

    /// The usage of every dimension combination in the buckets of `tier` from `from` on (inclusive;
    /// `None` is all of them), one row per combination.
    fn usage(&self, tier: Tier, from: Option<u32>) -> TrafficResult<Vec<(Arc<Dimensions>, Usage)>>;

    /// Deletes the dimension combinations that no usage row refers to and that are not in `keep`;
    /// returns how many.
    fn collect_tuples(&self, keep: &[Arc<Dimensions>]) -> TrafficResult<u64>;
}
