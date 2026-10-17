//! How the store lays out its state in the backend: namespaces, key encodings, metadata
//! records and the chunk manifest. Every stored record carries a format version (§0.3 rule
//! 6).

mod keys;
mod manifest;
mod meta;
