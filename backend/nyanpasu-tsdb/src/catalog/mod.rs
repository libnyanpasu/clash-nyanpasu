//! The catalog (§5): one dictionary per label, plus the table of combinations.
//!
//! - It is resident in memory as an immutable version per publication. The writer derives
//!   the next version through a [`CatalogDraft`], which copies only the pages it changes
//!   (§7.1). Old versions stay valid and unchanged for the snapshots that hold them.
//! - Its size is limited in bytes, counted at peak (§5.1).
//! - A new combination that does not fit goes down the fold ladder (§5.2).
//! - Its records go into the write transaction with each draft, and [`Catalog::load`]
//!   rebuilds a version from them (see `persist`).
//!
//! # Budget
//!
//! The limit is the profile's catalog budget, in bytes. A draft's peak is the base version's
//! [`bytes`](Catalog::bytes), which stay alive until the new version is published, plus
//! everything the draft allocated and still holds, at its highest:
//!
//! - its copy of the version's own structure (the vectors of dictionaries and columns);
//! - every spine and page it copied from the base, and every one it created;
//! - while a page, a spine or a hash table is replaced by a larger one, both copies.
//!
//! Not counted: the constant parts shared by every version of a store (the hasher, the
//! schema's dimensions and ladder, a few hundred bytes); a resolution's own workspace, a few
//! stack words per series dimension; allocator rounding past the 8-byte
//! alignment. A draft keeps nothing per resolution: it finds a combination through the
//! reverse lookup in the write transaction, which reads its own writes (§5.1).
//! The query budget counts the versions that snapshots keep alive (§7.4); the write path
//! reserves them there. Published catalogs carry no reservation. A draft's high-water staging
//! lease covers its own structure and copied or new pages until publication or withdrawal;
//! the policy ledger admits resident growth independently before those copies are allocated.
//!
//! Each write is checked before it is made: its cost is computed exactly, and a candidate
//! that would push the peak past the limit is not written (§5.2).
//!
//! # Releasing ids
//!
//! A release copies pages too, so a catalog filled to its limit could never shrink again.
//! Growth therefore stops short of the limit by a reserve: the most a single release
//! allocates in a fresh draft. A release may use the reserve, so every draft can release at
//! least one id even when growth cannot proceed, and maintenance always makes progress.
//! Released values leave holes in their dictionary's heap until the catalog is loaded again;
//! compacting a heap in place is not implemented.
//!
//! Deciding that nothing references an id belongs to maintenance (§5.3). The catalog keeps
//! what only it knows: how many live combinations use each label value, in storage
//! (`persist`).

#![allow(
    dead_code,
    reason = "the write path, queries and maintenance that use the catalog come later"
)]

mod combos;
mod dictionary;
mod error;
mod ids;
mod paged;
mod persist;
pub(crate) use persist::pin_totals;
#[cfg(test)]
mod tests;

use std::{iter, sync::Arc};

use snafu::{OptionExt, ResultExt, ensure};

#[cfg(debug_assertions)]
use self::error::CheckPinSnafu;
#[cfg(test)]
pub(crate) use self::persist::raw;
pub(crate) use self::{
    combos::ComboLabels,
    error::{LoadError, PinError, ReleaseError, ResolveError, UsesError},
    persist::{is_pinned, label_uses, pin_count, pinned_combo},
};
use self::{
    combos::Combos,
    dictionary::{Dictionary, ValueHasher},
    error::{
        CountOverflowSnafu, CountUnderflowSnafu, CountUsesSnafu, DeleteComboIndexSnafu,
        DeleteComboSnafu, DeleteLabelSnafu, InvalidComboIndexSnafu, ReadComboIndexSnafu,
        ReserveReleaseSnafu, StageReleaseSnafu, StageResolutionSnafu, StaleComboIndexSnafu,
        UncountUsesSnafu, WriteComboIndexSnafu, WriteComboSnafu,
    },
    paged::{ARC_HEADER, Ledger, PAGE_ALLOC, Paged, spine_bytes},
};
use crate::{
    LabelId, Schema, SeriesSpec,
    budget::{BudgetExceeded, MemoryAccount, Reservation},
    schema::MAX_DIMENSIONS,
    storage::{StorageRead, StorageWrite},
};

