//! The streaming chunk builder (§3.3).
//!
//! - It encodes one column block at a time and writes each fragment as soon as the fragment
//!   is full.
//! - It ends the chunk before any limit would be exceeded: rows, bytes, or the size of the
//!   fragment-0 directory.
//! - Fragment 0 is written last.
