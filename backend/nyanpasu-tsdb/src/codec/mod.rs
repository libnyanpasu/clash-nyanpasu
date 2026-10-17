//! Column encodings (§6.2) and compression (§6.3).
//!
//! - Everything is integer arithmetic. A SIMD fast path is allowed only behind runtime
//!   detection (§0.3 rule 3).
//! - Every encoded column block starts with its encoding and codec tags, so old and new
//!   codecs can coexist.

mod checksum;
mod compress;
mod dict;
mod int;
mod text;
mod varint;
