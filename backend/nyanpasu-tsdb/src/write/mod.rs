//! The write path (§4.4). A batch is one transaction.
//!
//! `stage` encodes the batch into the transaction and changes no shared state. A batch
//! contains:
//!
//! - journal records;
//! - catalog deltas;
//! - auxiliary state;
//! - metadata;
//! - any chunking the batch triggers.
//!
//! `commit` registers the pending version, commits without holding the registry lock, then
//! publishes.

mod chunking;
mod commit;
mod stage;
