//! The recovery classification shared by every error type of the crate.
//!
//! Errors follow snafu's design: each module has its own error types, and a variant
//! names the step that failed, with that step's context, such as the fragment it read or
//! the stream whose cursor expired. One underlying error, such as a
//! [`StorageError`](crate::storage::StorageError), therefore appears under several
//! variants, one for each step it can interrupt. Each public operation returns the error
//! type of its own module, so a caller matches only the failures that operation can have.
//!
//! On top of the cause, every error type has a `kind()` that answers what the caller has
//! to do next (§7.5). Whether a batch took effect is a separate question again, answered
//! by [`CommitFailure`](crate::storage::CommitFailure) (§3.2).

/// What an error means for the caller (§7.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// Affects this operation only: invalid input, an exhausted budget, a revoked query.
    /// The backend stays usable.
    Local,
    /// The backend instance has failed and must be closed and reopened (§7.5). Any
    /// operation can report it, reads included.
    BackendFailed,
    /// Stored data failed validation.
    Corrupted,
}