/// What every version of one loaded catalog shares.
struct Shared {
    hasher: ValueHasher,
    /// Whether the schema has a series, and so combinations.
    series: bool,
    /// The series dimensions, in key order.
    dimensions: Box<[LabelId]>,
    /// For each fold step, whether it folds the dimension at each position.
    fold: Box<[Box<[bool]>]>,
}

/// One version's state. Cloning it shares every array.
#[derive(Clone)]
struct Inner {
    shared: Arc<Shared>,
    /// Indexed by label id.
    dictionaries: Vec<Dictionary>,
    combos: Combos,
}

impl Inner {
    fn empty(schema: &Schema, hasher: ValueHasher) -> Self {
        let (dimensions, fold) = match &schema.series {
            Some(series) => {
                let fold = series
                    .fold
                    .iter()
                    .map(|step| {
                        series
                            .dimensions
                            .iter()
                            .map(|dimension| step.contains(dimension))
                            .collect()
                    })
                    .collect();
                (series.dimensions.clone().into_boxed_slice(), fold)
            }
            None => (Box::default(), Box::default()),
        };
        debug_assert!(
            dimensions.len() <= persist::MAX_KEY_DIMENSIONS,
            "{} series dimensions, past the {} of a valid schema",
            dimensions.len(),
            persist::MAX_KEY_DIMENSIONS
        );
        Self {
            dictionaries: schema.labels.iter().map(|_| Dictionary::new()).collect(),
            combos: Combos::new(dimensions.len()),
            shared: Arc::new(Shared {
                hasher,
                series: schema.series.is_some(),
                dimensions,
                fold,
            }),
        }
    }

    /// The version's own structure besides its arrays: its `Arc` allocation and its two
    /// vectors. A draft copies exactly this much when it starts.
    fn fixed_bytes(&self) -> u64 {
        (ARC_HEADER
            + size_of::<Inner>()
            + self.dictionaries.len() * size_of::<Dictionary>()
            + self.combos.dimensions() * size_of::<Paged<u32>>()) as u64
    }

    fn bytes(&self) -> u64 {
        self.fixed_bytes()
            + self.dictionaries.iter().map(Dictionary::bytes).sum::<u64>()
            + self.combos.bytes()
    }

    fn dictionary(&self, dimension: LabelId) -> &Dictionary {
        &self.dictionaries[usize::from(dimension.0)]
    }

    fn label(&self, dimension: LabelId, id: u32) -> Option<&[u8]> {
        self.dictionary(dimension).get(id)
    }

    fn find_label(&self, dimension: LabelId, value: &[u8]) -> Option<u32> {
        self.dictionary(dimension)
            .find(self.shared.hasher.hash(value), value)
    }
}

/// An immutable version of the catalog. Cloning it clones three words.
#[derive(Clone)]
pub(crate) struct Catalog {
    inner: Arc<Inner>,
    bytes: u64,
    /// The versions since the catalog was loaded whose draft released an id: two versions of
    /// one generation give every id they both hold the same value. Growth keeps it, since a
    /// new id never changes what an old one means (§5.3 rule 5).
    generation: u64,
}

impl Catalog {
    /// An empty catalog for `schema`, which has to be valid. The overflow marker of each
    /// dimension and combination 0 are reserved.
    pub(crate) fn new(schema: &Schema) -> Self {
        Self::with_hasher(schema, ValueHasher::random())
    }

    /// [`new`](Self::new), with the values hashed by `hasher`.
    fn with_hasher(schema: &Schema, hasher: ValueHasher) -> Self {
        Self::from_inner(Inner::empty(schema, hasher))
    }

    /// Rebuilds a version from the records `read` holds (see `persist`). Free ids are the
    /// gaps below the highest id.
    ///
    /// Loading counts its memory as a draft does, the version's own structure included, and
    /// admits all of it against `limit`, the catalog budget, before it allocates any
    /// ([`LoadError::ReserveMemory`]). It allocates each part once, at its final size, so it
    /// never holds more than the version it returns, and that version is no larger than any
    /// version holding the same records: a catalog written under `limit` loads under it, and
    /// one that does not fit fails to load.
    pub(crate) fn load(
        read: &impl StorageRead,
        schema: &Schema,
        limit: u64,
    ) -> Result<Self, LoadError> {
        Self::load_with(read, schema, ValueHasher::random(), limit)
    }

