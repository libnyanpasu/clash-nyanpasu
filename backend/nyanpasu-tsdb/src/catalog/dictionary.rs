//! A label dictionary (§5.1): values in a byte heap, a span per value id, and an
//! open-addressing table of `u32` ids for lookup, which compares keys against the heap. Id
//! 0 is the overflow marker and never a user value (§5.2).
//!
//! Every part is paged, so a version shares what the next one does not change (§7.1).
//! Releasing a value leaves its bytes in the heap as a hole until the catalog is loaded
//! again, and its table slot as a tombstone until the table is rebuilt.
//!
//! Loading builds each part at its final size from a first pass that counts the values (a
//! [`LoadPlan`]), so it never grows or copies one, and each part is no larger than the same
//! part of any version that holds those values (see `LoadPlan`).

use std::{collections::hash_map::RandomState, hash::BuildHasher, iter, sync::Arc};

use super::{
    error::CatalogProblem,
    ids::Ids,
    paged::{FIRST_PAGE_PAYLOAD, Ledger, PAGE_PAYLOAD, Paged, Pages, page_bytes},
};
use crate::budget::BudgetExceeded;

/// Hashes label values. Outsiders control them (connection targets, for example), so the
/// hash is SipHash under random keys (§0.3 rule 3). The keys are made once per loaded
/// catalog and shared by all its versions; the tables are rebuilt at load, so the keys never
/// persist.
pub(crate) struct ValueHasher {
    state: RandomState,
    /// A replacement hash, so tests can make values collide on purpose.
    #[cfg(test)]
    fixed: Option<fn(&[u8]) -> u64>,
}

