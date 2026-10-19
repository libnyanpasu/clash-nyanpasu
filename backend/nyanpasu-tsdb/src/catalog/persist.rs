//! How the catalog is stored. A draft writes the records of its changes into the write
//! transaction, and loading rebuilds a version from them.
//!
//! | Namespace     | Key                             | Value            |
//! | ------------- | ------------------------------- | ---------------- |
//! | `LABELS`      | dimension `u16`, value id `u32` | [`LabelRecord`]  |
//! | `COMBOS`      | combination id `u32`            | [`ComboRecord`]  |
//! | `COMBO_INDEX` | one `u32` per series dimension  | [`IndexRecord`]  |
//! | `PINS`        | combination id `u32`            | [`PinRecord`]    |
//! | `PIN_KEYS`    | the consumer's pin key          | [`PinKeyRecord`] |
//! | `LABEL_USES`  | dimension `u16`, value id `u32` | [`UsesRecord`]   |
//!
//! Numeric keys are big-endian, so they sort by their numbers. A value is its [`Format`]
//! byte, then the record in postcard, a specified and stable wire format (§0.3 rule 6). The
//! overflow marker and combination 0 are implicit and never stored.
//!
//! - The reverse lookup (label ids → combination id) lives in storage, not in memory
//!   (§5.1); the write path reads it through its write transaction.
//! - Pins record combinations the consumer's auxiliary state references (§5.3). The consumer
//!   names each pin with a key of its own, which its auxiliary state can hold in the same
//!   transaction; the key maps to the combination, and the combination counts its keys.
//!   Pins live in storage only, so they cost the catalog budget nothing; maintenance reads
//!   the counts in its transaction.
//! - Each label value counts the live combinations that use it (§5.3): a new combination adds
//!   one use to each of its values, and a released one takes it off. The counts live in
//!   storage only, so a new combination copies no page for them; maintenance reads them before
//!   it releases a value.
//! - Loading reads every label value and combination record; it reads neither the reverse
//!   lookup nor the pins. It sizes the version from a first pass over the label values and
//!   admits all of it against the catalog budget before it allocates any, then copies each
//!   value once (§5.1). So it never holds more than the budget, and a catalog written under
//!   a larger one fails to load instead of outgrowing a smaller one; one written under the
//!   budget loads under it (see [`load`]).
//!
//! Merging these records into catalog chunks (§5.1) is deferred.

use std::{borrow::Cow, ops::ControlFlow};

use serde::{Deserialize, Deserializer, Serialize};
use snafu::{OptionExt, ResultExt, ensure};

use super::{
    Inner,
    combos::Combos,
    dictionary::{Dictionary, LoadPlan, ValueHasher},
    error::{
        CatalogProblem, ComboKeySnafu, DeleteKeySnafu, DeleteRecordSnafu, DeleteUsesSnafu,
        InvalidComboSnafu, InvalidKeySnafu, InvalidLabelSnafu, InvalidRecordSnafu,
        InvalidUsesSnafu, LabelKeySnafu, LoadError, PinError, ReadCountSnafu, ReadKeySnafu,
        ReadUsesSnafu, ReserveMemorySnafu, ResolveError, ScanCombosSnafu, ScanLabelsSnafu,
        StageResolutionSnafu, UsesError, UsesOverflowSnafu, UsesUnderflowSnafu, WriteCountSnafu,
        WriteKeySnafu, WriteLabelSnafu, WriteUsesSnafu, ZeroPinsSnafu, ZeroUsesSnafu,
    },
    ids::MAX_ID,
    paged::Ledger,
};
use crate::{
    LabelId, Schema,
    budget::{BudgetExceeded, MemoryAccount},
    layout::{
        COMBO_INDEX, COMBOS, LABEL_USES, LABELS, PIN_KEYS, PINS,
        keys::{KeyBuf, KeyError, KeyReader},
        record::{self, bounded, encode},
    },
    schema::MAX_DIMENSIONS,
    storage::{Direction, KeyRange, StorageError, StorageRead, StorageWrite},
};