    /// [`load`](Self::load), with the values hashed by `hasher`.
    fn load_with(
        read: &impl StorageRead,
        schema: &Schema,
        hasher: ValueHasher,
        limit: u64,
    ) -> Result<Self, LoadError> {
        let mut ledger = Ledger::default();
        let inner = persist::load(read, schema, hasher, limit, &mut ledger)?;
        let catalog = Self::from_inner(inner);
        debug_assert_eq!(
            catalog.bytes,
            ledger.peak(),
            "loading held only the version"
        );
        Ok(catalog)
    }

    fn from_inner(inner: Inner) -> Self {
        let bytes = inner.bytes();
        Self {
            inner: Arc::new(inner),
            bytes,
            generation: 0,
        }
    }

    /// The value of label id `id` of `dimension`. The overflow marker, id 0, has none.
    pub(crate) fn label(&self, dimension: LabelId, id: u32) -> Option<&[u8]> {
        self.inner.label(dimension, id)
    }

    pub(crate) fn find_label(&self, dimension: LabelId, value: &[u8]) -> Option<u32> {
        self.inner.find_label(dimension, value)
    }

    /// The label ids of combination `id`, if it exists. Combination 0 always does.
    pub(crate) fn combo(&self, id: u32) -> Option<ComboLabels<'_>> {
        self.inner.combos.get(id)
    }

    /// The bytes of this version: every page and spine it reaches, shared with other
    /// versions or not, and its own structure. See the module docs.
    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Resident nonzero combinations, label values and folded nonzero combinations.
    /// This streams live ids and allocates nothing; implicit overflow markers are excluded.
    pub(crate) fn status_counts(&self) -> (u64, u64, u64) {
        let mut combos = 0;
        let mut folded = 0;
        for id in self.inner.combos.live_ids() {
            combos += 1;
            folded += u64::from(
                self.combo(id)
                    .expect("a live combination exists")
                    .any(|id| id == 0),
            );
        }
        let values = self
            .inner
            .dictionaries
            .iter()
            .map(|d| d.live_ids().count() as u64)
            .sum();
        (combos, values, folded)
    }

    /// The generation of this version. It is kept in memory only: a loaded catalog has
    /// generation 0, and only the marker each open makes (`OpenMarker`) tells two opens apart.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// The fixed structure a fresh draft copies, also used by the staging floor.
    pub(crate) fn draft_bytes(&self) -> u64 {
        self.inner.fixed_bytes()
    }

    /// Starts deriving the next version under `limit`, charging copies to `staging`.
    pub(crate) fn draft(
        &self,
        limit: u64,
        staging: &MemoryAccount,
    ) -> Result<CatalogDraft, BudgetExceeded> {
        let fixed = self.draft_bytes();
        let mut ledger = Ledger::staging(staging)?;
        ledger.alloc(fixed)?;
        let next = Inner::clone(&self.inner);
        Ok(CatalogDraft {
            reserve: release_reserve(fixed, limit.max(self.bytes)),
            base: self.clone(),
            next,
            ledger,
            limit,
            changed: false,
            released: false,
            failed: false,
        })
    }
}

/// The most a single release allocates in a fresh draft of a catalog of at most `size`
/// bytes: the draft's own structure, then a page and the spine of each array the release
/// writes. A value's release writes two (its dictionary's live bits and table), a
/// combination's one (the live bits). An array of such a catalog has at most
/// `size / PAGE_ALLOC + 1` pages.
fn release_reserve(fixed: u64, size: u64) -> u64 {
    let spine = spine_bytes::<u64>((size / PAGE_ALLOC as u64 + 1) as usize);
    fixed.saturating_add(2 * (PAGE_ALLOC as u64 + spine))
}

/// Where [`CatalogDraft::resolve_combo`] put a combination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedCombo {
    pub(crate) id: u32,
    /// The fold that applied, or `None` when the combination is the one asked for.
    pub(crate) fold: Option<Fold>,
}

/// A step of the fold ladder (§5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fold {
    /// Step `n` of [`SeriesSpec::fold`]: its dimensions were replaced by the overflow
    /// marker.
    Step(usize),
    /// The implied last step: combination 0, every dimension the overflow marker.
    Whole,
}

impl Fold {
    /// Whether this fold replaced `label` with the overflow marker, so a `FoldedLabel`
    /// column keeps the original value (§11.1).
    pub(crate) fn folds(self, series: &SeriesSpec, label: LabelId) -> bool {
        match self {
            Self::Step(step) => series.fold[step].contains(&label),
            Self::Whole => series.dimensions.contains(&label),
        }
    }
}

