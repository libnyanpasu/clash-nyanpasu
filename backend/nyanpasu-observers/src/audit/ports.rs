use std::time::{Duration, SystemTime};

use nyanpasu_tsdb::{ErrorKind, Retention};

use super::model::{AuditCursor, AuditFilter, AuditPage, AuditRow};

/// A successful append's rows rejected as late by storage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Appended {
    /// Rows older than the retained event window.
    pub late: u64,
}

/// Synchronous storage operations. The actor moves this unique owner into a
/// blocking task and awaits its return for every tick and query.
pub trait AuditStore: Send + 'static {
    /// Appends one batch in input order, assigning its batch ID and event sequences.
    ///
    /// # Errors
    /// Reports `Append`; unknown write outcomes must be `BackendFailed`.
    fn append(&mut self, rows: &[AuditRow]) -> Result<Appended, Error>;

    /// Runs storage maintenance and enforces both retention limits against wall time.
    ///
    /// # Errors
    /// Reports `Maintain`; unknown write outcomes must be `BackendFailed`.
    fn maintain(&mut self, now: SystemTime, retention: Retention) -> Result<(), Error>;

    /// Scans persisted records backward, exclusively before the cursor. The
    /// owner bounds `limit` to 1..=500 and supplies the returned loss count.
    ///
    /// # Errors
    /// Reports `Query`, including invalid/expired cursors and backend failure.
    fn query(
        &mut self,
        filter: AuditFilter,
        before: Option<AuditCursor>,
        limit: usize,
    ) -> Result<AuditPage, Error>;
}

/// Nonblocking retention snapshot, read afresh by the owner at every tick.
pub trait AuditRetention: Send + Sync + 'static {
    /// Maximum age and accounted storage bytes; both limits are enabled.
    fn retention(&self) -> (Duration, u64);
}

/// Failing storage step and its recovery classification. `BackendFailed`
/// includes unknown commit outcomes and disables this store until restart.
#[derive(Debug, snafu::Snafu)]
pub enum Error {
    /// The owner has stopped or retired its failed store.
    #[snafu(display("audit storage unavailable"))]
    AccessStore,
    /// The batch was refused or its write failed. It is never replayed.
    #[snafu(display("failed to append audit batch"))]
    Append {
        /// Storage recovery classification, including uncertain writes.
        kind: ErrorKind,
        /// Original adapter error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Commit or retention maintenance failed.
    #[snafu(display("failed to maintain audit storage"))]
    Maintain {
        /// Storage recovery classification, including uncertain writes.
        kind: ErrorKind,
        /// Original adapter error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The persisted page could not be scanned.
    #[snafu(display("failed to query audit storage"))]
    Query {
        /// Storage recovery classification.
        kind: ErrorKind,
        /// Original adapter error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl Error {
    /// Whether the failure retires the backend or affects only this operation.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::AccessStore => ErrorKind::Local,
            Self::Append { kind, .. } | Self::Maintain { kind, .. } | Self::Query { kind, .. } => {
                *kind
            }
        }
    }
}
