//! Serial audit persistence and queries over an injected store. The owner drains
//! the sink's bounded channel on each tick; storage failure never affects RPC work.

mod actor;
mod client;
mod model;
mod ports;
#[cfg(test)]
mod tests;

pub use actor::AuditArgs;
pub use client::{AuditClient, Error as SpawnError};
pub use model::{AUDIT_BATCH_ROWS, AuditCursor, AuditDropped, AuditFilter, AuditPage, AuditRow};
pub use ports::{Appended, AuditRetention, AuditStore, Error as AuditStoreError};
