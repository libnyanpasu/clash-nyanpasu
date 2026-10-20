//! Series accumulators (§4.2): one per open (tier, bucket), holding the sums of the
//! bucket's journal records in SoA layout sorted by combination id. Space is allocated only
//! for the combinations seen in the bucket.
//!
//! An accumulator is paged by combination-id range, each page behind its own `Arc`, so the
//! version a batch derives copies only the pages of the ranges it touches and shares the
//! rest (§7.1). Pages cover fixed ranges of ids (see [`RANGE_SHIFT`]), so an accumulator's
//! pages depend only on its contents, never on the order the deltas arrived in: the head a
//! reopen rebuilds from the journal is the head the writes built.

#[cfg(test)]
use std::collections::BTreeMap;
use std::{iter, sync::Arc};

use super::ARC_HEADER;
use crate::Timestamp;

/// Combination ids per page: `id >> RANGE_SHIFT` numbers the page. A full page of a series
/// with F fields holds `RANGE_IDS * (4 + 8F)` bytes of sums, 7 KiB for traffic's three
/// fields, which is what a batch copies when it touches one combination of the range.
const RANGE_SHIFT: u32 = 8;
#[cfg(test)]
const RANGE_IDS: u32 = 1 << RANGE_SHIFT;

/// The sums of one combination-id range: ids ascending, and one column per field.
#[derive(Debug, PartialEq, Eq)]
struct Page {
    combos: Box<[u32]>,
    fields: Box<[Box<[u64]>]>,
}

impl Page {
    /// `base` plus the deltas of `combos`, which all lie in `base`'s range, with their fields
    /// in `values` as a journal record lays them out. Equal ids add field by field,
    /// saturating.
    ///
    /// It allocates exactly its [`bytes`](Self::bytes): the ids and each column are
    /// allocated once, at the merged length, and boxing them reallocates nothing.
    fn merged(base: Option<&Page>, combos: &[u32], values: &[u64], fields: usize) -> Self {
        let merge = PageMerge::new(base, combos, values, fields);
        Self {
            combos: merge.ids().into_boxed_slice(),
            fields: (0..fields)
                .map(|field| merge.column(field).into_boxed_slice())
                .collect(),
        }
    }

    fn bytes(&self) -> u64 {
        page_bytes(self.combos.len(), self.fields.len())
    }
}

/// The merge of a base page and the deltas of its range, one column at a time.
struct PageMerge<'a> {
    base_combos: &'a [u32],
    base_fields: &'a [Box<[u64]>],
    combos: &'a [u32],
    values: &'a [u64],
    fields: usize,
    /// The combinations of the merged page.
    len: usize,
}

impl<'a> PageMerge<'a> {
    fn new(base: Option<&'a Page>, combos: &'a [u32], values: &'a [u64], fields: usize) -> Self {
        let (base_combos, base_fields): (&[u32], &[Box<[u64]>]) = match base {
            Some(page) => (&page.combos, &page.fields),
            None => (&[], &[]),
        };
        Self {
            base_combos,
            base_fields,
            combos,
            values,
            fields,
            len: merge_rows(base_combos, combos).count(),
        }
    }

    /// The merged ids, in a vector of exactly their number.
    fn ids(&self) -> Vec<u32> {
        exact(
            self.len,
            merge_rows(self.base_combos, self.combos).map(|(combo, _, _)| combo),
        )
    }

    /// The merged sums of `field`, in a vector of exactly their number.
    fn column(&self, field: usize) -> Vec<u64> {
        let old = self
            .base_fields
            .get(field)
            .map_or(&[][..], |column| &column[..]);
        let sums = merge_rows(self.base_combos, self.combos).map(|(_, base, delta)| {
            let old = base.map_or(0, |row| old[row]);
            let new = delta.map_or(0, |row| self.values[row * self.fields + field]);
            old.saturating_add(new)
        });
        exact(self.len, sums)
    }
}

