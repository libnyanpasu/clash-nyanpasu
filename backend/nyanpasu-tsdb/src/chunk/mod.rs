//! Chunks (§4.3, §6.1): immutable groups of columns, the unit of writing.
//!
//! - L0 chunks come from journal records and are only lightly encoded.
//! - Consolidated chunks come from all L0 chunks of an extent, with their column blocks
//!   compressed.
//! - L0 base and delta chunks share one format; only the header tells them apart.
//!
//! The format limits (§6.5) are constants of the format version, not profile values.

mod builder;
mod format;
mod reader;
