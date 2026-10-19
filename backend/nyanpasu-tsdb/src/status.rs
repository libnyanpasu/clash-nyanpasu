//! Observability (§9.4). `status()` reports, per tier and per stream: rows, bytes, the oldest
//! and newest data, the deletion watermark, folded combinations, deletions by reason,
//! dropped late writes and the maintenance backlog. It also returns the ring of the last 64
//! lossy operations, each written in the same transaction as the operation itself.

use serde::{Deserialize, Serialize};
use strum::FromRepr;

use crate::{layout::manifest::StreamRef, schema::MAX_FIELDS};

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