impl ValueHasher {
    pub(crate) fn random() -> Self {
        Self {
            state: RandomState::new(),
            #[cfg(test)]
            fixed: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn fixed(hash: fn(&[u8]) -> u64) -> Self {
        Self {
            state: RandomState::new(),
            fixed: Some(hash),
        }
    }

    /// The same SipHash under fixed keys, for tests whose outcome depends on where values
    /// land in a table: every run then lays them out alike.
    #[cfg(test)]
    pub(crate) fn fixed_keys() -> Self {
        use std::hash::{BuildHasherDefault, DefaultHasher};
        Self::fixed(|value| BuildHasherDefault::<DefaultHasher>::default().hash_one(value))
    }

    pub(crate) fn hash(&self, value: &[u8]) -> u64 {
        #[cfg(test)]
        if let Some(hash) = self.fixed {
            return hash(value);
        }
        self.state.hash_one(value)
    }
}

/// Where a value's bytes lie in the heap.
#[derive(Clone, Copy, Debug, Default)]
struct Span {
    page: u32,
    start: u32,
    len: u32,
}

/// Value bytes, in pages that each hold whole values, so a value is always one slice. A
/// value longer than a page gets a page of its own. The first page doubles from 64 bytes as
/// it fills, as in [`Paged`]. A loaded heap is one page that holds every value side by side.
#[derive(Clone)]
struct Heap {
    pages: Pages<u8>,
    /// The bytes used in the last page.
    used: usize,
}

/// How the heap makes room for a value.
enum Append {
    /// The value is empty and needs no bytes.
    Nothing,
    InPlace,
    /// The only page grows to this length.
    Grow(usize),
    NewPage(usize),
}

impl Heap {
    const fn new() -> Self {
        Self {
            pages: Pages::new(),
            used: 0,
        }
    }

    fn get(&self, span: Span) -> &[u8] {
        if span.len == 0 {
            return &[];
        }
        let start = span.start as usize;
        &self.pages.page(span.page as usize)[start..start + span.len as usize]
    }

    fn plan(&self, len: usize) -> Append {
        if len == 0 {
            return Append::Nothing;
        }
        let Some(last) = self.pages.last() else {
            return Append::NewPage(grown(FIRST_PAGE_PAYLOAD, len));
        };
        let need = self.used + len;
        if need <= last.len() {
            Append::InPlace
        } else if self.pages.count() == 1 && last.len() < PAGE_PAYLOAD && need <= PAGE_PAYLOAD {
            Append::Grow(grown(last.len(), need))
        } else {
            Append::NewPage(len.max(PAGE_PAYLOAD))
        }
    }

    fn append_cost(&self, len: usize) -> u64 {
        match self.plan(len) {
            Append::Nothing => 0,
            Append::InPlace => self.pages.write_cost(self.pages.count() - 1),
            Append::Grow(len) => self.pages.spine_copy_cost() + page_bytes::<u8>(len),
            Append::NewPage(len) => self.pages.push_spine_cost() + page_bytes::<u8>(len),
        }
    }

    /// # Panics
    ///
    /// If `value` is longer than `u32::MAX` bytes; the dictionary refuses those first.
    fn append(&mut self, value: &[u8], ledger: &mut Ledger) -> Result<Span, BudgetExceeded> {
        let len = u32::try_from(value.len()).expect("the dictionary refuses longer values");
        match self.plan(value.len()) {
            Append::Nothing => return Ok(Span::default()),
            Append::InPlace => {}
            Append::Grow(page_len) => {
                ledger.alloc(page_bytes::<u8>(page_len))?;
                let old = self.pages.page(0);
                let page = old
                    .iter()
                    .copied()
                    .chain(iter::repeat_n(0, page_len - old.len()))
                    .collect();
                self.pages.replace(0, page, ledger)?;
            }
            Append::NewPage(page_len) => {
                ledger.alloc(page_bytes::<u8>(page_len))?;
                self.pages
                    .push(iter::repeat_n(0, page_len).collect::<Arc<[u8]>>(), ledger)?;
                self.used = 0;
            }
        }
        let page = self.pages.count() - 1;
        let start = self.used;
        self.pages.page_mut(page, ledger)?[start..start + value.len()].copy_from_slice(value);
        self.used += value.len();
        Ok(Span {
            page: u32::try_from(page).expect("a heap has fewer than 2^32 pages"),
            start: u32::try_from(start).expect("a page holds fewer than 2^32 bytes"),
            len,
        })
    }

    /// One zeroed page of `bytes` bytes, or none for none, for loading values that total
    /// `bytes`. Once [`load`](Self::load) has filled it, an append starts a new page, or
    /// grows a lone short one.
    fn sized(bytes: usize, ledger: &mut Ledger) -> Self {
        let pages = match bytes {
            0 => Pages::new(),
            _ => Pages::filled(&[bytes], 0, ledger).expect("the loader admitted its heap"),
        };
        Self { pages, used: 0 }
    }

    fn sized_cost(bytes: usize) -> u64 {
        match bytes {
            0 => 0,
            _ => Pages::<u8>::filled_cost(&[bytes]),
        }
    }

    /// Copies `value` after the values loaded before it, into the page of
    /// [`sized`](Self::sized), which only the loader holds, so it allocates nothing.
    ///
    /// # Panics
    ///
    /// If the values loaded pass the bytes the page was sized for.
    fn load(&mut self, value: &[u8], ledger: &mut Ledger) -> Span {
        if value.is_empty() {
            return Span::default();
        }
        let start = self.used;
        self.pages
            .page_mut(0, ledger)
            .expect("loading owns its heap")[start..start + value.len()]
            .copy_from_slice(value);
        self.used += value.len();
        // The page holds at most `MAX_VALUE_BYTES`, so both fit.
        Span {
            page: 0,
            start: u32::try_from(start).expect("a loaded heap holds at most u32::MAX bytes"),
            len: u32::try_from(value.len()).expect("a loaded heap holds at most u32::MAX bytes"),
        }
    }
}

/// The most bytes the live values of one dictionary take together: what a span reaches in
/// one page, since loading puts them all in one (§5.1). A new value past it does not fit,
/// as when the ids run out, and goes down the fold ladder (§5.2). It is over a hundred times
/// the catalog budget of the largest profile.
pub(crate) const MAX_VALUE_BYTES: u64 = u32::MAX as u64;

/// What the values of a dictionary take once loaded, counted in a first pass over them in id
/// order: three numbers, nothing per value. [`Dictionary::sized`] allocates it, at the final
/// size of every part, and each part is no larger than the same part of any version holding
/// the same values:
///
/// - The id slots go up to the highest id, as in every version holding it, with their pages
///   full but the last ([`Paged::filled`]).
/// - The table is the smallest that holds the values within its load ([`table_slots`]).
/// - The heap is one page of exactly the bytes of the values. Any version holding them has
///   them in at least one page, besides its holes and the room left in its pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LoadPlan {
    /// The highest id.
    slots: u32,
    values: u32,
    /// The bytes of the values, at most [`MAX_VALUE_BYTES`].
    bytes: u64,
}

impl LoadPlan {
    /// Adds value `id`, of `len` bytes. Ids come in ascending order. No writer stores values
    /// that pass [`MAX_VALUE_BYTES`] together.
    pub(crate) fn add(&mut self, id: u32, len: usize) -> Result<(), CatalogProblem> {
        let bytes = self.bytes.saturating_add(len as u64);
        if bytes > MAX_VALUE_BYTES {
            return Err(CatalogProblem::ValueBytes { bytes });
        }
        self.slots = id;
        self.values += 1;
        self.bytes = bytes;
        Ok(())
    }
}

/// The length a page grows to, doubling from `len`, to hold `need` bytes. A need past a full
/// page gets a page of its own length.
fn grown(mut len: usize, need: usize) -> usize {
    if need > PAGE_PAYLOAD {
        return need;
    }
    while len < need {
        len = (2 * len).min(PAGE_PAYLOAD);
    }
    len
}

/// An empty table slot.
const EMPTY: u32 = 0;
/// A released value's table slot. It keeps the probe sequences through it intact.
const TOMBSTONE: u32 = u32::MAX;
const MIN_TABLE: usize = 8;

/// The slots of the smallest table that holds `values` within its 3/4 load, or none for no
/// value. Every table holding them is at least as large: it is a power of two of at least
/// [`MIN_TABLE`] slots, and its tombstones count toward the same load.
fn table_slots(values: u32) -> usize {
    if values == 0 {
        return 0;
    }
    (4 * values as usize)
        .div_ceil(3)
        .next_power_of_two()
        .max(MIN_TABLE)
}

/// Where a value goes in the table.
enum Place {
    /// The table is too full and is rebuilt with this many slots.
    Rebuild(usize),
    /// This slot, an empty or a tombstone one.
    Slot(usize),
}

#[derive(Clone)]
pub(crate) struct Dictionary {
    heap: Heap,
    /// Indexed by slot (id - 1). The span of a free id is stale.
    spans: Paged<Span>,
    ids: Ids,
    /// A power of two of slots, or none at all; each holds an id, [`EMPTY`] or
    /// [`TOMBSTONE`]. Linear probing, at most 3/4 occupied, tombstones included.
    table: Paged<u32>,
    /// Slots holding an id or a tombstone: both lengthen probes.
    occupied: usize,
    /// The bytes of the live values, at most [`MAX_VALUE_BYTES`].
    value_bytes: u64,
}

impl Dictionary {
    pub(crate) const fn new() -> Self {
        Self {
            heap: Heap::new(),
            spans: Paged::new(),
            ids: Ids::new(),
            table: Paged::new(),
            occupied: 0,
            value_bytes: 0,
        }
    }

