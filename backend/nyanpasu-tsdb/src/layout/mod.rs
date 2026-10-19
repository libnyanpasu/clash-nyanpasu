//! How the store lays out its state in the backend: namespaces, key encodings, the record
//! encoding, metadata records and the chunk manifest. Every stored record carries a format
//! version (§0.3 rule 6).

pub(crate) mod diagnostics;
pub(crate) mod keys;
pub(crate) mod manifest;
pub(crate) mod meta;
pub(crate) mod record;
pub(crate) mod references;

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
/// What retention did to each tier and stream: stream → its deletion watermark (see
/// `manifest`).
pub(crate) const STREAMS: Ns = Ns(9);
/// Consolidation's bookkeeping of the tiers and event streams: (stream, kind, key) → the L0
/// indexes, the run index, the crowding marks and the progress of an extent (see `manifest`).
pub(crate) const CONSOLIDATION: Ns = Ns(10);
/// How many live combinations use each label value: (dimension, value id) → count (§5.3).
pub(crate) const LABEL_USES: Ns = Ns(11);
/// The newest extent of each stream that references a combination or a label value:
/// (entity, stream) → extent (see `references`).
pub(crate) const REFERENCES: Ns = Ns(12);
/// The same references by stream and extent: (stream, extent, entity), with an empty value.
pub(crate) const REFERENCE_INDEX: Ns = Ns(13);
/// The combinations and label values that may have become free: entity, with an empty value.
pub(crate) const RECYCLE_CANDIDATES: Ns = Ns(14);
/// Transactional cumulative counters and the 64-slot loss ring (see `diagnostics`).
pub(crate) const DIAGNOSTICS: Ns = Ns(15);
/// The consumer's auxiliary state, written in the same transaction as its batch (§4.1).
pub(crate) const AUX: Ns = Ns(16);
