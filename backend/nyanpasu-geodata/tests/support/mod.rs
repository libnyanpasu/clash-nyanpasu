#![allow(dead_code)]

pub mod mmdb;
pub mod proto;

use std::io::Write;

/// `bytes` written to a temporary file and mapped back read-only, the way a
/// cache opens a stored index.
pub fn mapped(bytes: &[u8]) -> memmap2::Mmap {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(bytes).unwrap();
    // SAFETY: the file is private to this process and never written again.
    unsafe { memmap2::Mmap::map(&file) }.unwrap()
}