    /// The value of `id`, if it is live. Id 0, the overflow marker, has none.
    pub(crate) fn get(&self, id: u32) -> Option<&[u8]> {
        let slot = id.checked_sub(1)?;
        self.ids.is_live(slot).then(|| self.value(slot))
    }

    pub(crate) fn is_live(&self, id: u32) -> bool {
        id.checked_sub(1).is_some_and(|slot| self.ids.is_live(slot))
    }

    /// The id of `value`, whose hash is `hash`.
    pub(crate) fn find(&self, hash: u64, value: &[u8]) -> Option<u32> {
        let mask = self.table.len().checked_sub(1)?;
        let mut index = hash as usize & mask;
        loop {
            match self.table.get(index) {
                EMPTY => return None,
                TOMBSTONE => {}
                id if self.value(id - 1) == value => return Some(id),
                _ => {}
            }
            index = (index + 1) & mask;
        }
    }

    /// The id the next new value gets, if any is left.
    pub(crate) fn next_id(&self) -> Option<u32> {
        self.ids.next_slot().map(|slot| slot + 1)
    }

    /// The bytes [`insert`](Self::insert) allocates for `value`, or `None` when the value
    /// cannot be stored: the ids ran out, or the live values would pass [`MAX_VALUE_BYTES`].
    pub(crate) fn insert_cost(&self, hash: u64, value: &[u8]) -> Option<u64> {
        if self.value_bytes.saturating_add(value.len() as u64) > MAX_VALUE_BYTES {
            return None;
        }
        let slot = self.ids.next_slot()?;
        let span = if slot < self.ids.slots() {
            self.spans.set_cost(slot as usize)
        } else {
            self.spans.push_cost()
        };
        let table = match self.place(hash, 1) {
            Place::Rebuild(slots) => Paged::<u32>::filled_cost(slots),
            Place::Slot(index) => self.table.set_cost(index),
        };
        Some(self.heap.append_cost(value.len()) + span + self.ids.take_cost(slot) + table)
    }