/// A label value: its canonical bytes (§2).
#[derive(Serialize, Deserialize)]
struct LabelRecord<'a> {
    value: &'a [u8],
}

/// A combination: one label id per series dimension, in key order.
#[derive(Serialize, Deserialize)]
struct ComboRecord<'a> {
    #[serde(deserialize_with = "combo_labels")]
    labels: Cow<'a, [u32]>,
}

/// The labels of a [`ComboRecord`]: at most [`MAX_DIMENSIONS`], the most a valid schema has,
/// so a damaged record allocates nothing past them before loading compares the count with
/// the schema's.
fn combo_labels<'de, 'a, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Cow<'a, [u32]>, D::Error> {
    bounded::<_, _, MAX_DIMENSIONS>(deserializer).map(Cow::Owned)
}

/// The reverse lookup's answer for the label ids in the key.
#[derive(Serialize, Deserialize)]
struct IndexRecord {
    combo: u32,
}

/// How many pin keys pin a combination; never 0, which deletes the record.
#[derive(Serialize, Deserialize)]
struct PinRecord {
    count: u64,
}

/// The combination a pin key pins. Combination 0 has no count, but its keys are kept like
/// any other.
#[derive(Serialize, Deserialize)]
struct PinKeyRecord {
    combo: u32,
}

/// How many live combinations use a label value; never 0, which deletes the record.
#[derive(Serialize, Deserialize)]
struct UsesRecord {
    count: u64,
}

/// The record a value holds, its encoding's problems as the catalog's.
fn decode<'a, T: Deserialize<'a>>(value: &'a [u8]) -> Result<T, CatalogProblem> {
    record::decode(value).map_err(|problem| CatalogProblem::Encoding { problem })
}

pub(super) fn label_key(dimension: LabelId, id: u32) -> KeyBuf {
    KeyBuf::new().u16(dimension.0).u32(id)
}

pub(super) fn combo_key(id: u32) -> KeyBuf {
    KeyBuf::new().u32(id)
}

pub(super) fn pin_key(id: u32) -> KeyBuf {
    KeyBuf::new().u32(id)
}

/// The most series dimensions a reverse-lookup key holds: one `u32` each. The schema allows
/// at most [`MAX_DIMENSIONS`], which fill a [`KeyBuf`] exactly.
pub(super) const MAX_KEY_DIMENSIONS: usize = KeyBuf::CAPACITY / size_of::<u32>();

const _: () = assert!(MAX_DIMENSIONS <= MAX_KEY_DIMENSIONS);

/// The reverse-lookup key of `labels`: one component per series dimension, in key order.
///
/// # Panics
///
/// If there are more than [`MAX_KEY_DIMENSIONS`] labels, which a valid schema rules out.
pub(super) fn index_key(labels: &[u32]) -> KeyBuf {
    labels
        .iter()
        .fold(KeyBuf::new(), |key, &label| key.u32(label))
}

/// Writes a label through exact charged output, before any catalog page is changed.
pub(super) fn put_label(
    batch: &mut impl StorageWrite,
    dimension: LabelId,
    id: u32,
    value: &[u8],
    account: &MemoryAccount,
) -> Result<(), ResolveError> {
    let record =
        record::encode_within(account, &LabelRecord { value }).context(StageResolutionSnafu)?;
    batch
        .put(LABELS, label_key(dimension, id).as_slice(), &record)
        .context(WriteLabelSnafu {
            dimension: dimension.0,
            id,
        })
}

pub(super) fn delete_label(
    batch: &mut impl StorageWrite,
    dimension: LabelId,
    id: u32,
) -> Result<(), StorageError> {
    batch.delete(LABELS, label_key(dimension, id).as_slice())
}

pub(super) fn put_combo(
    batch: &mut impl StorageWrite,
    id: u32,
    labels: &[u32],
) -> Result<(), StorageError> {
    let record = encode(&ComboRecord {
        labels: Cow::Borrowed(labels),
    });
    batch.put(COMBOS, combo_key(id).as_slice(), &record)
}

pub(super) fn delete_combo(batch: &mut impl StorageWrite, id: u32) -> Result<(), StorageError> {
    batch.delete(COMBOS, combo_key(id).as_slice())
}

