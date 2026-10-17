//! Staging:
//!
//! - checks the batch id;
//! - looks up and registers catalog entries, with the fold ladder;
//! - reserves head space;
//! - turns sources into chunks in the same commit under memory pressure.
