//! Observability (§9.4). `status()` reports, per tier and per stream: rows, bytes, the oldest
//! and newest data, the deletion watermark, cumulative folding, deletions by reason,
//! dropped late writes and the maintenance backlog. It also returns the ring of the last 64
//! lossy operations, each written in the same transaction as the operation itself.

use serde::{Deserialize, Serialize};
use snafu::Snafu;
use strum::FromRepr;

use crate::{
    CaptureError, ErrorKind, LifecycleState,
    budget::{BudgetExceeded, FrozenVec, Reserved},
    catalog::PinError,
    layout::{
        diagnostics::DiagnosticsError,
        manifest::{ManifestError, StreamRef},
        meta::MetaError,
        references::ReferenceError,
    },
    schema::MAX_FIELDS,
    storage::StorageError,
};

mod read;

/// One diagnostic observation. Its data arrays retain query credit until this value drops.
/// Outside Open there is no data and no retained charge or old version.
#[derive(Debug)]
pub struct Status {
    /// Lifecycle at this observation; non-Open observations carry no dataset totals.
    pub lifecycle: LifecycleState,
    /// Query credit reserved at return, including this result and any live snapshots/results.
    pub query_reserved_bytes: u64,
    /// Owned scalars and arrays only, never a storage snapshot or catalog/version reference.
    data: Option<Reserved<StatusData>>,
}

impl Status {
    /// Dataset totals, absent while the facade is suspended, closed or requires recovery.
    pub fn data(&self) -> Option<&StatusData> {
        self.data.as_ref().map(Reserved::get)
    }

    /// Builds a state-only observation without allocating or retaining query credit.
    pub(crate) fn unavailable(lifecycle: LifecycleState, query_reserved_bytes: u64) -> Self {
        Self {
            lifecycle,
            query_reserved_bytes,
            data: None,
        }
    }
}

/// Metadata of one captured publication. Stored rows are representation rows, not input counts.
#[derive(Debug)]
pub struct StatusData {
    /// Open incarnation in the captured state record.
    pub incarnation: u64,
    /// Publication shared by storage metadata and the captured head/catalog.
    pub publication: u64,
    /// Last batch in that publication; None before any batch.
    pub last_batch: Option<u64>,
    /// Stored journal bytes, keys included, from its state record.
    pub journal_bytes: u64,
    /// Resident catalog sizes and persistent pin/recycling counts.
    pub catalog: CatalogStatus,
    /// Ephemeral backlog, profile gate and policy-free persistent work counts.
    pub maintenance: MaintenanceStatus,
    /// One element per schema tier then event stream, retaining query credit.
    streams: FrozenVec<StreamStatus>,
    /// At most 64 surviving records, in sequence order, retaining query credit.
    losses: FrozenVec<LossRecord>,
}

impl StatusData {
    /// Schema-order tier and event stream summaries, including empty streams.
    pub fn streams(&self) -> &[StreamStatus] {
        &self.streams
    }
    /// Surviving E7 history, oldest sequence first, independent of slot-key order.
    pub fn losses(&self) -> &[LossRecord] {
        &self.losses
    }
}

/// One stream's currently visible representation, cumulative input losses and pending cleanup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamStatus {
    /// Tier or event stream this element describes.
    pub stream: StreamRef,
    /// Unexpired ChunkEntry.rows plus head accumulator combinations or head event rows.
    /// Multiple chunks' aggregates count separately; this is not a logical distinct-row count.
    pub rows: u64,
    /// Unexpired chunk fragment bytes plus head journal bytes, the latter including keys.
    pub bytes: u64,
    /// Smallest actual series bucket or event key; None for an empty visible stream.
    pub oldest: Option<i64>,
    /// Largest actual series bucket or event key, inclusive; None when empty.
    pub newest: Option<i64>,
    /// Last expired extent number, not a timestamp or a policy-derived current cutoff.
    pub expiration_watermark: Option<i64>,
    /// Always None: downsampling is not implemented.
    pub downsampling_watermark: Option<i64>,
    /// E7 cumulative input-point/event counters, independent of current representation rows.
    pub cumulative: crate::StreamCounters,
    /// Persistent work counts, without calculating what is due.
    pub pending: StreamWork,
}

/// Persistent work visible without a retention policy or clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamWork {
    /// Manifest chunks at or below the expiration watermark, no longer visible to queries.
    pub cleanup_chunks: u64,
    /// Fragment bytes of those expired chunks; excludes journal bytes already deleted.
    pub cleanup_bytes: u64,
    /// Unexpired L0 chunks, which may need consolidation or merging later.
    pub l0_chunks: u64,
    /// Stored crowding marks, including marks not yet removed by cleanup.
    pub crowding_marks: u64,
    /// Stored sorted-run index records, including those pending expired-run cleanup.
    pub runs: u64,
}

/// One captured catalog's sizes; implicit overflow markers are excluded from live counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CatalogStatus {
    /// Every resident allocation reachable from this catalog version, shared or not.
    pub bytes: u64,
    /// Resident nonzero combinations, including any partially folded combinations.
    pub combinations: u64,
    /// Resident nonzero label values across all dimensions.
    pub label_values: u64,
    /// Resident nonzero combinations containing an overflow label. Reserved combo 0 is excluded.
    pub folded_combinations: u64,
    /// Whether the schema reserves the implicit whole-overflow combination 0.
    pub has_overflow_combination: bool,
    /// Distinct pinned combinations, including 0 when a host key pins it.
    pub pinned_combinations: u64,
    /// Host pin keys; several may pin the same combination.
    pub pin_keys: u64,
    /// Cumulative actual releases, from the E7 global record, counted once per lifetime.
    pub recycled_combinations: u64,
    /// Cumulative actual label-value releases from E7, across all streams.
    pub recycled_values: u64,
}