pub(super) fn put_index(
    batch: &mut impl StorageWrite,
    key: &[u8],
    id: u32,
) -> Result<(), StorageError> {
    batch.put(COMBO_INDEX, key, &encode(&IndexRecord { combo: id }))
}

pub(super) fn delete_index(batch: &mut impl StorageWrite, key: &[u8]) -> Result<(), StorageError> {
    batch.delete(COMBO_INDEX, key)
}

/// The combination the reverse lookup holds under `key`, if any.
pub(super) fn get_index(
    read: &impl StorageRead,
    key: &[u8],
) -> Result<Option<Result<u32, CatalogProblem>>, StorageError> {
    read.get(COMBO_INDEX, key, |value| {
        value.map(|value| {
            let IndexRecord { combo } = decode(value)?;
            if combo == 0 || combo > MAX_ID {
                return Err(CatalogProblem::ReservedId);
            }
            Ok(combo)
        })
    })
}

/// How many times combination `id` is pinned. A stored count of 0 is damage
/// ([`PinError::ZeroPins`]): read as 0, it would let maintenance release a combination the
/// consumer's auxiliary state still references.
pub(crate) fn pin_count(read: &impl StorageRead, id: u32) -> Result<u64, PinError> {
    let count = read
        .get(PINS, pin_key(id).as_slice(), |value| {
            value.map(|value| decode(value).map(|PinRecord { count }| count))
        })
        .context(ReadCountSnafu { id })?;
    match count {
        None => Ok(0),
        Some(Ok(0)) => ZeroPinsSnafu { id }.fail(),
        Some(Ok(count)) => Ok(count),
        Some(Err(problem)) => InvalidRecordSnafu { id, problem }.fail(),
    }
}

/// Whether combination `id` is pinned, in the state `read` sees.
pub(crate) fn is_pinned(read: &impl StorageRead, id: u32) -> Result<bool, PinError> {
    Ok(pin_count(read, id)? > 0)
}

/// Distinct pinned combinations (including 0) and pin keys, from one captured view.
/// Streams fixed pin records without collecting ids or copying key bytes.
pub(crate) fn pin_totals(
    read: &impl StorageRead,
    known: impl Fn(u32) -> bool,
) -> Result<(u64, u64), PinError> {
    let (mut combos, mut counted, mut keys, mut overflow) = (0u64, 0u64, 0u64, 0u64);
    let bad = read
        .scan(PINS, KeyRange::All, Direction::Forward, |key, value| {
            let valid = || {
                let id = u32::from_be_bytes(key.try_into().ok()?);
                let record: PinRecord = decode(value).ok()?;
                (id > 0 && known(id) && record.count > 0).then_some(record.count)
            };
            match valid().and_then(|n| counted.checked_add(n)) {
                Some(total) => {
                    counted = total;
                    combos += 1;
                    ControlFlow::Continue(())
                }
                None => ControlFlow::Break(()),
            }
        })
        .context(super::error::ScanPinsSnafu)?;
    ensure!(bad.is_none(), super::error::InvalidPinsSnafu);
    let bad = read
        .scan(
            PIN_KEYS,
            KeyRange::All,
            Direction::Forward,
            |_, value| match decode::<PinKeyRecord>(value) {
                Ok(record) if known(record.combo) => {
                    keys += 1;
                    overflow += u64::from(record.combo == 0);
                    ControlFlow::Continue(())
                }
                _ => ControlFlow::Break(()),
            },
        )
        .context(super::error::ScanPinsSnafu)?;
    ensure!(
        bad.is_none() && counted.checked_add(overflow) == Some(keys),
        super::error::InvalidPinsSnafu
    );
    Ok((combos + u64::from(overflow > 0), keys))
}

/// The combination pinned under the consumer's `key`, if any.
pub(crate) fn pinned_combo(read: &impl StorageRead, key: &[u8]) -> Result<Option<u32>, PinError> {
    let combo = read
        .get(PIN_KEYS, key, |value| {
            value.map(|value| {
                let PinKeyRecord { combo } = decode(value)?;
                if combo > MAX_ID {
                    return Err(CatalogProblem::ReservedId);
                }
                Ok(combo)
            })
        })
        .context(ReadKeySnafu { key })?;
    combo
        .transpose()
        .map_err(|problem| InvalidKeySnafu { key, problem }.build())
}