/// A pin [`CatalogDraft::unpin`] took off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Unpinned {
    /// The combination the key pinned.
    pub(crate) combo: u32,
    /// The pins it has left. Combination 0 counts none.
    pub(crate) left: u64,
}

/// The next version of the catalog, under construction. It writes the records of its
/// changes into the write transaction it is given, and nothing else: no commit, no
/// metadata. [`finish`](Self::finish) makes it a [`Catalog`]; dropping it discards it.
///
/// After a write to the batch, counting a label value's uses, or a resolution's allocation
/// fails, the draft and batch may hold only part of a change: drop them together. A release
/// refuses staging before changing either, so the round can publish its earlier releases.
pub(crate) struct CatalogDraft {
    base: Catalog,
    next: Inner,
    ledger: Ledger,
    limit: u64,
    /// Kept free by growth, for releases (see the module docs).
    reserve: u64,
    changed: bool,
    /// Whether the draft released an id, which gives the next version the next generation.
    released: bool,
    failed: bool,
}

impl CatalogDraft {
    pub(crate) fn label(&self, dimension: LabelId, id: u32) -> Option<&[u8]> {
        self.next.label(dimension, id)
    }

    pub(crate) fn find_label(&self, dimension: LabelId, value: &[u8]) -> Option<u32> {
        self.next.find_label(dimension, value)
    }

