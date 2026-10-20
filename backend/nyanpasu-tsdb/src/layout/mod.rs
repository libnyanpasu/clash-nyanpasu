//! How the store lays out its state in the backend: namespaces, key encodings, the record
//! encoding, metadata records and the chunk manifest. Every stored record carries a format
//! version (§0.3 rule 6).

pub(crate) mod keys;
pub(crate) mod manifest;
pub(crate) mod meta;
pub(crate) mod record;

use crate::storage::Ns;

/// Metadata records: the dataset's format and its state (see `meta`).
pub(crate) const META: Ns = Ns(0);
/// Label values: (dimension, value id) → value.
pub(crate) const LABELS: Ns = Ns(1);
/// Combinations: combination id → label ids.
pub(crate) const COMBOS: Ns = Ns(2);
/// The reverse lookup of combinations: label ids → combination id.
pub(crate) const COMBO_INDEX: Ns = Ns(3);
/// Pinned combinations: combination id → pin count.
pub(crate) const PINS: Ns = Ns(4);
/// Journal records not yet in chunks: source and journal sequence → deltas (§4.2).
pub(crate) const JOURNAL: Ns = Ns(5);
/// The chunk manifest: (stream, extent, chunk id) → chunk record (see `manifest`).
pub(crate) const MANIFEST: Ns = Ns(6);
/// The extents of each tier and stream: (stream, extent) → its chunk and journal bytes.
pub(crate) const EXTENTS: Ns = Ns(7);
/// The consumer's pin keys: pin key → the combination pinned under it (§5.3).
pub(crate) const PIN_KEYS: Ns = Ns(8);
// 9 to 15 are free for the store's later namespaces.
/// The consumer's auxiliary state, written in the same transaction as its batch (§4.1).
pub(crate) const AUX: Ns = Ns(16);
