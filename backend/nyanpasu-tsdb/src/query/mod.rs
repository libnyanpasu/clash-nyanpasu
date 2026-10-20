//! Queries (§8) run outside the owner, on a captured snapshot.
//!
//! - Series queries are grouped sums over a time range.
//! - Event queries are paged scans of a stream.
//!
//! A query's temporary memory is O(groups + decode buffers). It is reserved from a shared
//! budget before the query starts (§7.4, §8.2).
//!
//! Results report their resolution, whether folded data was involved, and whether retention
//! truncated the range (§9.4).

mod events;
mod plan;
mod series;
mod snapshot;

use crate::LabelId;
#[cfg(doc)]
use crate::Snapshot;

pub use self::{
    events::{
        Event, EventCondition, EventCursor, EventDegradation, EventFilter, EventPage,
        EventPredicate, EventQuery, EventScanError, EventValue,
    },
    series::{
        FieldMask, Group, GroupKey, GroupList, GroupRequest, KeyPart, LabelMap, SeriesGroups,
        SeriesQuery, SeriesQueryError,
    },
};

/// Admits a row by its label of one series dimension: one of `ids`, or, when `exclude` is
/// set, none of them. The ids are those of the snapshot's catalog
/// ([`Snapshot::find_label`]); 0 is the overflow marker, which a folded combination holds
/// (§5.2). The list is scanned for each row, so it is meant to be short: the values a user
/// picked, or the one value a topology skips (§8.1).
///
/// A filter built from ids admits the values they named only in a query of a snapshot whose
/// [`Snapshot::catalog`] equals that of the snapshot or result the ids came from (§5.3).
#[derive(Clone, Copy, Debug)]
pub struct LabelFilter<'a> {
    pub dimension: LabelId,
    pub ids: &'a [u32],
    pub exclude: bool,
}

/// Where a query read a row, for error messages: a chunk, or the head (`None`).
fn rows_of(chunk: Option<u64>) -> String {
    chunk.map_or_else(|| "the head".to_owned(), |chunk| format!("chunk {chunk}"))
}