/// The rows of the merge of two ascending lists of distinct ids, in order: each id, with its
/// index in `base` and in `combos` where it has one.
fn merge_rows<'a>(
    base: &'a [u32],
    combos: &'a [u32],
) -> impl Iterator<Item = (u32, Option<usize>, Option<usize>)> + 'a {
    let (mut old, mut new) = (0, 0);
    iter::from_fn(move || {
        let combo = *base.get(old).into_iter().chain(combos.get(new)).min()?;
        let in_base = (base.get(old) == Some(&combo)).then_some(old);
        let in_combos = (combos.get(new) == Some(&combo)).then_some(new);
        old += usize::from(in_base.is_some());
        new += usize::from(in_combos.is_some());
        Some((combo, in_base, in_combos))
    })
}

/// A vector of the `len` items `items` yields, allocated once at that length, so that
/// boxing it reallocates nothing.
fn exact<T>(len: usize, items: impl Iterator<Item = T>) -> Vec<T> {
    let mut vector = Vec::with_capacity(len);
    vector.extend(items);
    debug_assert_eq!(vector.len(), len, "the items fill the vector exactly");
    vector
}

/// A page of the accumulator that merging deltas into a base makes.
enum Slot<'a> {
    /// A page of the base that no delta touches, shared.
    Shared(&'a (u32, Arc<Page>)),
    /// Range `range`, which the deltas `combos`, with their fields in `values`, touch, merged
    /// into the base's page of it, if any.
    Merged {
        range: u32,
        base: Option<&'a Page>,
        combos: &'a [u32],
        values: &'a [u64],
    },
}

/// The pages of `base`, or of an empty accumulator, plus `deltas`, in range order.
fn slots<'a>(
    base: Option<&'a Accumulator>,
    deltas: &'a SourceDeltas,
    fields: usize,
) -> impl Iterator<Item = Slot<'a>> + 'a {
    let base_pages: &[(u32, Arc<Page>)] = base.map_or(&[], |base| &base.pages);
    let mut old = base_pages.iter().peekable();
    let mut start = 0;
    iter::from_fn(move || {
        let Some(&first) = deltas.combos.get(start) else {
            return old.next().map(Slot::Shared);
        };
        let range = first >> RANGE_SHIFT;
        if let Some(page) = old.next_if(|(number, _)| *number < range) {
            return Some(Slot::Shared(page));
        }
        let end =
            start + deltas.combos[start..].partition_point(|&combo| combo >> RANGE_SHIFT == range);
        let base = old
            .next_if(|(number, _)| *number == range)
            .map(|(_, page)| &**page);
        let slot = Slot::Merged {
            range,
            base,
            combos: &deltas.combos[start..end],
            values: &deltas.fields[start * fields..end * fields],
        };
        start = end;
        Some(slot)
    })
}

/// The allocations of a page of `len` combinations and `fields` fields: its `Arc`, the ids,
/// the column pointers and the columns.
fn page_bytes(len: usize, fields: usize) -> u64 {
    (ARC_HEADER
        + size_of::<Page>()
        + len * size_of::<u32>()
        + fields * (size_of::<Box<[u64]>>() + len * size_of::<u64>())) as u64
}

/// What [`Accumulator::merged`] makes, known before it allocates anything, so that the
/// memory can be reserved first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MergeCost {
    /// The merged accumulator's bytes, the pages it shares with its base included.
    pub(crate) bytes: u64,
    /// What the merge allocates: the accumulator's own allocations and the pages it
    /// rewrites. Until the base is dropped, both are alive.
    pub(crate) copied: u64,
    /// The combinations of the merged accumulator.
    pub(crate) len: usize,
}

/// The deltas of one source, in a journal record's layout: ids strictly ascending, and the
/// fields of `combos[i]` at `fields[i * F..(i + 1) * F]`.
#[cfg_attr(test, derive(Clone))]
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SourceDeltas {
    pub(crate) combos: Vec<u32>,
    pub(crate) fields: Vec<u64>,
}

