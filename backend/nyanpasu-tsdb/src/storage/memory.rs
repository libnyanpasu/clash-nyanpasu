//! The in-memory backend (§3.4): one `BTreeMap` per namespace, and fragments as `Vec<u8>`.
//! Every conformance and property test runs on it first.
//!
//! It models durability: a simulated crash returns to the last durable commit, so recovery
//! after lost `Consistent` commits can be tested without a disk.