/// Maintenance observations, not a planner's prediction of rounds due.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaintenanceStatus {
    /// Owner backlog flag observed under the capture lock; ephemeral, reset on successful open.
    pub behind: bool,
    /// Staging gates off consolidation of at least one windowed event stream in this schema.
    pub event_consolidation_gated_off: bool,
    /// Persistent recycle candidates; candidates may prove stale or still referenced.
    pub recycle_candidates: u64,
}

/// Reading or admitting a diagnostic observation failed. No maintenance or mutation occurred.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum StatusError {
    /// Capturing the coherent storage/version pair failed.
    #[snafu(display("failed to capture status"))]
    Capture { source: CaptureError },
    /// Result arrays or a bounded head-row workspace did not fit the query budget.
    #[snafu(display("no query credit for status"))]
    Reserve { source: BudgetExceeded },
    /// Metadata or a revocation boundary failed at the storage port.
    #[snafu(display("failed to read status metadata"))]
    Storage { source: StorageError },
    /// The captured state record was unreadable.
    #[snafu(display("failed to read status state"))]
    State { source: MetaError },
    /// Manifest or persistent consolidation metadata was invalid or unreadable.
    #[snafu(display("failed to read status manifest"))]
    Manifest { source: ManifestError },
    /// E7 counters or ring history was unreadable.
    #[snafu(display("failed to read status diagnostics"))]
    Diagnostics { source: DiagnosticsError },
    /// Persistent recycling candidate keys were invalid or unreadable.
    #[snafu(display("failed to read recycle candidates"))]
    References { source: ReferenceError },
    /// Pins were inconsistent or unreadable.
    #[snafu(display("failed to read catalog pins"))]
    Pins { source: PinError },
    /// Metadata arithmetic, range, kind or publication disagrees with the captured version.
    #[snafu(display("status metadata disagrees with the captured version or stream"))]
    Invalid,
}

impl StatusError {
    /// Only storage failures of kind BackendFailed latch the shared recovery state.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Capture { source } => source.kind(),
            Self::Reserve { source } => source.kind(),
            Self::Storage { source } => source.kind(),
            Self::State { source } => source.kind(),
            Self::Manifest { source } => source.kind(),
            Self::Diagnostics { source } => source.kind(),
            Self::References { source } => source.kind(),
            Self::Pins { source } => source.kind(),
            Self::Invalid => ErrorKind::Corrupted,
        }
    }
}

/// A loss recorded in the transaction that caused it. The discriminants are the stored
/// variant indices; never reorder or reuse one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, FromRepr)]
#[repr(u8)]
pub enum LossKind {
    /// Input points or events rejected below the deletion watermark.
    Late = 0,
    /// Accepted input points or events whose series combination folded.
    Fold = 1,
    /// Accepted event cells resolved to the label overflow value.
    LabelOverflow = 2,
    /// Extents expired by age; age wins when both retention limits apply.
    DeleteAge = 3,
    /// Extents expired by the stream's byte cap.
    DeleteBytes = 4,
}

/// An inclusive bounding range; summaries may contain gaps. Extents are numbers, not times.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LossRange {
    /// Series bucket numbers in the record's tier.
    SeriesBuckets {
        /// First affected bucket.
        first: i64,
        /// Last affected bucket.
        last: i64,
    },
    /// Event keys in the record's stream.
    EventKeys {
        /// First affected key.
        first: i64,
        /// Last affected key.
        last: i64,
    },
    /// The expired extent of the record's tier or event stream.
    Extent(i64),
}

/// A transaction's loss, in input units except for retention's extents and stored bytes.
/// Totals use saturating addition; u64 input counts and field sums do not overflow in practice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LossDelta {
    /// Late or folded input points/events, or input events with overflowing label cells.
    /// Zero for retention: deletion does not count rows.
    pub rows: u64,
    /// Stored chunk and journal bytes expired by retention, including their keys.
    pub bytes: u64,
    /// Extents expired by retention.
    pub extents: u64,
    /// Input event cells resolved to the overflow label value.
    pub label_overflows: u64,
    /// Field values of accepted folded input series points, in schema order; unused tail zero.
    /// Mirrored events contribute no field values.
    pub folded_sums: [u64; MAX_FIELDS],
}

/// One of the newest 64 committed loss summaries. Identity is `(incarnation, seq)`:
/// recovery can roll back Consistent history and reuse a sequence in a new incarnation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LossRecord {
    /// Sequence in the surviving history, independent of its slot (`seq % 64`).
    pub seq: u64,
    /// Open incarnation that committed this operation.
    pub incarnation: u64,
    /// Publication that committed this operation.
    pub publication: u64,
    /// Caller's clock in milliseconds at the operation.
    pub at_ms: i64,
    /// Tier or event stream affected.
    pub stream: StreamRef,
    /// Why data was lost or labels degraded.
    pub kind: LossKind,
    /// Bounding range of the affected input, or the expired extent.
    pub range: LossRange,
    /// Loss within this transaction, rather than cumulative totals.
    pub delta: LossDelta,
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod bench_tests;
