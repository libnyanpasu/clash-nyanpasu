//! The owner's lifecycle (§7.5, §9.3, §10.2):
//!
//! - open: increment the incarnation and commit it durably;
//! - recovery after an unknown commit or a failed backend: revoke snapshots, close by
//!   dropping the backend, wait until their reservations are returned, reopen, rebuild;
//! - the unavailable state, retried at the owner's next scheduled commit;
//! - suspend and resume;
//! - physical compaction, only once no snapshot is alive.
