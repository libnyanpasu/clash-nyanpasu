//! The byte layout (§6.1).
//!
//! - Fragment 0 holds the header, the column directory and the large-value directory.
//! - Fragments 1..n hold column blocks and large values. Each fragment carries a checksum.
//! - Column offsets are 8-byte aligned.
//!
//! Decoding checks every length, offset and count against the fragment and the format
//! limits before taking any typed view (§6.5).