    pub(crate) fn combo(&self, id: u32) -> Option<ComboLabels<'_>> {
        self.next.combos.get(id)
    }

    /// The most bytes the base version and this draft have held together so far. It never
    /// passes the limit, unless the base was loaded too close to or past it; then releases
    /// may still use the reserve above the base.
    pub(crate) fn peak(&self) -> u64 {
        self.base.bytes.saturating_add(self.ledger.peak())
    }

    /// The combination of `values`, one per series dimension in [`SeriesSpec::dimensions`]
    /// order, inserting the values and the combination that are new if they fit the
    /// budget. Otherwise it goes down the fold ladder: each step replaces its dimensions
    /// with the overflow marker and drops their new values, then looks the folded
    /// combination up, or inserts it with the remaining new values if they fit. The implied
    /// last step, combination 0, always succeeds (§5.2).
    ///
    /// A value or combination a fold discards is never inserted: it reaches neither the
    /// version nor storage.
    ///
    /// # Panics
    ///
    /// If the schema has no series, or `values` does not have one value per dimension.
    pub(crate) fn resolve_combo(
        &mut self,
        batch: &mut impl StorageWrite,
        values: &[&[u8]],
    ) -> Result<ResolvedCombo, ResolveError> {
        let shared = Arc::clone(&self.next.shared);
        assert!(
            shared.series,
            "resolving a combination of a schema without a series"
        );
        assert_eq!(
            values.len(),
            shared.dimensions.len(),
            "a combination has one value per series dimension"
        );
        let mut hashes = [0; MAX_DIMENSIONS];
        let mut found = [None; MAX_DIMENSIONS];
        for position in 0..values.len() {
            hashes[position] = shared.hasher.hash(values[position]);
            found[position] = self
                .next
                .dictionary(shared.dimensions[position])
                .find(hashes[position], values[position]);
        }
        let mut labels = [0; MAX_DIMENSIONS];
        let labels = &mut labels[..values.len()];
        let mut new = [0; MAX_DIMENSIONS];
        for step in iter::once(None).chain((0..shared.fold.len()).map(Some)) {
            let mut new_len = 0;
            for (position, label) in labels.iter_mut().enumerate() {
                let folded = step.is_some_and(|step| shared.fold[step][position]);
                *label = match found[position] {
                    _ if folded => 0,
                    Some(id) => id,
                    None => {
                        new[new_len] = position;
                        new_len += 1;
                        0
                    }
                };
            }
            let new = &new[..new_len];
            let fold = step.map(Fold::Step);
            // A combination with a new value is new too.
            if new.is_empty()
                && let Some(id) = self.lookup(batch, labels)?
            {
                return Ok(ResolvedCombo { id, fold });
            }
            let Some(cost) = self.growth_cost(&shared, new, values, &hashes) else {
                continue;
            };
            if !self.fits_growth(cost) {
                continue;
            }
            let before = self.ledger.allocated();
            for &position in new {
                labels[position] = self.insert_value(
                    batch,
                    shared.dimensions[position],
                    values[position],
                    hashes[position],
                )?;
            }
            let id = self.insert_combo(batch, labels)?;
            debug_assert_eq!(self.ledger.allocated() - before, cost, "predicted cost");
            return Ok(ResolvedCombo { id, fold });
        }
        Ok(ResolvedCombo {
            id: 0,
            fold: Some(Fold::Whole),
        })
    }

    /// The id of `value` in `dimension`, for an event's `Label` column: the existing one,
    /// or a new one if it fits the budget. Otherwise 0, the overflow marker; the caller
    /// counts the loss.
    pub(crate) fn resolve_label(
        &mut self,
        batch: &mut impl StorageWrite,
        dimension: LabelId,
        value: &[u8],
    ) -> Result<u32, ResolveError> {
        let hash = self.next.shared.hasher.hash(value);
        let dictionary = self.next.dictionary(dimension);
        if let Some(id) = dictionary.find(hash, value) {
            return Ok(id);
        }
        match dictionary.insert_cost(hash, value) {
            Some(cost) if self.fits_growth(cost) => {
                let before = self.ledger.allocated();
                let id = self.insert_value(batch, dimension, value, hash)?;
                debug_assert_eq!(self.ledger.allocated() - before, cost, "predicted cost");
                Ok(id)
            }
            _ => Ok(0),
        }
    }

    /// Pins combination `id` under `key`, a name the consumer chose: its auxiliary state
    /// references the combination, so it cannot be released (§5.3). The key and the
    /// combination's count of keys go into the batch, in the same transaction as the
    /// auxiliary state, which can hold the key; they cost the catalog budget nothing.
    /// Combination 0 is never released, so it gets no count, but its key is kept.
    ///
    /// Returns false, writing nothing, when `key` pins a combination already. A stored count
    /// that cannot be incremented is damage: it fails with [`PinError::CountOverflow`] and
    /// writes nothing.
    ///
    /// # Panics
    ///
    /// In debug builds, if `id` is not live.
    pub(crate) fn pin(
        &mut self,
        batch: &mut impl StorageWrite,
        key: &[u8],
        id: u32,
    ) -> Result<bool, PinError> {
        debug_assert!(
            self.next.combos.is_live(id),
            "pinning combination {id}, which is not live"
        );
        if pinned_combo(batch, key)?.is_some() {
            return Ok(false);
        }
        if id != 0 {
            let count = pin_count(batch, id)?
                .checked_add(1)
                .context(CountOverflowSnafu { id })?;
            let written = persist::put_pin_count(batch, id, count);
            self.written(written)?;
        }
        let written = persist::put_pin_key(batch, key, id);
        self.written(written)?;
        Ok(true)
    }

    /// Takes the pin under `key` off the combination it pins, and deletes the key. It never
    /// resolves labels, which can name another combination by now (§5.3).
    ///
    /// Returns `None`, writing nothing, when `key` pins nothing. A key whose combination has
    /// no count left is damage: it fails with [`PinError::CountUnderflow`] and writes
    /// nothing.
    pub(crate) fn unpin(
        &mut self,
        batch: &mut impl StorageWrite,
        key: &[u8],
    ) -> Result<Option<Unpinned>, PinError> {
        let Some(id) = pinned_combo(batch, key)? else {
            return Ok(None);
        };
        let mut left = 0;
        if id != 0 {
            left = pin_count(batch, id)?
                .checked_sub(1)
                .context(CountUnderflowSnafu { id })?;
            let written = persist::put_pin_count(batch, id, left);
            self.written(written)?;
        }
        let written = persist::delete_pin_key(batch, key);
        self.written(written)?;
        Ok(Some(Unpinned { combo: id, left }))
    }

    /// Releases combination `id`: deletes its record and its reverse-lookup entry, takes its
    /// use off each of its label values, and frees the id for reuse, smallest first (§5.3).
    /// Returns the values no live combination uses any more, which may have become free.
    ///
    /// The caller guarantees that nothing references the combination in the state the batch
    /// commits: no chunk, journal record or pin. Deciding that belongs to maintenance. Debug
    /// builds check the pins.
    ///
    /// Fails with [`ReleaseError::ReserveRelease`], changing nothing, when the budget has no
    /// room for the pages the release copies.
    ///
    /// # Panics
    ///
    /// If `id` is 0 or not live; in debug builds, if it is pinned.
    pub(crate) fn release_combo(
        &mut self,
        batch: &mut impl StorageWrite,
        id: u32,
    ) -> Result<std::iter::Take<std::array::IntoIter<(LabelId, u32), MAX_DIMENSIONS>>, ReleaseError>
    {
        assert!(id != 0, "combination 0 is implicit and never released");
        let mut labels = [0; MAX_DIMENSIONS];
        let stored = self
            .next
            .combos
            .get(id)
            .unwrap_or_else(|| panic!("releasing combination {id}, which is not live"));
        let len = stored.len();
        for (target, value) in labels.iter_mut().zip(stored) {
            *target = value;
        }
        let labels = &labels[..len];
        #[cfg(debug_assertions)]
        {
            let pinned = is_pinned(batch, id).context(CheckPinSnafu { id })?;
            assert!(!pinned, "releasing combination {id}, which is pinned");
        }
        let cost = self.next.combos.release_cost(id);
        self.reserve_release(cost)?;
        let written = persist::delete_combo(batch, id).context(DeleteComboSnafu { id });
        self.written(written)?;
        let key = persist::index_key(labels);
        let written =
            persist::delete_index(batch, key.as_slice()).context(DeleteComboIndexSnafu { id });
        self.written(written)?;
        let shared = Arc::clone(&self.next.shared);
        let mut unused = [(LabelId(0), 0); MAX_DIMENSIONS];
        let mut unused_len = 0;
        for (&dimension, &label) in shared.dimensions.iter().zip(labels) {
            if label == 0 {
                continue;
            }
            let left =
                persist::remove_use(batch, dimension, label).context(UncountUsesSnafu { id });
            if self.written(left)? == 0 {
                unused[unused_len] = (dimension, label);
                unused_len += 1;
            }
        }
        let before = self.ledger.allocated();
        self.next
            .combos
            .release(id, &mut self.ledger)
            .expect("the release admitted all its copies");
        debug_assert_eq!(self.ledger.allocated() - before, cost, "predicted cost");
        self.changed = true;
        self.released = true;
        Ok(unused.into_iter().take(unused_len))
    }

    /// Releases value `id` of `dimension`: deletes its record and frees the id for reuse,
    /// smallest first (§5.3).
    ///
    /// The caller guarantees that nothing references the value in the state the batch
    /// commits: no combination, and no `Label` column of a chunk or journal record.
    /// Maintenance knows the first from the value's count of uses ([`label_uses`]), and the
    /// second from the references the write path records (`layout::references`). Debug builds
    /// check the combinations by scanning them.
    ///
    /// Fails with [`ReleaseError::ReserveRelease`], changing nothing, when the budget has no
    /// room for the pages the release copies.
    ///
    /// # Panics
    ///
    /// If `id` is 0 or not live; in debug builds, if a live combination uses it.
    pub(crate) fn release_label(
        &mut self,
        batch: &mut impl StorageWrite,
        dimension: LabelId,
        id: u32,
    ) -> Result<(), ReleaseError> {
        assert!(
            id != 0,
            "label value 0 is the overflow marker and never released"
        );
        let dictionary = self.next.dictionary(dimension);
        assert!(
            dictionary.is_live(id),
            "releasing value {id} of {dimension:?}, which is not live"
        );
        #[cfg(debug_assertions)]
        if let Some(position) = self
            .next
            .shared
            .dimensions
            .iter()
            .position(|&d| d == dimension)
            && let Some(user) = self.next.combos.user_of(position, id)
        {
            panic!("releasing value {id} of {dimension:?}, which live combination {user} uses");
        }
        let cost = dictionary.release_cost(&self.next.shared.hasher, id);
        self.reserve_release(cost)?;
        let written = persist::delete_label(batch, dimension, id).context(DeleteLabelSnafu {
            dimension: dimension.0,
            id,
        });
        self.written(written)?;
        let before = self.ledger.allocated();
        let Inner {
            shared,
            dictionaries,
            ..
        } = &mut self.next;
        dictionaries[usize::from(dimension.0)]
            .release(&shared.hasher, id, &mut self.ledger)
            .expect("the release admitted all its copies");
        debug_assert_eq!(self.ledger.allocated() - before, cost, "predicted cost");
        self.changed = true;
        self.released = true;
        Ok(())
    }

    /// The version this draft built: of the base's generation, or of the next one when the
    /// draft released an id.
    ///
    /// Returns the catalog and its transient staging charge, kept until publication or withdrawal.
    ///
    /// # Panics
    ///
    /// If a write or allocation failed: the draft has to be dropped with its batch.
    pub(crate) fn finish(self) -> (Catalog, Reservation) {
        assert!(
            !self.failed,
            "finishing a catalog draft after a write to its batch failed"
        );
        if !self.changed {
            drop(self.next);
            return (self.base, self.ledger.finish());
        }
        let catalog = Catalog {
            generation: self.base.generation + u64::from(self.released),
            ..Catalog::from_inner(self.next)
        };
        (catalog, self.ledger.finish())
    }

    /// The bytes inserting the new values at positions `new`, then a combination, allocate;
    /// `None` when one of them cannot be stored at all. Each value goes to another
    /// dictionary, so the costs are independent and add up.
    fn growth_cost(
        &self,
        shared: &Shared,
        new: &[usize],
        values: &[&[u8]],
        hashes: &[u64],
    ) -> Option<u64> {
        let mut cost = self.next.combos.insert_cost()?;
        for &position in new {
            let dictionary = self.next.dictionary(shared.dimensions[position]);
            cost += dictionary.insert_cost(hashes[position], values[position])?;
        }
        Some(cost)
    }

    /// Whether growth that allocates `cost` more bytes keeps the peak within the limit,
    /// with the release reserve to spare.
    fn fits_growth(&self, cost: u64) -> bool {
        self.held()
            .saturating_add(cost)
            .saturating_add(self.reserve)
            <= self.limit
    }

    /// Checks that a release allocating `cost` bytes fits. Releases may use the reserve.
    /// When a base loaded past a lowered limit leaves no room at all, they may still use the
    /// reserve above the base, so that the catalog can shrink.
    fn reserve_release(&mut self, cost: u64) -> Result<(), ReleaseError> {
        let ceiling = self.limit.max(self.base.bytes.saturating_add(self.reserve));
        let held = self.held();
        if held.saturating_add(cost) > ceiling {
            return Err(BudgetExceeded {
                what: "catalog",
                needed: cost,
                available: ceiling.saturating_sub(held),
            })
            .context(ReserveReleaseSnafu);
        }
        self.ledger.reserve_copies(cost).context(StageReleaseSnafu)
    }

    /// The bytes the base and the draft hold now.
    fn held(&self) -> u64 {
        self.base.bytes.saturating_add(self.ledger.owned())
    }

    /// The combination with `labels`, from the reverse lookup in `read`.
    fn lookup(&self, read: &impl StorageRead, labels: &[u32]) -> Result<Option<u32>, ResolveError> {
        let key = persist::index_key(labels);
        let id = match persist::get_index(read, key.as_slice()).context(ReadComboIndexSnafu)? {
            None => None,
            Some(Ok(id)) => Some(id),
            Some(Err(problem)) => return InvalidComboIndexSnafu { problem }.fail(),
        };
        if let Some(id) = id {
            ensure!(
                self.next.combos.matches(id, labels),
                StaleComboIndexSnafu { id }
            );
        }
        Ok(id)
    }

    fn insert_value(
        &mut self,
        batch: &mut impl StorageWrite,
        dimension: LabelId,
        value: &[u8],
        hash: u64,
    ) -> Result<u32, ResolveError> {
        let Inner {
            shared,
            dictionaries,
            ..
        } = &mut self.next;
        let dictionary = &mut dictionaries[usize::from(dimension.0)];
        let id = dictionary.next_id().expect("the cost check found an id");
        if let Err(error) = persist::put_label(batch, dimension, id, value, self.ledger.account()) {
            self.failed = true;
            return Err(error);
        }
        let inserted = dictionary
            .insert(&shared.hasher, hash, value, &mut self.ledger)
            .context(StageResolutionSnafu);
        let inserted = self.written(inserted)?;
        debug_assert_eq!(inserted, id, "the dictionary took the id it named");
        self.changed = true;
        Ok(id)
    }

    fn insert_combo(
        &mut self,
        batch: &mut impl StorageWrite,
        labels: &[u32],
    ) -> Result<u32, ResolveError> {
        let id = self
            .next
            .combos
            .next_id()
            .expect("the cost check found an id");
        let written = persist::put_combo(batch, id, labels).context(WriteComboSnafu { id });
        self.written(written)?;
        let key = persist::index_key(labels);
        let written =
            persist::put_index(batch, key.as_slice(), id).context(WriteComboIndexSnafu { id });
        self.written(written)?;
        let shared = Arc::clone(&self.next.shared);
        for (&dimension, &label) in shared.dimensions.iter().zip(labels) {
            if label != 0 {
                let counted =
                    persist::add_use(batch, dimension, label).context(CountUsesSnafu { id });
                self.written(counted)?;
            }
        }
        let inserted = self
            .next
            .combos
            .insert(labels, &mut self.ledger)
            .context(StageResolutionSnafu);
        let inserted = self.written(inserted)?;
        debug_assert_eq!(inserted, id, "the combinations took the id they named");
        self.changed = true;
        Ok(id)
    }

    /// Notes a failed write to the batch.
    fn written<T, E>(&mut self, result: Result<T, E>) -> Result<T, E> {
        self.failed |= result.is_err();
        result
    }
}

