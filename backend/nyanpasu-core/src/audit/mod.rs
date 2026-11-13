//! Synchronous tsdb adapter for the observers audit owner. The shell opens the
//! private dataset on a blocking thread and injects its unique store into core.

mod schema;
mod storage;
mod store;
#[cfg(test)]
mod tests;

pub use storage::{Error as OpenError, open_audit_store};
pub use store::TsdbAuditStore;
