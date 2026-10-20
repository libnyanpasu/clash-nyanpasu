//! Turning journal records into L0 chunks (§4.2). Chunking is triggered when:
//!
//! - a bucket has closed and stayed quiet for Q;
//! - a source reaches its size limit;
//! - an event buffer reaches its limit;
//! - the head is under memory pressure.
//!
//! This module also folds the journal.