impl SourceDeltas {
    /// The storage for `combos` ids and their `fields` values.
    pub(crate) fn bytes(combos: u64, fields: usize) -> u64 {
        combos.saturating_mul((size_of::<u32>() + fields * size_of::<u64>()) as u64)
    }
}

/// Sums or deltas of one source, one combination at a time in ascending order, each written
/// into the caller's buffer of one value per field, so that reading them allocates nothing.
pub(crate) trait SourceRows {
    /// The next combination, with its values written into `fields`, or `None` after the last.
    fn next_into(&mut self, fields: &mut [u64]) -> Option<u32>;
}

/// The sums of an accumulator plus deltas, one combination at a time in ascending order, read
/// without building them: what [`Accumulator::merged`] of the two would hold. Either may be
/// missing.
#[derive(Clone, Debug)]
pub(crate) struct Sums<'a> {
    pages: &'a [(u32, Arc<Page>)],
    page: usize,
    row: usize,
    deltas: Option<&'a SourceDeltas>,
    delta: usize,
    fields: usize,
}

impl<'a> Sums<'a> {
    pub(crate) fn new(
        base: Option<&'a Accumulator>,
        deltas: Option<&'a SourceDeltas>,
        fields: usize,
    ) -> Self {
        Self {
            pages: base.map_or(&[], |base| &base.pages),
            page: 0,
            row: 0,
            deltas,
            delta: 0,
            fields,
        }
    }
}

impl SourceRows for Sums<'_> {
    fn next_into(&mut self, fields: &mut [u64]) -> Option<u32> {
        // A page is never empty: a merge makes one only for the deltas of its range.
        let base = self
            .pages
            .get(self.page)
            .map(|(_, page)| page.combos[self.row]);
        let delta = self
            .deltas
            .and_then(|deltas| deltas.combos.get(self.delta).copied());
        let combo = base.into_iter().chain(delta).min()?;
        fields.fill(0);
        if base == Some(combo) {
            let page = &self.pages[self.page].1;
            for (value, column) in fields.iter_mut().zip(&page.fields) {
                *value = column[self.row];
            }
            self.row += 1;
            if self.row == page.combos.len() {
                (self.page, self.row) = (self.page + 1, 0);
            }
        }
        if let Some(deltas) = self.deltas
            && delta == Some(combo)
        {
            let values = &deltas.fields[self.delta * self.fields..][..self.fields];
            for (value, &delta) in fields.iter_mut().zip(values) {
                *value = value.saturating_add(delta);
            }
            self.delta += 1;
        }
        Some(combo)
    }
}

/// Sums deltas per combination, saturating, in any order. Saturating addition of unsigned
/// values is associative, so summing a batch first and then adding it to an accumulator
/// gives what adding each delta would.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct DeltaSums {
    sums: BTreeMap<u32, Box<[u64]>>,
}

#[cfg(test)]
impl DeltaSums {
    pub(crate) fn add(&mut self, combo: u32, fields: &[u64]) {
        let sums = self
            .sums
            .entry(combo)
            .or_insert_with(|| vec![0; fields.len()].into_boxed_slice());
        for (sum, &value) in sums.iter_mut().zip(fields) {
            *sum = sum.saturating_add(value);
        }
    }

    pub(crate) fn into_deltas(self) -> SourceDeltas {
        let mut deltas = SourceDeltas::default();
        for (combo, fields) in self.sums {
            deltas.combos.push(combo);
            deltas.fields.extend_from_slice(&fields);
        }
        deltas
    }
}

/// What the records a head update adds to a source say about it, besides the deltas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceUpdate {
    /// The `now` of the latest batch among the records.
    pub(crate) written: Timestamp,
    pub(crate) records: u64,
    pub(crate) journal_bytes: u64,
}

