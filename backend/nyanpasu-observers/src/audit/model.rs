use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use nyanpasu_tsdb::EventCursor;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Maximum rows removed from the diagnostic channel by one tick.
pub const AUDIT_BATCH_ROWS: usize = 1024;
pub(super) const MAX_PAGE_ROWS: usize = 500;

/// An owned terminal HTTP/local audit record, excluding arguments and results.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AuditRow {
    /// Milliseconds since the Unix epoch, sampled by the sink.
    pub at: i64,
    /// `http` or `local`.
    pub transport: String,
    /// The fixed method name from the RPC header.
    pub method: String,
    /// Instance-keyed caller hash; meaningful only within this process run.
    pub owner: String,
    /// Invocation identity within this process run.
    pub invocation_id: u64,
    /// The local transport's request identity, when supplied.
    pub request_id: Option<u64>,
    /// `rejected`, `finished` or `subscription`.
    pub phase: String,
    /// The tracing sink's outcome spelling, absent for rejections.
    pub outcome: Option<String>,
    /// Error classification, when supplied by the audit kernel.
    pub kind: Option<String>,
    /// Error code, truncated by the sink to at most 128 UTF-8 bytes.
    pub code: Option<String>,
    /// Monotonic elapsed time in nanoseconds.
    pub duration_ns: u64,
}

/// Exact stored transport/outcome matches; absent fields do not restrict a scan.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AuditFilter {
    /// The stored transport spelling, or all transports.
    pub transport: Option<String>,
    /// The stored outcome spelling, or all outcomes.
    pub outcome: Option<String>,
}

/// Opaque exclusive position for newest-first paging. Consumers pass it back
/// unchanged; only the store adapter converts it to/from a tsdb cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AuditCursor {
    epoch: u64,
    key: i64,
    incarnation: u64,
    seq: u64,
}

impl From<EventCursor> for AuditCursor {
    fn from(cursor: EventCursor) -> Self {
        Self {
            epoch: cursor.epoch,
            key: cursor.key,
            incarnation: cursor.incarnation,
            seq: cursor.seq,
        }
    }
}

impl From<AuditCursor> for EventCursor {
    fn from(cursor: AuditCursor) -> Self {
        Self {
            epoch: cursor.epoch,
            key: cursor.key,
            incarnation: cursor.incarnation,
            seq: cursor.seq,
        }
    }
}

/// Persisted rows in descending event order and the process-run loss count.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AuditPage {
    /// At most 500 matching records, newest first.
    pub rows: Vec<AuditRow>,
    /// Pass back as `before` to continue toward older records.
    pub next: Option<AuditCursor>,
    /// Rows not saved during this run, including channel overflow and late rows.
    pub dropped: u64,
}

/// One process-run diagnostic loss counter shared by the sink and audit owner.
#[derive(Clone, Debug, Default)]
pub struct AuditDropped(Arc<AtomicU64>);

impl AuditDropped {
    /// Counts one row rejected by a full diagnostic channel.
    pub fn increment(&self) {
        self.add(1);
    }

    /// Counts late rows or rows in a failed batch.
    pub(super) fn add(&self, rows: u64) {
        // Shared ownership is required: the synchronous sink and serial owner
        // both contribute losses. This counter publishes no other state.
        self.0.fetch_add(rows, Ordering::Relaxed);
    }

    /// Returns the current run's best-effort count without synchronizing storage.
    #[must_use]
    pub fn count(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}
