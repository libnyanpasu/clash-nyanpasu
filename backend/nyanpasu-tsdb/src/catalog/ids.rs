//! Which ids of a dictionary, or of the combinations, are live. Id 0 is reserved in both
//! (the overflow marker, the whole-overflow combination) and has no slot: id `n` is slot
//! `n - 1`. The slots in use form a prefix; each is live or free, and a new id takes the
//! smallest free slot before a fresh one (§5.3).

use std::iter;

use super::paged::{Ledger, Paged};
use crate::budget::BudgetExceeded;

/// The largest id: `u32::MAX` marks a deleted slot in a dictionary's hash table.
pub(crate) const MAX_ID: u32 = u32::MAX - 1;

/// One bit per slot, set while the id is live.
#[derive(Clone)]
pub(crate) struct Ids {
    bits: Paged<u64>,
    /// Slots in use, live or free: ids `1..=slots`.
    slots: u32,
    free: u32,
    /// No free slot lies below it, so the search for the smallest starts here.
    hint: u32,
}

impl Ids {
    pub(crate) const fn new() -> Self {
        Self {
            bits: Paged::new(),
            slots: 0,
            free: 0,
            hint: 0,
        }
    }

    /// `slots` slots, every one free, for loading: [`load`](Self::load) then makes the
    /// stored ids live. It allocates exactly [`slots_cost`](Self::slots_cost).
    pub(crate) fn free(slots: u32, ledger: &mut Ledger) -> Self {
        Self {
            bits: Paged::filled(slots.div_ceil(64) as usize, 0, ledger)
                .expect("the loader admitted its live bits"),
            slots,
            free: slots,
            hint: 0,
        }
    }

    pub(crate) fn slots(&self) -> u32 {
        self.slots
    }

    pub(crate) fn live(&self) -> u32 {
        self.slots - self.free
    }

    pub(crate) fn is_live(&self, slot: u32) -> bool {
        slot < self.slots && self.bits.get(word(slot)) & bit(slot) != 0
    }

    /// The slot the next id takes: the smallest free one, or else a fresh one; `None` once
    /// the ids run out.
    pub(crate) fn next_slot(&self) -> Option<u32> {
        if self.free == 0 {
            return (self.slots < MAX_ID).then_some(self.slots);
        }
        let mut word_index = word(self.hint);
        let mut candidates = !0u64 << (self.hint % 64);
        loop {
            let clear = !self.bits.get(word_index) & candidates;
            if clear != 0 {
                // `free > 0` puts a free slot at or above the hint and below `slots`, so the
                // first clear bit is that slot, not one past the end.
                return Some(word_index as u32 * 64 + clear.trailing_zeros());
            }
            word_index += 1;
            candidates = !0;
        }
    }

    /// The bytes [`take`](Self::take) allocates for `slot`.
    pub(crate) fn take_cost(&self, slot: u32) -> u64 {
        if slot == self.slots && slot.is_multiple_of(64) {
            self.bits.push_cost()
        } else {
            self.bits.set_cost(word(slot))
        }
    }

    /// Makes `slot` live.
    ///
    /// `slot` has to be [`next_slot`](Self::next_slot): ids are reused smallest first, and
    /// the search for the smallest relies on it. Debug builds check it.
    pub(crate) fn take(&mut self, slot: u32, ledger: &mut Ledger) -> Result<(), BudgetExceeded> {
        debug_assert_eq!(
            Some(slot),
            self.next_slot(),
            "ids are taken smallest free first"
        );
        if slot < self.slots {
            self.free -= 1;
            // `slot` was the smallest free slot, so every slot up to it is live now.
            self.hint = slot + 1;
        } else {
            self.extend(ledger)?;
        }
        self.set_bit(slot, true, ledger)?;
        Ok(())
    }

    /// The bytes [`release`](Self::release) allocates for `slot`.
    pub(crate) fn release_cost(&self, slot: u32) -> u64 {
        self.bits.set_cost(word(slot))
    }

    /// # Panics
    ///
    /// If `slot` is not live.
    pub(crate) fn release(&mut self, slot: u32, ledger: &mut Ledger) -> Result<(), BudgetExceeded> {
        assert!(
            self.is_live(slot),
            "releasing slot {slot}, which is not live"
        );
        self.set_bit(slot, false, ledger)?;
        self.free += 1;
        self.hint = self.hint.min(slot);
        Ok(())
    }

    /// Makes free `slot` live while loading. Only the loader holds the bits, so it allocates
    /// nothing.
    ///
    /// # Panics
    ///
    /// If `slot` is past the slots, or live already.
    pub(crate) fn load(&mut self, slot: u32, ledger: &mut Ledger) {
        assert!(
            slot < self.slots && !self.is_live(slot),
            "loading slot {slot}, which is not a free one of {} slots",
            self.slots
        );
        self.set_bit(slot, true, ledger)
            .expect("loading owns its live bits");
        self.free -= 1;
    }

    /// The live slots, in ascending order.
    pub(crate) fn live_slots(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.bits.len()).flat_map(move |index| {
            iter_bits(self.bits.get(index)).map(move |bit| index as u32 * 64 + bit)
        })
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.bits.bytes()
    }

    /// The least memory the live bits of `slots` slots take.
    pub(crate) fn slots_cost(slots: u32) -> u64 {
        Paged::<u64>::filled_cost(slots.div_ceil(64) as usize)
    }

    /// Adds a free slot at the end. The bits past `slots` are all clear.
    fn extend(&mut self, ledger: &mut Ledger) -> Result<(), BudgetExceeded> {
        if self.slots.is_multiple_of(64) {
            self.bits.push(0, ledger)?;
        }
        self.slots += 1;
        Ok(())
    }

    fn set_bit(
        &mut self,
        slot: u32,
        live: bool,
        ledger: &mut Ledger,
    ) -> Result<(), BudgetExceeded> {
        let index = word(slot);
        let word = self.bits.get(index);
        let word = if live {
            word | bit(slot)
        } else {
            word & !bit(slot)
        };
        self.bits.set(index, word, ledger)?;
        Ok(())
    }
}