/// The sums of one open (tier, bucket), and what chunking needs to know about its journal
/// records.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Accumulator {
    /// By range number, ascending.
    pages: Box<[(u32, Arc<Page>)]>,
    /// The combinations of all pages.
    len: usize,
    /// The `now` of the last batch that wrote to the source.
    updated: Timestamp,
    /// The source's journal records, and their bytes, keys included.
    records: u64,
    journal_bytes: u64,
    /// Every allocation the accumulator reaches, shared with other versions or not.
    bytes: u64,
}

impl Accumulator {
    /// `base`, or an empty accumulator, plus `deltas`. Pages whose range no delta touches
    /// are shared with `base`.
    pub(crate) fn merged(
        base: Option<&Accumulator>,
        deltas: &SourceDeltas,
        update: SourceUpdate,
        fields: usize,
    ) -> Self {
        let pages = Self::merged_pages(base, deltas, fields);
        let (records, journal_bytes) = match base {
            Some(base) => (base.records, base.journal_bytes),
            None => (0, 0),
        };
        Self::new(
            pages.into_boxed_slice(),
            update.written,
            records.saturating_add(update.records),
            journal_bytes.saturating_add(update.journal_bytes),
        )
    }

    /// The pages of [`merged`](Self::merged), in a vector of exactly their number.
    fn merged_pages(
        base: Option<&Accumulator>,
        deltas: &SourceDeltas,
        fields: usize,
    ) -> Vec<(u32, Arc<Page>)> {
        let pages = slots(base, deltas, fields).map(|slot| match slot {
            Slot::Shared(page) => page.clone(),
            Slot::Merged {
                range,
                base,
                combos,
                values,
            } => (range, Arc::new(Page::merged(base, combos, values, fields))),
        });
        exact(slots(base, deltas, fields).count(), pages)
    }

    /// What [`merged`](Self::merged) with the same arguments allocates and leaves.
    pub(crate) fn merge_cost(
        base: Option<&Accumulator>,
        deltas: &SourceDeltas,
        fields: usize,
    ) -> MergeCost {
        let (mut pages, mut shared, mut rewritten, mut combos) = (0, 0, 0, 0);
        for slot in slots(base, deltas, fields) {
            pages += 1;
            match slot {
                Slot::Shared((_, page)) => {
                    shared += page.bytes();
                    combos += page.combos.len();
                }
                Slot::Merged {
                    base,
                    combos: added,
                    values,
                    ..
                } => {
                    let len = PageMerge::new(base, added, values, fields).len;
                    rewritten += page_bytes(len, fields);
                    combos += len;
                }
            }
        }
        let own = Self::own_bytes(pages);
        MergeCost {
            bytes: own + shared + rewritten,
            copied: own + rewritten,
            len: combos,
        }
    }

    fn new(
        pages: Box<[(u32, Arc<Page>)]>,
        updated: Timestamp,
        records: u64,
        journal_bytes: u64,
    ) -> Self {
        Self {
            len: pages.iter().map(|(_, page)| page.combos.len()).sum(),
            bytes: Self::own_bytes(pages.len())
                + pages.iter().map(|(_, page)| page.bytes()).sum::<u64>(),
            pages,
            updated,
            records,
            journal_bytes,
        }
    }

    /// The accumulator's own allocations: its `Arc`, its struct and its list of pages.
    fn own_bytes(pages: usize) -> u64 {
        (ARC_HEADER + size_of::<Accumulator>() + pages * size_of::<(u32, Arc<Page>)>()) as u64
    }

