//! How the catalog is stored.
//!
//! - Delta records are written with each commit and merged into catalog chunks.
//! - The reverse lookup (label ids → combination id) lives in storage, not in memory.
//! - Pins record combinations referenced by the consumer's auxiliary state (§5.3).