    /// Stores `value`, which the dictionary does not hold, and returns its new id: the one
    /// [`next_id`](Self::next_id) named.
    ///
    /// # Panics
    ///
    /// If [`insert_cost`](Self::insert_cost) refuses the value.
    pub(crate) fn insert(
        &mut self,
        hasher: &ValueHasher,
        hash: u64,
        value: &[u8],
        ledger: &mut Ledger,
    ) -> Result<u32, BudgetExceeded> {
        debug_assert_eq!(
            self.find(hash, value),
            None,
            "inserting a value held already"
        );
        let slot = self.ids.next_slot().expect("the ids ran out");
        let place = self.place(hash, 1);
        let span = self.heap.append(value, ledger)?;
        if slot < self.ids.slots() {
            self.spans.set(slot as usize, span, ledger)?;
        } else {
            self.spans.push(span, ledger)?;
        }
        self.ids.take(slot, ledger)?;
        self.fill(place, hasher, slot + 1, ledger)?;
        self.value_bytes += value.len() as u64;
        Ok(slot + 1)
    }

    /// An empty dictionary with room for the values `plan` saw, every id slot free, for
    /// loading: [`load`](Self::load) then stores the values, in the order the plan saw
    /// them. It allocates exactly [`sized_cost`](Self::sized_cost).
    pub(crate) fn sized(plan: &LoadPlan, ledger: &mut Ledger) -> Self {
        Self {
            heap: Heap::sized(plan.bytes as usize, ledger),
            spans: Paged::filled(plan.slots as usize, Span::default(), ledger)
                .expect("the loader admitted its arrays"),
            ids: Ids::free(plan.slots, ledger),
            table: Paged::filled(table_slots(plan.values), EMPTY, ledger)
                .expect("the loader admitted its arrays"),
            occupied: 0,
            value_bytes: 0,
        }
    }

    pub(crate) fn sized_cost(plan: &LoadPlan) -> u64 {
        Heap::sized_cost(plan.bytes as usize)
            + Paged::<Span>::filled_cost(plan.slots as usize)
            + Ids::slots_cost(plan.slots)
            + Paged::<u32>::filled_cost(table_slots(plan.values))
    }

    /// Stores `value`, whose hash is `hash`, under `id` while loading, after the values
    /// loaded before it. Values come in the order the plan of [`sized`](Self::sized) counted
    /// them, so loading allocates nothing. A value that repeats an earlier one is refused.
    ///
    /// # Panics
    ///
    /// If `id` has no free slot, or the table or the heap has no room left: the plan
    /// counted fewer values.
    pub(crate) fn load(
        &mut self,
        hasher: &ValueHasher,
        hash: u64,
        id: u32,
        value: &[u8],
        ledger: &mut Ledger,
    ) -> Result<(), CatalogProblem> {
        if let Some(first) = self.find(hash, value) {
            return Err(CatalogProblem::DuplicateValue { first });
        }
        let place = self.place(hash, 1);
        assert!(
            matches!(place, Place::Slot(_)),
            "the table of a loaded dictionary has room for every value"
        );
        let slot = id - 1;
        let span = self.heap.load(value, ledger);
        self.spans
            .set(slot as usize, span, ledger)
            .expect("loading owns its arrays");
        self.ids.load(slot, ledger);
        self.fill(place, hasher, id, ledger)
            .expect("loading owns its arrays");
        self.value_bytes += value.len() as u64;
        Ok(())
    }

    /// The bytes [`release`](Self::release) allocates for `id`.
    pub(crate) fn release_cost(&self, hasher: &ValueHasher, id: u32) -> u64 {
        let slot = id - 1;
        self.ids.release_cost(slot) + self.table.set_cost(self.position(hasher, id))
    }

    /// Frees `id`. Its bytes stay in the heap as a hole.
    ///
    /// # Panics
    ///
    /// If `id` is not live.
    pub(crate) fn release(
        &mut self,
        hasher: &ValueHasher,
        id: u32,
        ledger: &mut Ledger,
    ) -> Result<(), BudgetExceeded> {
        assert!(self.is_live(id), "releasing value {id}, which is not live");
        let index = self.position(hasher, id);
        self.value_bytes -= self.value(id - 1).len() as u64;
        self.ids.release(id - 1, ledger)?;
        self.table.set(index, TOMBSTONE, ledger)?;
        Ok(())
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.heap.pages.bytes() + self.spans.bytes() + self.ids.bytes() + self.table.bytes()
    }

    /// The live ids, ascending.
    pub(crate) fn live_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.ids.live_slots().map(|slot| slot + 1)
    }

    fn value(&self, slot: u32) -> &[u8] {
        self.heap.get(self.spans.get(slot as usize))
    }