#[cfg(test)]
impl Ids {
    pub(crate) fn unshared(&self, base: &Self) -> u64 {
        self.bits.unshared(&base.bits)
    }

    /// The free slots below the highest live one: what loading restores. Free slots above
    /// it are indistinguishable from fresh ones.
    pub(crate) fn gaps(&self) -> Vec<u32> {
        let top = self.live_slots().last().map_or(0, |slot| slot + 1);
        (0..top).filter(|&slot| !self.is_live(slot)).collect()
    }
}

fn word(slot: u32) -> usize {
    (slot / 64) as usize
}

fn bit(slot: u32) -> u64 {
    1 << (slot % 64)
}

/// The positions of the set bits of `word`, lowest first.
fn iter_bits(mut word: u64) -> impl Iterator<Item = u32> {
    iter::from_fn(move || {
        (word != 0).then(|| {
            let bit = word.trailing_zeros();
            word &= word - 1;
            bit
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(ids: &mut Ids, ledger: &mut Ledger) -> u32 {
        let slot = ids.next_slot().unwrap();
        let cost = ids.take_cost(slot);
        let before = ledger.allocated();
        ids.take(slot, ledger).unwrap();
        assert_eq!(ledger.allocated() - before, cost);
        slot
    }

    #[test]
    fn free_slots_are_reused_smallest_first() {
        let mut ledger = Ledger::default();
        let mut ids = Ids::new();
        for expected in 0..200 {
            assert_eq!(take(&mut ids, &mut ledger), expected);
        }
        for slot in [150, 3, 64, 70, 199] {
            ids.release(slot, &mut ledger).unwrap();
        }
        assert_eq!(ids.live(), 195);
        let reused: Vec<u32> = (0..6).map(|_| take(&mut ids, &mut ledger)).collect();
        assert_eq!(reused, [3, 64, 70, 150, 199, 200]);

        // A release below the last reuse moves the search back.
        ids.release(10, &mut ledger).unwrap();
        ids.release(5, &mut ledger).unwrap();
        assert_eq!(take(&mut ids, &mut ledger), 5);
        assert_eq!(take(&mut ids, &mut ledger), 10);
        assert_eq!(take(&mut ids, &mut ledger), 201);
    }

    #[test]
    fn live_slots_lists_exactly_the_live_ones() {
        let mut ledger = Ledger::default();
        let mut ids = Ids::new();
        for _ in 0..130 {
            take(&mut ids, &mut ledger);
        }
        for slot in [0, 63, 64, 129] {
            ids.release(slot, &mut ledger).unwrap();
        }
        let live: Vec<u32> = ids.live_slots().collect();
        let expected: Vec<u32> = (0..130)
            .filter(|slot| ![0, 63, 64, 129].contains(slot))
            .collect();
        assert_eq!(live, expected);
        assert_eq!(ids.gaps(), [0, 63, 64]);
    }

    #[test]
    fn loading_leaves_gaps_free() {
        let mut ledger = Ledger::default();
        let mut ids = Ids::free(201, &mut ledger);
        for slot in [2, 3, 70, 200] {
            ids.load(slot, &mut ledger);
        }
        // The slots were allocated up front, and loading wrote them in place.
        assert_eq!(ledger.allocated(), Ids::slots_cost(201));
        assert_eq!(ids.slots(), 201);
        assert_eq!(ids.live(), 4);
        assert_eq!(ids.live_slots().collect::<Vec<_>>(), [2, 3, 70, 200]);
        assert_eq!(take(&mut ids, &mut ledger), 0);
        assert_eq!(take(&mut ids, &mut ledger), 1);
        assert_eq!(take(&mut ids, &mut ledger), 4);
    }

    /// Loaded bits end in a short page past the first; fresh slots fill it before another
    /// starts.
    #[test]
    fn taking_fresh_slots_after_loading_more_than_a_page_of_bits() {
        let mut ledger = Ledger::default();
        let slots = (Paged::<u64>::PER_PAGE as u32 + 1) * 64;
        let mut ids = Ids::free(slots, &mut ledger);
        for slot in 0..slots {
            ids.load(slot, &mut ledger);
        }
        for expected in slots..slots + 2 * 64 {
            assert_eq!(take(&mut ids, &mut ledger), expected);
        }
        assert!((0..slots + 2 * 64).all(|slot| ids.is_live(slot)));
    }

    #[test]
    fn ids_run_out_at_the_largest_id() {
        let mut ids = Ids::new();
        ids.slots = MAX_ID;
        assert_eq!(ids.next_slot(), None);
        ids.slots = MAX_ID - 1;
        assert_eq!(ids.next_slot(), Some(MAX_ID - 1));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "smallest free first")]
    fn taking_another_slot_than_the_next_panics() {
        let mut ledger = Ledger::default();
        let mut ids = Ids::new();
        take(&mut ids, &mut ledger);
        take(&mut ids, &mut ledger);
        ids.release(0, &mut ledger).unwrap();
        ids.take(2, &mut ledger).unwrap();
    }
}
