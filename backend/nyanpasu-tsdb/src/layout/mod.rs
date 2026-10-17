//! How the store lays out its state in the backend: namespaces, key encodings, the record
//! encoding, metadata records and the chunk manifest. Every stored record carries a format
//! version (§0.3 rule 6).

pub(crate) mod keys;
mod manifest;
mod meta;
pub(crate) mod record;

use crate::storage::Ns;

/// Label values: (dimension, value id) → value.
pub(crate) const LABELS: Ns = Ns(1);
/// Combinations: combination id → label ids.
pub(crate) const COMBOS: Ns = Ns(2);
/// The reverse lookup of combinations: label ids → combination id.
pub(crate) const COMBO_INDEX: Ns = Ns(3);
/// Pinned combinations: combination id → pin count.
pub(crate) const PINS: Ns = Ns(4);
/// The consumer's pin keys: pin key → the combination pinned under it (§5.3).
pub(crate) const PIN_KEYS: Ns = Ns(8);