/// What a version holds, for comparing two of them in tests.
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Contents {
    /// One per dimension.
    labels: Vec<Live<Vec<u8>>>,
    combos: Live<Vec<u32>>,
}

/// The live ids with what they hold, and the free ids below the highest.
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Live<T> {
    live: Vec<(u32, T)>,
    gaps: Vec<u32>,
}

#[cfg(test)]
impl Inner {
    fn contents(&self) -> Contents {
        let labels = self
            .dictionaries
            .iter()
            .map(|dictionary| Live {
                live: dictionary
                    .live_ids()
                    .map(|id| (id, dictionary.get(id).unwrap().to_vec()))
                    .collect(),
                gaps: dictionary.gaps(),
            })
            .collect();
        let combos = Live {
            live: self
                .combos
                .live_ids()
                .map(|id| (id, self.combos.get(id).unwrap().collect()))
                .collect(),
            gaps: self.combos.gaps(),
        };
        Contents { labels, combos }
    }

    /// The bytes of this version that `base` does not share.
    fn unshared(&self, base: &Self) -> u64 {
        let dictionaries: u64 = self
            .dictionaries
            .iter()
            .zip(&base.dictionaries)
            .map(|(dictionary, base)| dictionary.unshared(base))
            .sum();
        self.fixed_bytes() + dictionaries + self.combos.unshared(&base.combos)
    }
}