pub(super) fn put_pin_key(
    batch: &mut impl StorageWrite,
    key: &[u8],
    id: u32,
) -> Result<(), PinError> {
    batch
        .put(PIN_KEYS, key, &encode(&PinKeyRecord { combo: id }))
        .context(WriteKeySnafu { key })
}

pub(super) fn delete_pin_key(batch: &mut impl StorageWrite, key: &[u8]) -> Result<(), PinError> {
    batch.delete(PIN_KEYS, key).context(DeleteKeySnafu { key })
}

/// Stores the pin count of `id`; a count of 0 deletes the record.
pub(super) fn put_pin_count(
    batch: &mut impl StorageWrite,
    id: u32,
    count: u64,
) -> Result<(), PinError> {
    let key = pin_key(id);
    if count == 0 {
        return batch
            .delete(PINS, key.as_slice())
            .context(DeleteRecordSnafu { id });
    }
    batch
        .put(PINS, key.as_slice(), &encode(&PinRecord { count }))
        .context(WriteCountSnafu { id })
}

/// How many live combinations use value `id` of `dimension`, in the state `read` sees. A
/// stored count of 0 is damage ([`UsesError::ZeroUses`]): read as 0, it would let maintenance
/// release a value a live combination uses.
pub(crate) fn label_uses(
    read: &impl StorageRead,
    dimension: LabelId,
    id: u32,
) -> Result<u64, UsesError> {
    let dimension_id = dimension.0;
    let count = read
        .get(LABEL_USES, label_key(dimension, id).as_slice(), |value| {
            value.map(|value| decode(value).map(|UsesRecord { count }| count))
        })
        .context(ReadUsesSnafu {
            dimension: dimension_id,
            id,
        })?;
    match count {
        None => Ok(0),
        Some(Ok(0)) => ZeroUsesSnafu {
            dimension: dimension_id,
            id,
        }
        .fail(),
        Some(Ok(count)) => Ok(count),
        Some(Err(problem)) => InvalidUsesSnafu {
            dimension: dimension_id,
            id,
            problem,
        }
        .fail(),
    }
}

/// Adds the use of a new combination to value `id` of `dimension`.
pub(super) fn add_use(
    batch: &mut impl StorageWrite,
    dimension: LabelId,
    id: u32,
) -> Result<(), UsesError> {
    let count = label_uses(&*batch, dimension, id)?
        .checked_add(1)
        .context(UsesOverflowSnafu {
            dimension: dimension.0,
            id,
        })?;
    put_uses(batch, dimension, id, count)
}

/// Takes the use of a released combination off value `id` of `dimension`, and returns the
/// uses left.
pub(super) fn remove_use(
    batch: &mut impl StorageWrite,
    dimension: LabelId,
    id: u32,
) -> Result<u64, UsesError> {
    let count = label_uses(&*batch, dimension, id)?
        .checked_sub(1)
        .context(UsesUnderflowSnafu {
            dimension: dimension.0,
            id,
        })?;
    put_uses(batch, dimension, id, count)?;
    Ok(count)
}

/// Stores the uses of value `id` of `dimension`; a count of 0 deletes the record.
fn put_uses(
    batch: &mut impl StorageWrite,
    dimension: LabelId,
    id: u32,
    count: u64,
) -> Result<(), UsesError> {
    let key = label_key(dimension, id);
    let dimension = dimension.0;
    if count == 0 {
        return batch
            .delete(LABEL_USES, key.as_slice())
            .context(DeleteUsesSnafu { dimension, id });
    }
    batch
        .put(LABEL_USES, key.as_slice(), &encode(&UsesRecord { count }))
        .context(WriteUsesSnafu { dimension, id })
}

