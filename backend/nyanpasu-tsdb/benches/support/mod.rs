//! Crate-only benchmark components; no consumer or GUI dependency.

use super::tsdb;
#[cfg(feature = "redb")]
pub mod disk;
pub mod fixture;
pub mod output;
pub mod run;

#[cfg(feature = "redb")]
pub mod space;

/// Private stores stay on Cargo's target filesystem, including after an aborted run.
#[cfg(feature = "redb")]
pub fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}
