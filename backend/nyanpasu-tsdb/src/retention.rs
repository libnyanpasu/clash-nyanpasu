//! Retention (§2, §9.1). How long to keep data is the application's choice, not the
//! store's: the consumer reads it from its configuration and passes a policy to each
//! maintenance call, so a changed setting needs no reopen.

use std::time::Duration;

/// One [`Retention`] per series tier and per event stream, indexed by
/// [`TierId`](crate::TierId) and [`StreamId`](crate::StreamId).
#[derive(Clone, Debug, Default)]
pub struct RetentionPolicy {
    pub tiers: Vec<Retention>,
    pub streams: Vec<Retention>,
}

/// Both limits may be set; kept data has to satisfy both, and exceeding either deletes
/// whole extents from the oldest (§9.1). Lengthening a limit never brings deleted data
/// back: deletion watermarks only move forward (§5.3 rule 3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Retention {
    /// `None` keeps data regardless of age.
    pub max_age: Option<Duration>,
    /// Counted from the bytes the store wrote for the data, not the file size (§9.1).
    /// `None` sets no cap.
    pub max_bytes: Option<u64>,
}
