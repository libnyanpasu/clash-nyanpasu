//! zstd at level 1, one column block at a time, with contexts owned by the caller (§6.3
//! rules 3–5).
//!
//! - A block under 128 B stays uncompressed.
//! - The compressed form is kept only when its total, frame header included, is at most
//!   0.9 × the light-encoded size.
//! - The declared decompressed size is checked against the format limit before
//!   decompressing.
