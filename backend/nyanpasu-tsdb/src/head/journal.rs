//! Journal records: deltas not yet in chunks, appended to the journal namespace.
//!
//! - Record keys group records by source (a tier and bucket, or a stream). Chunking
//!   therefore deletes exactly the records it absorbs.
//! - Journal bytes have their own cap. Folding replaces a bucket's records with one summary
//!   record (§4.2).