/// A version holding every label value and combination `read` holds, its values hashed by
/// `hasher`, built in two passes.
///
/// The first pass counts what the version takes (a [`LoadPlan`] per dictionary, and the
/// highest combination id), holding a few numbers per dimension and nothing per value, and
/// admits it against `limit` before anything past the version's own structure is
/// allocated. The structure itself, the part [`Inner::fixed_bytes`] counts, is counted as
/// held, as a draft counts its copy of it; an empty catalog, which allocates nothing past
/// it, loads whatever the budget, as [`Catalog::new`](super::Catalog::new) makes it. The
/// second pass stores the records in place, allocating nothing.
///
/// So `ledger` ends holding exactly the version's [`bytes`](Inner::bytes), with no transient
/// copy, and the version is no larger than any version holding the same records (see
/// [`LoadPlan`] and [`Combos::slots_cost`]): a catalog written under `limit` loads under it.
/// One that does not fit fails with an error of kind `Local`.
pub(super) fn load(
    read: &impl StorageRead,
    schema: &Schema,
    hasher: ValueHasher,
    limit: u64,
    ledger: &mut Ledger,
) -> Result<Inner, LoadError> {
    let plans = plan_labels(read, schema.labels.len())?;
    let combos = highest_combo(read, schema)?;
    let mut inner = Inner::empty(schema, hasher);
    ledger
        .alloc(inner.fixed_bytes())
        .context(ReserveMemorySnafu)?;
    let dimensions = inner.combos.dimensions();
    let needed = plans
        .iter()
        .map(Dictionary::sized_cost)
        .fold(Combos::slots_cost(dimensions, combos), u64::saturating_add);
    if needed > 0 {
        #[cfg(test)]
        if let Some(faults) = read.fault_schedule()
            && !faults.admission("open.catalog.admit")
        {
            return Err(BudgetExceeded {
                what: "catalog load",
                needed,
                available: 0,
            })
            .context(ReserveMemorySnafu);
        }
        admit(ledger, needed, limit).context(ReserveMemorySnafu)?;
    }
    let before = ledger.allocated();
    for (dictionary, plan) in inner.dictionaries.iter_mut().zip(&plans) {
        *dictionary = Dictionary::sized(plan, ledger);
    }
    inner.combos = Combos::free(dimensions, combos, ledger);
    debug_assert_eq!(ledger.allocated() - before, needed, "predicted cost");

    let broken = read
        .scan(
            LABELS,
            KeyRange::All,
            Direction::Forward,
            |key, value| match load_label(&mut inner, key, value, ledger) {
                Ok(()) => ControlFlow::Continue(()),
                Err(error) => ControlFlow::Break(error),
            },
        )
        .context(ScanLabelsSnafu)?;
    if let Some(error) = broken {
        return Err(error);
    }

    let broken = read
        .scan(
            COMBOS,
            KeyRange::All,
            Direction::Forward,
            |key, value| match load_combo(&mut inner, key, value, ledger) {
                Ok(()) => ControlFlow::Continue(()),
                Err(error) => ControlFlow::Break(error),
            },
        )
        .context(ScanCombosSnafu)?;
    if let Some(error) = broken {
        return Err(error);
    }
    debug_assert_eq!(
        ledger.allocated() - before,
        needed,
        "loading allocated more"
    );
    Ok(inner)
}

/// What the label values of each of `labels` dimensions take once loaded, from one pass over
/// them in key order: by dimension, then by id.
fn plan_labels(read: &impl StorageRead, labels: usize) -> Result<Vec<LoadPlan>, LoadError> {
    let mut plans = vec![LoadPlan::default(); labels];
    let broken = read
        .scan(
            LABELS,
            KeyRange::All,
            Direction::Forward,
            |key, value| match plan_label(&mut plans, key, value) {
                Ok(()) => ControlFlow::Continue(()),
                Err(error) => ControlFlow::Break(error),
            },
        )
        .context(ScanLabelsSnafu)?;
    match broken {
        Some(error) => Err(error),
        None => Ok(plans),
    }
}