    /// Reports concrete copied layouts to a construction probe, excluding shared pages.
    #[cfg(test)]
    pub(crate) fn observe_copies(
        &self,
        base: Option<&Accumulator>,
        account: &crate::budget::MemoryAccount,
    ) {
        account.constructed(
            (ARC_HEADER + size_of::<Self>() + self.pages.len() * size_of::<(u32, Arc<Page>)>())
                as u64,
        );
        for (range, page) in &self.pages {
            let shared = base.is_some_and(|base| {
                base.pages
                    .binary_search_by_key(range, |(range, _)| *range)
                    .is_ok_and(|index| Arc::ptr_eq(page, &base.pages[index].1))
            });
            if !shared {
                let columns: usize = page
                    .fields
                    .iter()
                    .map(|column| std::mem::size_of_val(&**column))
                    .sum();
                account.constructed(
                    (ARC_HEADER
                        + size_of::<Page>()
                        + std::mem::size_of_val(&*page.combos)
                        + std::mem::size_of_val(&*page.fields)
                        + columns) as u64,
                );
            }
        }
    }

    /// The same sums after folding replaced the source's records with one record of
    /// `journal_bytes` bytes (§4.2). Every page is shared.
    pub(crate) fn folded(&self, journal_bytes: u64) -> Self {
        Self::new(self.pages.clone(), self.updated, 1, journal_bytes)
    }

    /// The bytes of the smallest accumulator: one combination of `fields` fields.
    pub(crate) fn smallest_bytes(fields: usize) -> u64 {
        Self::own_bytes(1) + page_bytes(1, fields)
    }

    /// What [`folded`](Self::folded) allocates: the accumulator's own allocations.
    pub(crate) fn folded_bytes(&self) -> u64 {
        Self::own_bytes(self.pages.len())
    }

