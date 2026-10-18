//! The references a batch accepts (§5.3): every combination and label value its kept points
//! and rows hold, by stream and extent. Staging collects them as it resolves the batch, then
//! records them in the batch's transaction (`layout::references`), so that maintenance knows
//! when nothing references an id any more.
//!
//! The list is reduced to the newest extent of each (stream, entity) before it is recorded, so
//! a batch reads one reference record per pair, and writes it only when the pair is new to the
//! stream or reaches a newer extent. A late point or row that staging dropped references
//! nothing; an accepted one into an older extent leaves a newer record as it is, or records the
//! pair when the stream had none.
//!
//! A tier's combinations skip the list where the head shows the record already: the head is
//! the journal (§4.2), and each journal record's references were recorded when it was written,
//! so a combination the current head holds in the point's bucket has a record of that extent
//! or a newer one. A record goes only once its extent is expired and cleaned, and expiring an
//! extent takes it out of the head. Event buffers are not searched.

use std::cmp::Reverse;

use crate::{
    head::series::Accumulator,
    layout::{
        manifest::StreamRef,
        references::{self, Entity, ReferenceError},
    },
    storage::StorageWrite,
};

/// One reference a batch accepted.
#[derive(Clone, Copy, Debug)]
struct Accepted {
    stream: StreamRef,
    entity: Entity,
    extent: i64,
}

/// The references of one batch, in a list allocated once at the capacity staging reserved.
pub(crate) struct References {
    accepted: Vec<Accepted>,
}

impl References {
    /// The bytes a list of `capacity` references takes.
    pub(crate) fn bytes(capacity: usize) -> u64 {
        (capacity * size_of::<Accepted>()) as u64
    }

    /// An empty list with room for `capacity` references, which [`bytes`](Self::bytes)
    /// reserved.
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            accepted: Vec::with_capacity(capacity),
        }
    }

    /// Notes that data of `extent` of `stream` holds `entity`. Combination 0 and the overflow
    /// marker are never released, so they are not noted.
    ///
    /// # Panics
    ///
    /// In debug builds, past the capacity: the list would reallocate past what was reserved.
    pub(crate) fn push(&mut self, stream: StreamRef, entity: Entity, extent: i64) {
        let (Entity::Combo(id) | Entity::Label(_, id)) = entity;
        if id == 0 {
            return;
        }
        debug_assert!(
            self.accepted.len() < self.accepted.capacity(),
            "more references than staging reserved"
        );
        self.accepted.push(Accepted {
            stream,
            entity,
            extent,
        });
    }

    /// Notes the combinations of a tier's bucket of `extent`, `combos`, ascending as a journal
    /// record's deltas are, but those `current`, the bucket's accumulator in the current head,
    /// holds already. Its pages hold ascending ids too, each page a range of them, so one pass
    /// walks both.
    pub(crate) fn push_tier(
        &mut self,
        stream: StreamRef,
        extent: i64,
        combos: &[u32],
        current: Option<&Accumulator>,
    ) {
        debug_assert!(combos.is_sorted(), "a source's deltas are ascending");
        let mut pages = current
            .into_iter()
            .flat_map(Accumulator::pages)
            .map(|(ids, _)| ids)
            .peekable();
        for &combo in combos {
            // A page that ends before `combo` ends before every later one.
            while pages
                .next_if(|ids| ids.last().is_none_or(|&last| last < combo))
                .is_some()
            {}
            // The head holding `combo` in this bucket implies a record of this extent or a
            // newer one: a bucket leaves the head when its extent expires, before cleanup can
            // forget the record. A change to how long the head keeps a bucket, or to what a
            // reopen replays into it, has to keep this.
            let held = pages
                .peek()
                .is_some_and(|ids| ids.binary_search(&combo).is_ok());
            if !held {
                self.push(stream, Entity::Combo(combo), extent);
            }
        }
    }

    /// Records the references in `batch`: the newest extent of each (stream, entity), with a
    /// read each.
    pub(crate) fn record(mut self, batch: &mut impl StorageWrite) -> Result<(), ReferenceError> {
        // The newest extent of each pair first, so that deduplicating keeps it.
        self.accepted.sort_unstable_by_key(|accepted| {
            (accepted.stream, accepted.entity, Reverse(accepted.extent))
        });
        self.accepted
            .dedup_by_key(|accepted| (accepted.stream, accepted.entity));
        for accepted in &self.accepted {
            references::refer(batch, accepted.entity, accepted.stream, accepted.extent)?;
        }
        Ok(())
    }
}
