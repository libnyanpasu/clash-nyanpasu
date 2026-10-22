//! Combinations: one column of label ids per series dimension, indexed by combination id,
//! and the record of which ids are live.
//!
//! - Combination 0 is the whole-overflow combination, every dimension the overflow marker.
//!   It always exists, is implicit, and has no slot; id `n` is slot `n - 1`.
//! - A combination a fold step produced is an ordinary one with some dimensions 0 (§5.2).
//! - Ids are reused smallest first (§5.3). A released combination's column entries stay,
//!   stale, until its id is reused.
//! - Columns are `u32`; choosing a narrower width per dimension (§5.1) is deferred.

use super::{
    ids::Ids,
    paged::{Ledger, Paged},
};
use crate::budget::BudgetExceeded;

#[derive(Clone)]
pub(crate) struct Combos {
    columns: Vec<Paged<u32>>,
    ids: Ids,
}

impl Combos {
    pub(crate) fn new(dimensions: usize) -> Self {
        Self {
            columns: (0..dimensions).map(|_| Paged::new()).collect(),
            ids: Ids::new(),
        }
    }

    /// `slots` combination slots of `dimensions` dimensions, every one free, for loading:
    /// [`load`](Self::load) then stores the combinations. It allocates exactly
    /// [`slots_cost`](Self::slots_cost).
    pub(crate) fn free(dimensions: usize, slots: u32, ledger: &mut Ledger) -> Self {
        Self {
            columns: (0..dimensions)
                .map(|_| {
                    Paged::filled(slots as usize, 0, ledger)
                        .expect("the loader admitted its columns")
                })
                .collect(),
            ids: Ids::free(slots, ledger),
        }
    }

    pub(crate) fn dimensions(&self) -> usize {
        self.columns.len()
    }

    /// The id range, including implicit combination 0 and released slots.
    pub(crate) fn slots(&self) -> usize {
        self.ids.slots() as usize + 1
    }

    pub(crate) fn is_live(&self, id: u32) -> bool {
        id.checked_sub(1).is_none_or(|slot| self.ids.is_live(slot))
    }