/// Counts a label record in the plan of its dimension.
fn plan_label(plans: &mut [LoadPlan], key: &[u8], value: &[u8]) -> Result<(), LoadError> {
    let (dimension, id, value) = label_entry(key, value, plans.len())?;
    plans[usize::from(dimension)]
        .add(id, value.len())
        .map_err(|problem| {
            InvalidLabelSnafu {
                dimension,
                id,
                problem,
            }
            .build()
        })
}

/// The highest combination id stored, or 0 for none. Keys sort by id, so it is the last.
fn highest_combo(read: &impl StorageRead, schema: &Schema) -> Result<u32, LoadError> {
    // Without a series, any combination record is invalid, and loading it says so.
    if schema.series.is_none() {
        return Ok(0);
    }
    let last = read
        .scan(COMBOS, KeyRange::All, Direction::Backward, |key, _| {
            ControlFlow::Break(combo_key_id(key).context(ComboKeySnafu { key }))
        })
        .context(ScanCombosSnafu)?;
    let id = last.transpose()?.unwrap_or(0);
    ensure!(
        id <= MAX_ID,
        InvalidComboSnafu {
            id,
            problem: CatalogProblem::ReservedId
        }
    );
    Ok(id)
}

/// The dimension, id and value of a label record, checked as far as the record alone allows,
/// for a schema with `labels` label dimensions.
fn label_entry<'a>(
    key: &[u8],
    value: &'a [u8],
    labels: usize,
) -> Result<(u16, u32, &'a [u8]), LoadError> {
    let (dimension, id) = label_key_parts(key).context(LabelKeySnafu { key })?;
    let invalid = |problem| {
        InvalidLabelSnafu {
            dimension,
            id,
            problem,
        }
        .build()
    };
    if usize::from(dimension) >= labels {
        return Err(invalid(CatalogProblem::UnknownDimension { labels }));
    }
    if id == 0 || id > MAX_ID {
        return Err(invalid(CatalogProblem::ReservedId));
    }
    let LabelRecord { value } = decode(value).map_err(invalid)?;
    if u32::try_from(value.len()).is_err() {
        return Err(invalid(CatalogProblem::TooLong { len: value.len() }));
    }
    Ok((dimension, id, value))
}

fn load_label(
    inner: &mut Inner,
    key: &[u8],
    value: &[u8],
    ledger: &mut Ledger,
) -> Result<(), LoadError> {
    let Inner {
        shared,
        dictionaries,
        ..
    } = inner;
    let (dimension, id, value) = label_entry(key, value, dictionaries.len())?;
    let hash = shared.hasher.hash(value);
    let index = usize::from(dimension);
    dictionaries[index]
        .load(&shared.hasher, hash, id, value, ledger)
        .map_err(|problem| {
            InvalidLabelSnafu {
                dimension,
                id,
                problem,
            }
            .build()
        })
}

/// Checks that allocating `cost` more bytes keeps what loading holds within `limit`.
fn admit(ledger: &Ledger, cost: u64, limit: u64) -> Result<(), BudgetExceeded> {
    let held = ledger.owned();
    if held.saturating_add(cost) > limit {
        return Err(BudgetExceeded {
            what: "catalog",
            needed: cost,
            available: limit.saturating_sub(held),
        });
    }
    Ok(())
}

fn label_key_parts(key: &[u8]) -> Result<(u16, u32), KeyError> {
    let mut reader = KeyReader::new(key);
    let dimension = reader.u16()?;
    let id = reader.u32()?;
    reader.finish()?;
    Ok((dimension, id))
}

