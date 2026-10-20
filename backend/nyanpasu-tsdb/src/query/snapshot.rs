//! Snapshots. A snapshot holds:
//!
//! - the storage snapshot, plus the catalog and head of one publication;
//! - a budget reservation sized to the bytes of that head and catalog at capture.
//!
//! Scans check for revocation at the boundary of each column block and row batch.