#[cfg(test)]
impl Catalog {
    fn contents(&self) -> Contents {
        self.inner.contents()
    }

    /// The live combinations with their label ids, ascending. Combination 0 is not among
    /// them.
    pub(crate) fn live_combos(&self) -> Vec<(u32, Vec<u32>)> {
        self.contents().combos.live
    }

    /// The live values of `dimension` with their bytes, ascending.
    pub(crate) fn live_labels(&self, dimension: LabelId) -> Vec<(u32, Vec<u8>)> {
        self.inner
            .dictionary(dimension)
            .live_ids()
            .map(|id| (id, self.label(dimension, id).unwrap().to_vec()))
            .collect()
    }
}

#[cfg(test)]
impl CatalogDraft {
    /// The bytes the draft holds, by its ledger.
    fn owned(&self) -> u64 {
        self.ledger.owned()
    }

    /// The bytes the draft holds, by comparing it with its base.
    fn unshared(&self) -> u64 {
        self.next.unshared(&self.base.inner)
    }

    fn reserve(&self) -> u64 {
        self.reserve
    }

    /// What inserting one combination would allocate now.
    fn combo_cost(&self) -> u64 {
        self.next.combos.insert_cost().unwrap()
    }

    /// What inserting `value` into `dimension` would allocate now.
    fn value_cost(&self, dimension: LabelId, value: &[u8]) -> u64 {
        let hash = self.next.shared.hasher.hash(value);
        self.next
            .dictionary(dimension)
            .insert_cost(hash, value)
            .unwrap()
    }

    /// Moves the limit so that exactly `room` more bytes of growth fit.
    fn set_room(&mut self, room: u64) {
        self.limit = self.held() + self.reserve + room;
    }
}