    /// The label ids of combination `id`, one per series dimension, if it is live.
    pub(crate) fn get(&self, id: u32) -> Option<ComboLabels<'_>> {
        self.is_live(id).then(|| ComboLabels {
            columns: &self.columns,
            slot: id.checked_sub(1).map(|slot| slot as usize),
            position: 0,
        })
    }

    /// Whether `id` is live and has exactly these labels.
    pub(crate) fn matches(&self, id: u32, labels: &[u32]) -> bool {
        self.get(id)
            .is_some_and(|stored| stored.eq(labels.iter().copied()))
    }

    /// The id the next new combination gets, if any is left.
    pub(crate) fn next_id(&self) -> Option<u32> {
        self.ids.next_slot().map(|slot| slot + 1)
    }

    /// The bytes [`insert`](Self::insert) allocates, or `None` when the ids ran out.
    pub(crate) fn insert_cost(&self) -> Option<u64> {
        let slot = self.ids.next_slot()?;
        let columns: u64 = if slot < self.ids.slots() {
            let index = slot as usize;
            self.columns
                .iter()
                .map(|column| column.set_cost(index))
                .sum()
        } else {
            self.columns.iter().map(Paged::push_cost).sum()
        };
        Some(columns + self.ids.take_cost(slot))
    }

    /// Stores a combination of `labels` and returns its id: the one
    /// [`next_id`](Self::next_id) named. The caller makes sure no live combination has
    /// these labels.
    ///
    /// # Panics
    ///
    /// If the ids ran out, or `labels` does not have one id per dimension.
    pub(crate) fn insert(
        &mut self,
        labels: &[u32],
        ledger: &mut Ledger,
    ) -> Result<u32, BudgetExceeded> {
        assert_eq!(labels.len(), self.columns.len(), "one label per dimension");
        let slot = self.ids.next_slot().expect("the ids ran out");
        let fresh = slot == self.ids.slots();
        for (column, &label) in self.columns.iter_mut().zip(labels) {
            if fresh {
                column.push(label, ledger)?;
            } else {
                column.set(slot as usize, label, ledger)?;
            }
        }
        self.ids.take(slot, ledger)?;
        Ok(slot + 1)
    }

    /// Stores combination `id` in its free slot while loading. Only the loader holds the
    /// columns, so it allocates nothing.
    ///
    /// # Panics
    ///
    /// If `id` has no free slot.
    pub(crate) fn load(&mut self, id: u32, labels: &[u32], ledger: &mut Ledger) {
        let slot = id - 1;
        for (column, &label) in self.columns.iter_mut().zip(labels) {
            column
                .set(slot as usize, label, ledger)
                .expect("loading owns its columns");
        }
        self.ids.load(slot, ledger);
    }

    /// The bytes [`release`](Self::release) allocates for `id`.
    pub(crate) fn release_cost(&self, id: u32) -> u64 {
        self.ids.release_cost(id - 1)
    }

    /// Frees `id`. Its column entries stay until the id is reused.
    ///
    /// # Panics
    ///
    /// If `id` is 0 or not live.
    pub(crate) fn release(&mut self, id: u32, ledger: &mut Ledger) -> Result<(), BudgetExceeded> {
        assert!(id != 0, "combination 0 is implicit and never released");
        self.ids.release(id - 1, ledger)?;
        Ok(())
    }

    /// A live combination whose dimension `position` is `label`, if any. It scans every
    /// combination.
    pub(crate) fn user_of(&self, position: usize, label: u32) -> Option<u32> {
        let column = &self.columns[position];
        self.ids
            .live_slots()
            .find(|&slot| column.get(slot as usize) == label)
            .map(|slot| slot + 1)
    }

    /// The live ids, ascending. Combination 0 is not among them.
    pub(crate) fn live_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.ids.live_slots().map(|slot| slot + 1)
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.columns.iter().map(Paged::bytes).sum::<u64>() + self.ids.bytes()
    }

    /// The least memory `slots` id slots of combinations of `dimensions` dimensions take,
    /// live or free: a label in each column and a live bit each.
    pub(crate) fn slots_cost(dimensions: usize, slots: u32) -> u64 {
        dimensions as u64 * Paged::<u32>::filled_cost(slots as usize) + Ids::slots_cost(slots)
    }
}

#[cfg(test)]
impl Combos {
    pub(crate) fn unshared(&self, base: &Self) -> u64 {
        let columns: u64 = self
            .columns
            .iter()
            .zip(&base.columns)
            .map(|(column, base)| column.unshared(base))
            .sum();
        columns + self.ids.unshared(&base.ids)
    }

    pub(crate) fn gaps(&self) -> Vec<u32> {
        self.ids.gaps().into_iter().map(|slot| slot + 1).collect()
    }
}

/// The label ids of one combination, in [`SeriesSpec::dimensions`] order.
///
/// [`SeriesSpec::dimensions`]: crate::SeriesSpec::dimensions
#[derive(Clone)]
pub(crate) struct ComboLabels<'a> {
    columns: &'a [Paged<u32>],
    /// `None` for combination 0, every label of which is the overflow marker.
    slot: Option<usize>,
    position: usize,
}

impl Iterator for ComboLabels<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        let column = self.columns.get(self.position)?;
        self.position += 1;
        Some(self.slot.map_or(0, |slot| column.get(slot)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.columns.len() - self.position;
        (left, Some(left))
    }
}

impl ExactSizeIterator for ComboLabels<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loaded columns end in a short page past the first; fresh combinations fill it before
    /// another starts.
    #[test]
    fn inserting_after_loading_more_than_a_page_of_combinations() {
        let mut ledger = Ledger::default();
        let loaded = Paged::<u32>::PER_PAGE as u32 + 269;
        let mut combos = Combos::free(2, loaded, &mut ledger);
        for id in 1..=loaded {
            combos.load(id, &[id, id + 1], &mut ledger);
        }
        for id in loaded + 1..=2 * loaded {
            let cost = combos.insert_cost().unwrap();
            let before = ledger.allocated();
            assert_eq!(combos.insert(&[id, id + 1], &mut ledger).unwrap(), id);
            assert_eq!(ledger.allocated() - before, cost);
        }
        for id in 1..=2 * loaded {
            assert!(combos.matches(id, &[id, id + 1]));
        }
    }
}
