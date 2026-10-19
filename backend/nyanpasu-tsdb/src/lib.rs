//! Time-series storage for statistics and logs: a label catalog, pre-aggregated series
//! tiers and append-only event streams, over a replaceable storage backend.
//!
//! The design is `docs/design/nyanpasu-tsdb.md`; "§n" in this crate's docs refers to its
//! sections. Consumers own the business semantics (reports, topology, log previews) and
//! drive every write and maintenance round from their own actor: the crate starts no
//! thread, timer or pool and holds no global state (§3.1).
//!
//! The owner holds a [`Tsdb`](db) handle and stages and commits batches through it;
//! queries run outside the owner on snapshots captured through a shared reader.

// Every supported platform is 64-bit little-endian. Rejecting the rest at compile time
// turns that assumption into a rule: the chunk format reads fixed-width little-endian
// columns in place, and 64-bit atomics hold counters (design §0.3 rule 4).
#[cfg(not(all(target_pointer_width = "64", target_endian = "little")))]
compile_error!("nyanpasu-tsdb supports 64-bit little-endian targets only");

mod budget;
mod catalog;
mod chunk;
mod codec;
mod db;
mod error;
mod head;
mod layout;
mod lifecycle;
mod maintain;
mod profile;
mod query;
mod retention;
mod schema;
mod status;
pub mod storage;
mod time;
#[cfg(test)]
mod verification;
mod version;
mod write;

pub use budget::{BudgetExceeded, MemoryBudget, Reservation};
pub use db::{Reader, Tsdb};
pub use error::ErrorKind;
pub use layout::{
    diagnostics::{Deletions, StreamCounters},
    manifest::StreamRef,
};
pub use lifecycle::{
    CheckpointError, CompactError, CompactStorage, Compaction, OpenError, RecoveryError,
    RecoveryProgress, RecoveryView,
};
pub use maintain::{Maintain, MaintainError, Maintained};
pub use profile::{Profile, ProfileError};
pub use query::{
    Event, EventCondition, EventCursor, EventDegradation, EventFilter, EventPage, EventPredicate,
    EventQuery, EventScanError, EventValue, FieldMask, Group, GroupKey, GroupList, GroupRequest,
    KeyPart, LabelFilter, LabelMap, SeriesGroups, SeriesQuery, SeriesQueryError,
};
pub use retention::{Retention, RetentionPolicy};
pub use schema::{
    Aggregate, ColumnSpec, ColumnType, EventKey, EventStreamSpec, ExtentSpec, FieldSpec, LabelId,
    LabelSpec, NameScope, Schema, SchemaError, SeriesSpec, StreamId, TierId, TierSpec,
};
pub use status::{
    CatalogStatus, LossDelta, LossKind, LossRange, LossRecord, MaintenanceStatus, Status,
    StatusData, StatusError, StreamStatus, StreamWork,
};
pub use time::{DurationError, ReversedRange, TimeRange, Timestamp};
pub use version::{CaptureError, CatalogToken, LifecycleState, Snapshot};
pub use write::{
    AuxOp, ComboId, EventRow, EventSeq, Input, InputError, MAX_PIN_KEY_LEN, PinOp, SeriesPoint,
    StageError, Value, WriteError, Written,
};
