//! Retention (§9.1) works in two steps:
//!
//! 1. Expire: advance the deletion watermark and delete the extent's journal records.
//! 2. Clean up: delete at most D chunks per round.
//!
//! Byte caps come from byte accounting, not file sizes.
