//! Series accumulators (§4.2): one per open (tier, bucket), holding the sums of the
//! bucket's journal records in SoA layout sorted by combination id. Space is allocated only
//! for the combinations seen in the bucket.
//!
//! An accumulator is paged by combination-id range, each page behind its own `Arc`, so the
//! version a batch derives copies only the pages of the ranges it touches and shares the
//! rest (§7.1). Pages cover fixed ranges of ids (see [`RANGE_SHIFT`]), so an accumulator's
//! pages depend only on its contents, never on the order the deltas arrived in: the head a
//! reopen rebuilds from the journal is the head the writes built.

use std::{collections::BTreeMap, iter, sync::Arc};

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
    fn merged(base: Option<&Page>, combos: &[u32], values: &[u64], fields: usize) -> Self {
        let (base_combos, base_fields): (&[u32], &[Box<[u64]>]) = match base {
            Some(page) => (&page.combos, &page.fields),
            None => (&[], &[]),
        };
        let capacity = base_combos.len() + combos.len();
        let mut merged = Vec::with_capacity(capacity);
        let mut columns: Vec<Vec<u64>> = iter::repeat_with(|| Vec::with_capacity(capacity))
            .take(fields)
            .collect();
        let (mut old, mut new) = (0, 0);
        while old < base_combos.len() || new < combos.len() {
            let take_old =
                new == combos.len() || (old < base_combos.len() && base_combos[old] <= combos[new]);
            let take_new =
                old == base_combos.len() || (new < combos.len() && combos[new] <= base_combos[old]);
            merged.push(if take_old {
                base_combos[old]
            } else {
                combos[new]
            });
            for (field, column) in columns.iter_mut().enumerate() {
                let old_value = if take_old { base_fields[field][old] } else { 0 };
                let new_value = if take_new {
                    values[new * fields + field]
                } else {
                    0
                };
                column.push(old_value.saturating_add(new_value));
            }
            old += usize::from(take_old);
            new += usize::from(take_new);
        }
        Self {
            combos: merged.into_boxed_slice(),
            fields: columns.into_iter().map(Vec::into_boxed_slice).collect(),
        }
    }

    /// The page's allocations: its `Arc`, the ids, the column pointers and the columns.
    fn bytes(&self) -> u64 {
        let len = self.combos.len();
        (ARC_HEADER
            + size_of::<Page>()
            + len * size_of::<u32>()
            + self.fields.len() * (size_of::<Box<[u64]>>() + len * size_of::<u64>())) as u64
    }
}

/// The deltas of one source, in a journal record's layout: ids strictly ascending, and the
/// fields of `combos[i]` at `fields[i * F..(i + 1) * F]`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SourceDeltas {
    pub(crate) combos: Vec<u32>,
    pub(crate) fields: Vec<u64>,
}

/// Sums deltas per combination, saturating, in any order. Saturating addition of unsigned
/// values is associative, so summing a batch first and then adding it to an accumulator
/// gives what adding each delta would.
#[derive(Debug, Default)]
pub(crate) struct DeltaSums {
    sums: BTreeMap<u32, Box<[u64]>>,
}

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
        let base_pages: &[(u32, Arc<Page>)] = base.map_or(&[], |base| &base.pages);
        let mut old = base_pages.iter().peekable();
        let mut pages = Vec::with_capacity(base_pages.len() + deltas.combos.len());
        let mut start = 0;
        while start < deltas.combos.len() {
            let range = deltas.combos[start] >> RANGE_SHIFT;
            let end = start
                + deltas.combos[start..].partition_point(|&combo| combo >> RANGE_SHIFT == range);
            while let Some(page) = old.next_if(|(number, _)| *number < range) {
                pages.push(page.clone());
            }
            let page = old
                .next_if(|(number, _)| *number == range)
                .map(|(_, page)| &**page);
            let page = Page::merged(
                page,
                &deltas.combos[start..end],
                &deltas.fields[start * fields..end * fields],
                fields,
            );
            pages.push((range, Arc::new(page)));
            start = end;
        }
        pages.extend(old.cloned());

        let (records, journal_bytes) = match base {
            Some(base) => (base.records, base.journal_bytes),
            None => (0, 0),
        };
        let mut accumulator = Self {
            pages: pages.into_boxed_slice(),
            updated: update.written,
            records: records + update.records,
            journal_bytes: journal_bytes + update.journal_bytes,
            bytes: 0,
        };
        accumulator.bytes = (ARC_HEADER
            + size_of::<Accumulator>()
            + accumulator.pages.len() * size_of::<(u32, Arc<Page>)>())
            as u64
            + accumulator
                .pages
                .iter()
                .map(|(_, page)| page.bytes())
                .sum::<u64>();
        accumulator
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
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

    #[cfg(test)]
    pub(crate) fn stats(&self) -> (Timestamp, u64, u64) {
        (self.updated, self.records, self.journal_bytes)
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
        assert_eq!(second.stats(), (Timestamp(1), 2, 20));
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
                accumulator = Some(Accumulator::merged(accumulator.as_ref(), &batch, update(1), FIELDS));
                start = end;
            }
            let whole = Accumulator::merged(None, &deltas(&entries), update(1), FIELDS);
            let split = accumulator.unwrap();
            prop_assert_eq!(split.entries(), whole.entries());
            prop_assert_eq!(&split.pages, &whole.pages);
            prop_assert_eq!(split.bytes(), whole.bytes());
        }
    }
}