    /// [`folded`](Self::folded), keeping this accumulator's allocations.
    pub(crate) fn into_folded(self, journal_bytes: u64) -> Self {
        Self {
            records: 1,
            journal_bytes,
            ..self
        }
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The bytes of this accumulator that `base`, the same source's accumulator in another
    /// version, does not share: its own allocations unless it is `base` itself, and each page
    /// `base` does not hold for the same range.
    #[cfg(test)]
    pub(crate) fn copied_from(&self, base: Option<&Accumulator>) -> u64 {
        if base.is_some_and(|base| std::ptr::eq(self, base)) {
            return 0;
        }
        let base_pages = base.map_or(&[][..], |base| &base.pages);
        let copied: u64 = self
            .pages
            .iter()
            .filter(|(number, page)| {
                let shared = base_pages
                    .binary_search_by_key(number, |(other, _)| *other)
                    .is_ok_and(|index| Arc::ptr_eq(page, &base_pages[index].1));
                !shared
            })
            .map(|(_, page)| page.bytes())
            .sum();
        Self::own_bytes(self.pages.len()) + copied
    }

    /// The number of combinations with sums.
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// The `now` of the last batch that wrote to the source.
    pub(crate) fn updated(&self) -> Timestamp {
        self.updated
    }

    /// The source's journal records.
    pub(crate) fn records(&self) -> u64 {
        self.records
    }

    /// The bytes of the source's journal records, keys included.
    pub(crate) fn journal_bytes(&self) -> u64 {
        self.journal_bytes
    }

    /// The sums in combination order, a page at a time: the ids, and one column per field.
    #[allow(dead_code, reason = "the series query reads the head's sums (§8.1)")]
    pub(crate) fn pages(&self) -> impl Iterator<Item = (&[u32], &[Box<[u64]>])> {
        self.pages
            .iter()
            .map(|(_, page)| (&page.combos[..], &page.fields[..]))
    }

    /// Every combination with its sums, ascending.
    #[cfg(test)]
    pub(crate) fn entries(&self) -> Vec<(u32, Vec<u64>)> {
        self.pages
            .iter()
            .flat_map(|(_, page)| {
                page.combos.iter().enumerate().map(|(index, &combo)| {
                    let fields = page.fields.iter().map(|column| column[index]).collect();
                    (combo, fields)
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const FIELDS: usize = 2;

    fn update(records: u64) -> SourceUpdate {
        SourceUpdate {
            written: Timestamp(records as i64),
            records,
            journal_bytes: 10 * records,
        }
    }

    fn deltas(entries: &[(u32, [u64; FIELDS])]) -> SourceDeltas {
        let mut sums = DeltaSums::default();
        for (combo, fields) in entries {
            sums.add(*combo, fields);
        }
        sums.into_deltas()
    }

    #[test]
    fn deltas_add_up_saturating_and_merge_in_id_order() {
        let first = Accumulator::merged(
            None,
            &deltas(&[(300, [1, 2]), (5, [u64::MAX, 1]), (5, [1, 1])]),
            update(1),
            FIELDS,
        );
        assert_eq!(first.entries(), [(5, vec![u64::MAX, 2]), (300, vec![1, 2])]);
        let second = Accumulator::merged(
            Some(&first),
            &deltas(&[(4, [7, 7]), (5, [1, 3]), (300, [1, 1]), (301, [0, 9])]),
            update(1),
            FIELDS,
        );
        // The batch sums saturate, and so does adding a batch to the accumulator.
        assert_eq!(
            second.entries(),
            [
                (4, vec![7, 7]),
                (5, vec![u64::MAX, 5]),
                (300, vec![2, 3]),
                (301, vec![0, 9])
            ]
        );
        assert_eq!(
            (second.updated(), second.records(), second.journal_bytes()),
            (Timestamp(1), 2, 20)
        );
        assert_eq!(second.len(), 4);
    }

    #[test]
    fn a_merge_copies_only_the_pages_it_touches() {
        let base = Accumulator::merged(
            None,
            &deltas(&[
                (1, [1, 1]),
                (RANGE_IDS + 1, [1, 1]),
                (3 * RANGE_IDS, [1, 1]),
            ]),
            update(1),
            FIELDS,
        );
        let next = Accumulator::merged(
            Some(&base),
            &deltas(&[(RANGE_IDS + 2, [1, 1])]),
            update(1),
            FIELDS,
        );
        let numbers: Vec<u32> = next.pages.iter().map(|(number, _)| *number).collect();
        assert_eq!(numbers, [0, 1, 3]);
        let page = |number: usize| next.pages[number].1.bytes();
        assert_eq!(
            next.copied_from(Some(&base)),
            Accumulator::own_bytes(3) + page(1)
        );
        assert_eq!(next.copied_from(Some(&next)), 0);
        assert_eq!(next.copied_from(None), next.bytes());
        // Folding keeps every page and the sums.
        let folded = next.folded(7);
        assert_eq!(folded.entries(), next.entries());
        assert_eq!(
            (folded.records(), folded.journal_bytes(), folded.bytes()),
            (1, 7, next.bytes())
        );
        assert_eq!(folded.copied_from(Some(&next)), Accumulator::own_bytes(3));
        let shared: Vec<bool> = base
            .pages
            .iter()
            .zip(next.pages.iter())
            .map(|((_, a), (_, b))| Arc::ptr_eq(a, b))
            .collect();
        assert_eq!(shared, [true, false, true]);
    }

    #[test]
    fn bytes_count_every_allocation() {
        let accumulator = Accumulator::merged(
            None,
            &deltas(&[(1, [1, 1]), (2, [1, 1]), (RANGE_IDS, [1, 1])]),
            update(1),
            FIELDS,
        );
        let arc = ARC_HEADER as u64;
        // A page of n ids: its Arc and struct, n ids, two column pointers, 2n sums.
        let page = |n: u64| arc + size_of::<Page>() as u64 + 4 * n + 2 * (16 + 8 * n);
        let own = arc + size_of::<Accumulator>() as u64 + 2 * size_of::<(u32, Arc<Page>)>() as u64;
        assert_eq!(accumulator.bytes(), own + page(2) + page(1));
    }

    /// Codex's case: combinations 0 to 255, all in one range, merged into an empty
    /// accumulator and then once more into the one holding them. Each merge allocates its
    /// list of pages, and the page's ids and columns, once, at the lengths they end with, so
    /// the cost reserved before it is what it allocates.
    #[test]
    fn a_merge_allocates_its_pages_and_columns_at_their_final_lengths() {
        let entries: Vec<(u32, [u64; FIELDS])> =
            (0..RANGE_IDS).map(|combo| (combo, [1, 1])).collect();
        let deltas = deltas(&entries);
        let combos = RANGE_IDS as usize;
        let mut accumulator: Option<Accumulator> = None;
        for round in 1..=2 {
            let base = accumulator.as_ref();
            let pages = Accumulator::merged_pages(base, &deltas, FIELDS);
            let page = base.map(|base| &*base.pages[0].1);
            let merge = PageMerge::new(page, &deltas.combos, &deltas.fields, FIELDS);
            let ids = merge.ids();
            let columns: Vec<Vec<u64>> = (0..FIELDS).map(|field| merge.column(field)).collect();
            assert_eq!(pages.capacity(), 1, "round {round}");
            assert_eq!(ids.capacity(), combos, "round {round}");
            for column in &columns {
                assert_eq!(column.capacity(), combos, "round {round}");
                assert_eq!(column, &vec![round; combos]);
            }
            // The accumulator's own allocations, then the page's: its `Arc`, its ids, and
            // each column with its pointer.
            let own = ARC_HEADER
                + size_of::<Accumulator>()
                + pages.capacity() * size_of::<(u32, Arc<Page>)>();
            let page = ARC_HEADER
                + size_of::<Page>()
                + ids.capacity() * size_of::<u32>()
                + columns
                    .iter()
                    .map(|column| size_of::<Box<[u64]>>() + column.capacity() * size_of::<u64>())
                    .sum::<usize>();
            let cost = Accumulator::merge_cost(base, &deltas, FIELDS);
            assert_eq!(cost.copied, (own + page) as u64, "round {round}");
            accumulator = Some(Accumulator::merged(base, &deltas, update(1), FIELDS));
        }
    }

    proptest! {
        // However the deltas are split into batches, the accumulator ends up the same: the
        // property the head's rebuild from the journal relies on.
        #[test]
        fn the_split_into_batches_does_not_change_the_result(
            entries in prop::collection::vec(
                (0..4 * RANGE_IDS, prop::array::uniform2(prop_oneof![Just(u64::MAX), 0..10u64])),
                1..200,
            ),
            cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..5),
        ) {
            let mut cuts: Vec<usize> = cuts.iter().map(|cut| cut.index(entries.len())).collect();
            cuts.sort_unstable();
            let mut accumulator: Option<Accumulator> = None;
            let mut start = 0;
            for end in cuts.into_iter().chain([entries.len()]) {
                let batch = deltas(&entries[start..end]);
                // The cost is known before the merge, and is what the merge then takes.
                let cost = Accumulator::merge_cost(accumulator.as_ref(), &batch, FIELDS);
                let merged = Accumulator::merged(accumulator.as_ref(), &batch, update(1), FIELDS);
                prop_assert_eq!(cost.bytes, merged.bytes());
                prop_assert_eq!(cost.copied, merged.copied_from(accumulator.as_ref()));
                accumulator = Some(merged);
                start = end;
            }
            let whole_deltas = deltas(&entries);
            let whole = Accumulator::merged(None, &whole_deltas, update(1), FIELDS);
            let cost = Accumulator::merge_cost(None, &whole_deltas, FIELDS);
            prop_assert_eq!(cost.bytes, whole.bytes());
            prop_assert_eq!(cost.copied, whole.copied_from(None));
            let split = accumulator.unwrap();
            prop_assert_eq!(split.entries(), whole.entries());
            prop_assert_eq!(&split.pages, &whole.pages);
            prop_assert_eq!(split.bytes(), whole.bytes());
        }
    }
}