fn load_combo(
    inner: &mut Inner,
    key: &[u8],
    value: &[u8],
    ledger: &mut Ledger,
) -> Result<(), LoadError> {
    let id = combo_key_id(key).context(ComboKeySnafu { key })?;
    let invalid = |problem| InvalidComboSnafu { id, problem };
    if !inner.shared.series {
        return invalid(CatalogProblem::NoSeries).fail();
    }
    if id == 0 || id > MAX_ID {
        return invalid(CatalogProblem::ReservedId).fail();
    }
    let ComboRecord { labels } = decode(value).map_err(|problem| invalid(problem).build())?;
    let expected = inner.shared.dimensions.len();
    if labels.len() != expected {
        return invalid(CatalogProblem::LabelCount {
            count: labels.len(),
            expected,
        })
        .fail();
    }
    let dimensions = inner.shared.dimensions.iter();
    for (position, (&label, dimension)) in labels.iter().zip(dimensions).enumerate() {
        // A label is a value of its dimension's dictionary, or the overflow marker.
        if label != 0 && !inner.dictionaries[usize::from(dimension.0)].is_live(label) {
            return invalid(CatalogProblem::UnknownLabel { position, label }).fail();
        }
    }
    if labels.iter().all(|&label| label == 0) {
        return invalid(CatalogProblem::WholeOverflow).fail();
    }
    inner.combos.load(id, &labels, ledger);
    Ok(())
}

fn combo_key_id(key: &[u8]) -> Result<u32, KeyError> {
    let mut reader = KeyReader::new(key);
    let id = reader.u32()?;
    reader.finish()?;
    Ok(id)
}

#[cfg(test)]
pub(crate) mod raw {
    //! Record encodings for tests that write or inspect records directly.

    use super::*;

    pub(crate) fn combo_record(labels: &[u32]) -> Vec<u8> {
        encode(&ComboRecord {
            labels: Cow::Borrowed(labels),
        })
    }

    pub(crate) fn label_record(value: &[u8]) -> Vec<u8> {
        encode(&LabelRecord { value })
    }

    pub(crate) fn index_record(id: u32) -> Vec<u8> {
        encode(&IndexRecord { combo: id })
    }

    pub(crate) fn pin_record(count: u64) -> Vec<u8> {
        encode(&PinRecord { count })
    }

    pub(crate) fn pin_key_record(id: u32) -> Vec<u8> {
        encode(&PinKeyRecord { combo: id })
    }

    pub(crate) fn uses_record(count: u64) -> Vec<u8> {
        encode(&UsesRecord { count })
    }

    /// Stores `count` as the pins of combination `id`, as damage would: a count of 0 too, which
    /// `put_pin_count` deletes instead.
    pub(crate) fn put_pin_record(
        batch: &mut impl StorageWrite,
        id: u32,
        count: u64,
    ) -> Result<(), StorageError> {
        batch.put(PINS, pin_key(id).as_slice(), &pin_record(count))
    }

    /// Stores `count` as the uses of value `id` of `dimension`, as damage would: a count of 0
    /// too, which `put_uses` deletes instead.
    pub(crate) fn put_uses_record(
        batch: &mut impl StorageWrite,
        dimension: LabelId,
        id: u32,
        count: u64,
    ) -> Result<(), StorageError> {
        let key = label_key(dimension, id);
        batch.put(LABEL_USES, key.as_slice(), &uses_record(count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The records are stored, so their bytes must not change unnoticed: postcard writes
    // integers as LEB128 varints, and a sequence as its length, then its elements.
    #[test]
    fn records_have_a_fixed_encoding() {
        assert_eq!(encode(&LabelRecord { value: b"ab" }), [1, 2, b'a', b'b']);
        assert_eq!(
            encode(&ComboRecord {
                labels: Cow::Borrowed(&[1, 300, 0]),
            }),
            [1, 3, 1, 0xac, 0x02, 0]
        );
        assert_eq!(encode(&IndexRecord { combo: 128 }), [1, 0x80, 0x01]);
        assert_eq!(encode(&PinRecord { count: 2 }), [1, 2]);
        assert_eq!(encode(&PinKeyRecord { combo: 300 }), [1, 0xac, 0x02]);
        assert_eq!(encode(&UsesRecord { count: 300 }), [1, 0xac, 0x02]);
    }

    #[test]
    fn a_reverse_lookup_key_has_one_big_endian_component_per_dimension() {
        assert_eq!(
            index_key(&[1, 0x0203_0405]).as_slice(),
            [0, 0, 0, 1, 2, 3, 4, 5]
        );
        let full = [u32::MAX; MAX_KEY_DIMENSIONS];
        assert_eq!(index_key(&full).as_slice(), [0xff; KeyBuf::CAPACITY]);
    }
}