    /// Where `added` more values go: past 3/4 occupied, the table is rebuilt for the live
    /// values at most half full, which also drops its tombstones. Otherwise a value takes
    /// the first empty or tombstone slot of its probe sequence; it is absent, so the probe
    /// meets no match.
    fn place(&self, hash: u64, added: usize) -> Place {
        let slots = self.table.len();
        if (self.occupied + added) * 4 > slots * 3 {
            let live = self.ids.live() as usize + added;
            return Place::Rebuild((2 * live).next_power_of_two().max(MIN_TABLE));
        }
        let mask = slots - 1;
        let mut index = hash as usize & mask;
        while !matches!(self.table.get(index), EMPTY | TOMBSTONE) {
            index = (index + 1) & mask;
        }
        Place::Slot(index)
    }

    /// Puts `id`, which is live, in the table where `place` says.
    fn fill(
        &mut self,
        place: Place,
        hasher: &ValueHasher,
        id: u32,
        ledger: &mut Ledger,
    ) -> Result<(), BudgetExceeded> {
        match place {
            Place::Rebuild(slots) => self.rebuild(slots, hasher, ledger)?,
            Place::Slot(index) => {
                if self.table.get(index) == EMPTY {
                    self.occupied += 1;
                }
                self.table.set(index, id, ledger)?;
            }
        }
        Ok(())
    }

    /// Rebuilds the table with `slots` slots, from every live id, including one just taken.
    fn rebuild(
        &mut self,
        slots: usize,
        hasher: &ValueHasher,
        ledger: &mut Ledger,
    ) -> Result<(), BudgetExceeded> {
        let mut table = Paged::filled(slots, EMPTY, ledger)?;
        let mask = slots - 1;
        for slot in self.ids.live_slots() {
            let mut index = hasher.hash(self.value(slot)) as usize & mask;
            while table.get(index) != EMPTY {
                index = (index + 1) & mask;
            }
            // The new table is the writer's own, so this allocates nothing.
            table.set(index, slot + 1, ledger)?;
        }
        std::mem::replace(&mut self.table, table).discard(ledger);
        self.occupied = self.ids.live() as usize;
        Ok(())
    }

    /// The table slot holding `id`, which is live.
    fn position(&self, hasher: &ValueHasher, id: u32) -> usize {
        let mask = self.table.len() - 1;
        let mut index = hasher.hash(self.value(id - 1)) as usize & mask;
        while self.table.get(index) != id {
            index = (index + 1) & mask;
        }
        index
    }
}

#[cfg(test)]
impl Dictionary {
    pub(crate) fn unshared(&self, base: &Self) -> u64 {
        self.heap.pages.unshared(&base.heap.pages)
            + self.spans.unshared(&base.spans)
            + self.ids.unshared(&base.ids)
            + self.table.unshared(&base.table)
    }

