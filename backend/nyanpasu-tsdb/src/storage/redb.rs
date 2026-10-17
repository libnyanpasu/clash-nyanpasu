//! The redb backend (§3.4): one table per namespace, and fragments in a table keyed by
//! (chunk, index).
//!
//! - The page cache is always sized explicitly; redb's default is 1 GiB.
//! - Commit errors map to [`CommitFailure`](super::CommitFailure).
//!   `TransactionPoisoned` means the transaction rolled back. Any other error after the
//!   commit started may have persisted (§7.5).
//! - One I/O failure latches redb into `PreviousIo` until it is reopened, whichever
//!   operation hit it. Such errors map to `BackendFailed` on every operation, reads
//!   included (§3.2).
