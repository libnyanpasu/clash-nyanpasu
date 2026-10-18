//! Range planning (§8.4):
//!
//! - rounds a series query's range out to whole buckets of its tier, as the traffic store
//!   does today (`nyanpasu-traffic/src/bucket.rs:30-57`): a bucket the range touches counts
//!   whole;
//! - chooses the chunks of the tier whose extent and key range meet those buckets, from the
//!   manifest, so a chunk outside them is never opened.
//!
//! Skipping blocks by their stats happens as each chunk is read (`series`).

use std::ops::Range;

use crate::{
    TierSpec, TimeRange, Timestamp,
    layout::manifest::Listed,
    time::{Grid, Span},
};

/// The time grid of a tier of a valid schema.
pub(crate) fn tier_grid(tier: &TierSpec) -> Grid {
    let span =
        |duration| Span::from_duration(duration).expect("a valid schema's widths are positive");
    Grid::new(span(tier.bucket), span(tier.extent))
        .expect("a valid schema's extents are whole numbers of buckets")
}

/// A range rounded out to whole buckets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Buckets {
    /// The bucket numbers, half-open; empty for an empty range.
    pub(crate) numbers: Range<i64>,
    /// The time those buckets span: from where the first starts to where the last ends,
    /// clamped to the timestamps an `i64` holds.
    pub(crate) covered: TimeRange,
}

pub(crate) fn round(grid: Grid, range: TimeRange) -> Buckets {
    let numbers = grid.bucket().covered(range);
    let start = |bucket| {
        grid.bucket().start_of(bucket).unwrap_or(if bucket < 0 {
            Timestamp(i64::MIN)
        } else {
            Timestamp(i64::MAX)
        })
    };
    let covered = TimeRange::new(start(numbers.start), start(numbers.end))
        .expect("bucket starts do not decrease");
    Buckets { numbers, covered }
}

impl Buckets {
    /// Whether a chunk of `extent` holding buckets `key_range` (inclusive) has rows in the
    /// buckets: a chunk's buckets lie in its extent, so the extent decides first.
    pub(crate) fn meets(&self, grid: Grid, listed: &Listed) -> bool {
        let Self { numbers, .. } = self;
        if numbers.is_empty() {
            return false;
        }
        let last = numbers.end - 1;
        let extents = grid.extent_of_bucket(numbers.start)..=grid.extent_of_bucket(last);
        let (min, max) = listed.entry.key_range;
        extents.contains(&listed.extent) && min <= last && max >= numbers.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        layout::manifest::ChunkEntry,
        storage::ChunkId,
        time::{Grid, Span},
    };

    fn grid() -> Grid {
        // Buckets of 10 ms, extents of 4 buckets.
        Grid::new(Span::new(10).unwrap(), Span::new(40).unwrap()).unwrap()
    }

    fn range(start: i64, end: i64) -> TimeRange {
        TimeRange::new(Timestamp(start), Timestamp(end)).unwrap()
    }

    fn listed(extent: i64, key_range: (i64, i64)) -> Listed {
        Listed {
            extent,
            chunk: ChunkId(1),
            entry: ChunkEntry {
                kind: 1,
                run: 1,
                key_range,
                rows: 1,
                fragments: 1,
                fragment0: 1,
                bytes: 1,
            },
        }
    }

    #[test]
    fn a_range_rounds_out_to_the_buckets_it_touches() {
        let rounded = round(grid(), range(15, 31));
        assert_eq!(rounded.numbers, 1..4);
        assert_eq!(rounded.covered, range(10, 40));
        // On the boundaries, nothing is added.
        let rounded = round(grid(), range(10, 40));
        assert_eq!((rounded.numbers, rounded.covered), (1..4, range(10, 40)));
        let empty = round(grid(), range(15, 15));
        assert_eq!((empty.numbers, empty.covered), (1..1, range(10, 10)));
        let below = round(grid(), range(-11, -1));
        assert_eq!((below.numbers, below.covered), (-2..0, range(-20, 0)));
    }

    #[test]
    fn rounding_the_whole_timeline_clamps_the_covered_time() {
        let rounded = round(grid(), range(i64::MIN, i64::MAX));
        assert_eq!(rounded.covered, range(i64::MIN, i64::MAX));
        assert_eq!(rounded.numbers, i64::MIN / 10 - 1..i64::MAX / 10 + 1);
    }

    #[test]
    fn a_chunk_meets_the_buckets_when_its_extent_and_key_range_do() {
        let buckets = round(grid(), range(30, 90));
        // Buckets 3..9, extents 0..=2.
        assert!(buckets.meets(grid(), &listed(0, (3, 3))));
        assert!(buckets.meets(grid(), &listed(2, (8, 8))));
        assert!(buckets.meets(grid(), &listed(1, (4, 7))));
        assert!(!buckets.meets(grid(), &listed(0, (0, 2))));
        assert!(!buckets.meets(grid(), &listed(2, (9, 11))));
        // An extent outside, whatever the key range says.
        assert!(!buckets.meets(grid(), &listed(3, (8, 8))));
        let empty = round(grid(), range(30, 30));
        assert!(!empty.meets(grid(), &listed(0, (3, 3))));
    }
}