    /// Whether both share every page.
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.heap.pages.same(&other.heap.pages)
            && self.spans.same(&other.spans)
            && self.table.same(&other.table)
    }

    pub(crate) fn gaps(&self) -> Vec<u32> {
        self.ids.gaps().into_iter().map(|slot| slot + 1).collect()
    }

    pub(crate) fn table_slots(&self) -> usize {
        self.table.len()
    }

    /// The length of each page of the heap.
    pub(crate) fn heap_page_lens(&self) -> Vec<usize> {
        (0..self.heap.pages.count())
            .map(|page| self.heap.pages.page(page).len())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A replacement table is admitted beside the old one before any allocation or retirement.
    #[test]
    fn staging_covers_the_old_and_replacement_tables_together() {
        let budget = crate::budget::MemoryBudget::new(64 * 1024);
        let account = crate::budget::MemoryAccount::new(
            &budget,
            "staging",
            crate::budget::MemoryPlan::default(),
        )
        .unwrap();
        let mut ledger = Ledger::staging(&account).unwrap();
        let hasher = ValueHasher::fixed_keys();
        let mut dictionary = Dictionary::new();
        for n in 0..6u8 {
            let value = [n];
            dictionary
                .insert(&hasher, hasher.hash(&value), &value, &mut ledger)
                .unwrap();
        }
        assert_eq!(dictionary.table.len(), 8);
        let old = dictionary.table.clone();
        let allocated = ledger.allocated();
        let overlap = ledger.owned() + Paged::<u32>::filled_cost(16);
        assert!(overlap > budget.reserved());
        let occupied = budget
            .try_reserve("other staging work", 64 * 1024 - (overlap - 1))
            .unwrap();
        dictionary.rebuild(16, &hasher, &mut ledger).unwrap_err();
        assert!(dictionary.table.same(&old));
        assert_eq!(ledger.allocated(), allocated);
        for n in 0..6u8 {
            assert_eq!(
                dictionary.find(hasher.hash(&[n]), &[n]),
                Some(u32::from(n) + 1)
            );
        }
        drop(occupied);
        dictionary.rebuild(16, &hasher, &mut ledger).unwrap();
        assert_eq!(ledger.peak(), overlap);
        assert_eq!(budget.reserved(), overlap);
        assert_eq!(dictionary.table.len(), 16);
        drop(old);
        drop(dictionary);
        drop(ledger);
        drop(account);
        assert_eq!(budget.reserved(), 0);
    }

    fn insert(dictionary: &mut Dictionary, hasher: &ValueHasher, value: &[u8]) -> u32 {
        let mut ledger = Ledger::default();
        let hash = hasher.hash(value);
        let cost = dictionary.insert_cost(hash, value).unwrap();
        let expected = dictionary.next_id().unwrap();
        let id = dictionary.insert(hasher, hash, value, &mut ledger).unwrap();
        assert_eq!(ledger.allocated(), cost, "cost of inserting {value:?}");
        assert_eq!(id, expected);
        id
    }

    fn find(dictionary: &Dictionary, hasher: &ValueHasher, value: &[u8]) -> Option<u32> {
        dictionary.find(hasher.hash(value), value)
    }

    /// Values of every shape: empty, short, a full page, past a page, and `small` small
    /// ones.
    fn values(small: usize) -> Vec<Vec<u8>> {
        let mut values = vec![
            Vec::new(),
            b"(other)".to_vec(),
            vec![b'x'; PAGE_PAYLOAD],
            vec![b'y'; PAGE_PAYLOAD + 1],
            vec![b'z'; 3 * PAGE_PAYLOAD],
        ];
        values.extend((0..small).map(|n| format!("host-{n}.example").into_bytes()));
        values
    }

    #[test]
    fn values_round_trip_through_ids_and_lookups() {
        // With one hash for every value, each lookup walks the whole probe sequence, so
        // that case stays small.
        for (hasher, small) in [
            (ValueHasher::random(), 3000),
            (ValueHasher::fixed(|_| 7), 300),
        ] {
            let mut dictionary = Dictionary::new();
            let values = values(small);
            for (index, value) in values.iter().enumerate() {
                assert_eq!(insert(&mut dictionary, &hasher, value), index as u32 + 1);
            }
            for (index, value) in values.iter().enumerate() {
                let id = index as u32 + 1;
                assert_eq!(dictionary.get(id), Some(&value[..]));
                assert_eq!(find(&dictionary, &hasher, value), Some(id));
            }
            assert_eq!(dictionary.get(0), None);
            assert_eq!(dictionary.get(values.len() as u32 + 1), None);
            assert_eq!(find(&dictionary, &hasher, b"absent"), None);
            assert_eq!(find(&dictionary, &hasher, b"host-1.exampl"), None);
        }
    }

    #[test]
    fn colliding_values_are_told_apart_by_their_bytes() {
        // Every value hashes to the same slot, so lookups walk one long probe sequence.
        let hasher = ValueHasher::fixed(|_| 0);
        let mut dictionary = Dictionary::new();
        for n in 0..200u32 {
            insert(&mut dictionary, &hasher, &n.to_le_bytes());
        }
        for n in 0..200u32 {
            assert_eq!(find(&dictionary, &hasher, &n.to_le_bytes()), Some(n + 1));
        }
        // Releasing one in the middle leaves a tombstone the others are still found past.
        let mut ledger = Ledger::default();
        dictionary.release(&hasher, 50, &mut ledger).unwrap();
        assert_eq!(find(&dictionary, &hasher, &49u32.to_le_bytes()), None);
        for n in (0..200u32).filter(|&n| n != 49) {
            assert_eq!(find(&dictionary, &hasher, &n.to_le_bytes()), Some(n + 1));
        }
    }

    #[test]
    fn released_ids_are_reused_smallest_first_and_find_their_new_value() {
        let hasher = ValueHasher::random();
        let mut dictionary = Dictionary::new();
        for n in 0..10 {
            insert(&mut dictionary, &hasher, format!("v{n}").as_bytes());
        }
        let mut ledger = Ledger::default();
        for id in [7, 3] {
            let cost = dictionary.release_cost(&hasher, id);
            let before = ledger.allocated();
            dictionary.release(&hasher, id, &mut ledger).unwrap();
            assert_eq!(ledger.allocated() - before, cost);
        }
        assert_eq!(dictionary.get(3), None);
        assert_eq!(find(&dictionary, &hasher, b"v2"), None);
        assert_eq!(dictionary.gaps(), [3, 7]);

        assert_eq!(insert(&mut dictionary, &hasher, b"new"), 3);
        assert_eq!(insert(&mut dictionary, &hasher, b"v2"), 7);
        assert_eq!(insert(&mut dictionary, &hasher, b"next"), 11);
        assert_eq!(dictionary.get(3), Some(&b"new"[..]));
        assert_eq!(find(&dictionary, &hasher, b"v2"), Some(7));
    }

    #[test]
    fn tombstones_count_toward_the_load_and_a_rebuild_drops_them() {
        let hasher = ValueHasher::random();
        let mut dictionary = Dictionary::new();
        let mut ledger = Ledger::default();
        // Six values fill the eight-slot table to 3/4.
        for n in 0..6 {
            insert(&mut dictionary, &hasher, format!("v{n}").as_bytes());
        }
        assert_eq!(dictionary.table_slots(), 8);
        for id in 1..=5 {
            dictionary.release(&hasher, id, &mut ledger).unwrap();
        }
        assert_eq!(dictionary.occupied, 6);
        // The next insert would pass 3/4 counting tombstones, so the table is rebuilt for
        // the two live values: still eight slots, now without tombstones.
        insert(&mut dictionary, &hasher, b"w");
        assert_eq!(dictionary.table_slots(), 8);
        assert_eq!(dictionary.occupied, 2);
        assert_eq!(find(&dictionary, &hasher, b"v5"), Some(6));
        assert_eq!(find(&dictionary, &hasher, b"w"), Some(1));
    }

    /// Loads `values`, (id, value) in ascending ids, as the catalog's loader does.
    fn load(hasher: &ValueHasher, values: &[(u32, &[u8])], ledger: &mut Ledger) -> Dictionary {
        let mut plan = LoadPlan::default();
        for &(id, value) in values {
            plan.add(id, value.len()).unwrap();
        }
        let mut dictionary = Dictionary::sized(&plan, ledger);
        for &(id, value) in values {
            let hash = hasher.hash(value);
            dictionary.load(hasher, hash, id, value, ledger).unwrap();
        }
        // Every part was allocated at its final size, and loading wrote it in place.
        assert_eq!(ledger.allocated(), Dictionary::sized_cost(&plan));
        assert_eq!(ledger.owned(), dictionary.bytes());
        assert_eq!(dictionary.value_bytes, plan.bytes);
        dictionary
    }

    #[test]
    fn loading_restores_ids_with_their_gaps() {
        let hasher = ValueHasher::random();
        let mut ledger = Ledger::default();
        let mut dictionary = load(&hasher, &[(2, &b"b"[..]), (3, b""), (9, b"i")], &mut ledger);
        assert_eq!(dictionary.get(2), Some(&b"b"[..]));
        assert_eq!(dictionary.get(3), Some(&b""[..]));
        assert_eq!(find(&dictionary, &hasher, b"i"), Some(9));
        assert_eq!(dictionary.gaps(), [1, 4, 5, 6, 7, 8]);
        assert_eq!(dictionary.next_id(), Some(1));
        assert_eq!(
            dictionary.load(&hasher, hasher.hash(b"b"), 10, b"b", &mut ledger),
            Err(CatalogProblem::DuplicateValue { first: 2 })
        );
    }

    #[test]
    fn a_loaded_heap_is_one_page_of_exactly_its_values() {
        let hasher = ValueHasher::random();
        let long = vec![b'l'; PAGE_PAYLOAD + 1];
        let short: Vec<Vec<u8>> = (0..3).map(|n| vec![n; 2000]).collect();
        let values = [
            (1, &short[0][..]),
            (2, &long),
            (3, &short[1]),
            (4, b""),
            (5, &short[2]),
        ];
        let mut dictionary = load(&hasher, &values, &mut Ledger::default());
        assert_eq!(dictionary.heap_page_lens(), [6000 + PAGE_PAYLOAD + 1]);
        for &(id, value) in &values {
            assert_eq!(dictionary.get(id), Some(value));
            assert_eq!(find(&dictionary, &hasher, value), Some(id));
        }
        // The heap is full, so the next value starts a page of its own.
        assert_eq!(insert(&mut dictionary, &hasher, b"next"), 6);
        assert_eq!(
            dictionary.heap_page_lens(),
            [6000 + PAGE_PAYLOAD + 1, PAGE_PAYLOAD]
        );
        assert_eq!(dictionary.get(5), Some(&short[2][..]));
        assert_eq!(dictionary.get(6), Some(&b"next"[..]));

        // A short loaded page grows as the first page of a writer's heap does.
        let mut short = load(&hasher, &[(1, &b"abc"[..])], &mut Ledger::default());
        assert_eq!(short.heap_page_lens(), [3]);
        insert(&mut short, &hasher, b"de");
        assert_eq!(short.heap_page_lens(), [6]);
        assert_eq!(short.get(1), Some(&b"abc"[..]));
    }

    /// The spans of a loaded dictionary end in a short page past the first; inserting
    /// fills it before starting another.
    #[test]
    fn inserting_after_loading_more_than_a_page_of_spans() {
        let hasher = ValueHasher::random();
        let loaded = Paged::<Span>::PER_PAGE as u32 + 269;
        let values: Vec<Vec<u8>> = (1..=loaded).map(|n| format!("v{n}").into_bytes()).collect();
        let pairs: Vec<(u32, &[u8])> = (1..=loaded).zip(values.iter().map(Vec::as_slice)).collect();
        let mut dictionary = load(&hasher, &pairs, &mut Ledger::default());
        for n in loaded + 1..=2 * loaded {
            assert_eq!(
                insert(&mut dictionary, &hasher, format!("v{n}").as_bytes()),
                n
            );
        }
        for n in 1..=2 * loaded {
            assert_eq!(dictionary.get(n), Some(format!("v{n}").as_bytes()));
        }
    }

    /// A load plan is three numbers whatever the values: planning a load holds nothing per
    /// value. It refuses values that together pass what a dictionary holds.
    #[test]
    fn a_load_plan_counts_the_values_without_holding_them() {
        assert!(!std::mem::needs_drop::<LoadPlan>());
        let mut plan = LoadPlan::default();
        plan.add(3, PAGE_PAYLOAD + 1).unwrap();
        plan.add(7, 0).unwrap();
        plan.add(8, 10).unwrap();
        let bytes = PAGE_PAYLOAD as u64 + 11;
        assert_eq!((plan.slots, plan.values, plan.bytes), (8, 3, bytes));
        let mut full = LoadPlan::default();
        full.add(1, u32::MAX as usize).unwrap();
        assert_eq!(
            full.add(2, 1),
            Err(CatalogProblem::ValueBytes {
                bytes: MAX_VALUE_BYTES + 1
            })
        );
        full.add(2, 0).unwrap();
    }

    /// A writer stores no more value bytes than a loaded heap holds: it counts the live
    /// values' bytes, and a value past them does not fit, as when the ids run out.
    #[test]
    fn a_dictionary_refuses_values_past_what_a_loaded_heap_holds() {
        let hasher = ValueHasher::random();
        let mut dictionary = Dictionary::new();
        for value in [&b"ab"[..], b"cde", b""] {
            insert(&mut dictionary, &hasher, value);
        }
        assert_eq!(dictionary.value_bytes, 5);
        dictionary
            .release(&hasher, 2, &mut Ledger::default())
            .unwrap();
        assert_eq!(dictionary.value_bytes, 2);
        // As if the other values took all but a byte.
        dictionary.value_bytes = MAX_VALUE_BYTES - 1;
        assert_eq!(dictionary.insert_cost(hasher.hash(b"fg"), b"fg"), None);
        assert!(dictionary.insert_cost(hasher.hash(b"f"), b"f").is_some());
        dictionary
            .release(&hasher, 1, &mut Ledger::default())
            .unwrap();
        assert!(dictionary.insert_cost(hasher.hash(b"fg"), b"fg").is_some());
    }

    #[test]
    fn a_loaded_table_is_the_smallest_that_holds_the_values() {
        assert_eq!(table_slots(0), 0);
        assert_eq!(table_slots(1), MIN_TABLE);
        assert_eq!(table_slots(6), 8);
        assert_eq!(table_slots(7), 16);
        assert_eq!(table_slots(49_152), 65_536);
        assert_eq!(table_slots(49_153), 131_072);
        // An inserting writer's table at the same counts: it rebuilds past 3/4.
        let hasher = ValueHasher::random();
        let mut dictionary = Dictionary::new();
        for n in 0..1536u32 {
            insert(&mut dictionary, &hasher, &n.to_be_bytes());
            assert_eq!(
                table_slots(n + 1),
                dictionary.table_slots(),
                "{} values",
                n + 1
            );
        }
    }
}
