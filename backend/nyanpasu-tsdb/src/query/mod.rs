//! Queries (§8) run outside the owner, on a captured snapshot.
//!
//! - Series queries are grouped sums over a time range.
//! - Event queries are paged scans of a stream.
//!
//! A query's temporary memory is O(groups + decode buffers). It is reserved from a shared
//! budget before the query starts (§7.4, §8.2).
//!
//! Results report their resolution, whether folded data was involved, and whether retention
//! truncated the range (§9.4).

mod events;
mod plan;
mod series;
mod snapshot;
