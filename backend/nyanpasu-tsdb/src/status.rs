//! Observability (§9.4). `status()` reports, per tier and per stream: rows, bytes, the oldest
//! and newest data, the deletion watermark, folded combinations, deletions by reason,
//! dropped late writes and the maintenance backlog. It also returns the ring of the last 64
//! lossy operations, each written in the same transaction as the operation itself.
