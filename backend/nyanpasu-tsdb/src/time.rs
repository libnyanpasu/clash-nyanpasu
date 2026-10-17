//! Time: milliseconds since the Unix epoch, half-open ranges, and bucket alignment that
//! rounds the way the traffic store does today (§8.4).
//!
//! Buckets and extents are counted from the epoch, so a timestamp's bucket is
//! `div_euclid` of its milliseconds by the bucket width. Negative timestamps floor toward
//! minus infinity and get negative bucket numbers; nothing clamps at the epoch.
//!
//! Overflow never panics. A function that returns a bucket number is total, because
//! dividing by a positive width cannot overflow. A function that returns a timestamp
//! returns `None` when the result does not fit in an `i64`, and a saturating subtraction
//! stops at the end of the `i64` range, as `nyanpasu-traffic` does.

#![allow(dead_code, reason = "the modules that use the bucket math come later")]

use std::{ops::Range, time::Duration};

use snafu::{OptionExt, Snafu, ensure};

use crate::ErrorKind;

const NANOS_PER_MILLI: u32 = 1_000_000;

/// Milliseconds since the Unix epoch. Negative values are before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub i64);

/// A half-open range `[start, end)` of timestamps, with `start <= end`. Empty when they
/// are equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimeRange {
    start: Timestamp,
    end: Timestamp,
}

impl TimeRange {
    pub fn new(start: Timestamp, end: Timestamp) -> Result<Self, ReversedRange> {
        ensure!(start <= end, ReversedRangeSnafu { start, end });
        Ok(Self { start, end })
    }

    pub fn start(self) -> Timestamp {
        self.start
    }

    /// The first timestamp after the range.
    pub fn end(self) -> Timestamp {
        self.end
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn contains(self, timestamp: Timestamp) -> bool {
        self.start <= timestamp && timestamp < self.end
    }
}

/// A range whose start is after its end.
#[derive(Debug, Snafu, PartialEq, Eq)]
#[snafu(display("range starts after it ends: {} > {}", start.0, end.0))]
#[snafu(visibility(pub(crate)))]
pub struct ReversedRange {
    pub start: Timestamp,
    pub end: Timestamp,
}

impl ReversedRange {
    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Local
    }
}

/// A duration that is not a positive, whole number of milliseconds.
#[derive(Debug, Snafu, PartialEq, Eq)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum DurationError {
    #[snafu(display("duration is zero"))]
    Zero,

    /// Bucket, extent and window widths are whole milliseconds, so a finer duration would
    /// round differently from the value the application wrote.
    #[snafu(display("duration {duration:?} has a sub-millisecond part"))]
    SubMillisecond { duration: Duration },

    #[snafu(display("duration {duration:?} does not fit in an i64 of milliseconds"))]
    TooLong { duration: Duration },
}

impl DurationError {
    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Local
    }
}

/// A positive whole number of milliseconds: a bucket width, an extent width, or how far
/// back a range reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Span(i64);

impl Span {
    /// `None` unless `millis` is positive.
    pub const fn new(millis: i64) -> Option<Self> {
        if millis > 0 { Some(Self(millis)) } else { None }
    }

    /// The checked conversion schema validation uses (§2): a zero, sub-millisecond or
    /// oversized duration is an error, never rounded.
    pub fn from_duration(duration: Duration) -> Result<Self, DurationError> {
        ensure!(!duration.is_zero(), ZeroSnafu);
        ensure!(
            duration.subsec_nanos().is_multiple_of(NANOS_PER_MILLI),
            SubMillisecondSnafu { duration }
        );
        let millis = i64::try_from(duration.as_millis())
            .ok()
            .context(TooLongSnafu { duration })?;
        Ok(Self(millis))
    }

    pub const fn millis(self) -> i64 {
        self.0
    }

    /// The number of the bucket (or extent) of this width that holds `timestamp`.
    pub fn index_of(self, timestamp: Timestamp) -> i64 {
        timestamp.0.div_euclid(self.0)
    }

    /// Where bucket number `index` starts, or `None` if that is not an `i64`.
    pub fn start_of(self, index: i64) -> Option<Timestamp> {
        index.checked_mul(self.0).map(Timestamp)
    }

    /// The start of the bucket that holds `timestamp`: the timestamp aligned down.
    pub fn floor(self, timestamp: Timestamp) -> Option<Timestamp> {
        self.start_of(self.index_of(timestamp))
    }

    /// The time covered by bucket number `index`, or `None` if either end is not an `i64`.
    pub fn window(self, index: i64) -> Option<TimeRange> {
        let start = self.start_of(index)?;
        let end = Timestamp(start.0.checked_add(self.0)?);
        Some(TimeRange { start, end })
    }

    /// The numbers of the buckets that overlap `range`, as a half-open range of numbers.
    /// A bucket the range touches only partly counts whole, and an empty range covers
    /// none.
    pub fn covered(self, range: TimeRange) -> Range<i64> {
        let first = self.index_of(range.start);
        if range.is_empty() {
            return first..first;
        }
        let last = range.end.0;
        let after = match last.rem_euclid(self.0) {
            0 => last.div_euclid(self.0),
            // A remainder means the width is at least 2, so the quotient is at most
            // `i64::MAX / 2` and adding one cannot overflow.
            _ => last.div_euclid(self.0) + 1,
        };
        first..after
    }

    /// The first bucket of a range that reaches `lookback` back from `now`, as
    /// `nyanpasu-traffic` rounds it (`bucket.rs`, `TrafficRange::start`): the bucket that
    /// holds `now - lookback` is included whole, so the range spans up to one bucket more
    /// than its nominal length.
    ///
    /// The traffic store clamps this at its first bucket because its bucket numbers are
    /// unsigned; here a range reaching before the epoch has a negative first bucket, and
    /// a consumer with unsigned numbers clamps it itself.
    pub fn lookback_index(self, now: Timestamp, lookback: Span) -> i64 {
        self.index_of(Timestamp(now.0.saturating_sub(lookback.0)))
    }

    /// Where [`lookback_index`](Self::lookback_index) begins, in milliseconds.
    pub fn lookback_start(self, now: Timestamp, lookback: Span) -> Option<Timestamp> {
        self.start_of(self.lookback_index(now, lookback))
    }
}

/// The time grid of one series tier: buckets, grouped into extents that are a whole
/// number of buckets (§1). Both count from the epoch, so an extent never splits a bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Grid {
    bucket: Span,
    extent: Span,
    buckets_per_extent: i64,
}

impl Grid {
    /// `None` unless `extent` is a whole multiple of `bucket`.
    pub fn new(bucket: Span, extent: Span) -> Option<Self> {
        (extent.0 % bucket.0 == 0).then(|| Self {
            bucket,
            extent,
            buckets_per_extent: extent.0 / bucket.0,
        })
    }

    pub fn bucket(self) -> Span {
        self.bucket
    }

    pub fn extent(self) -> Span {
        self.extent
    }

    /// The number of the extent that holds bucket number `bucket`.
    pub fn extent_of_bucket(self, bucket: i64) -> i64 {
        bucket.div_euclid(self.buckets_per_extent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE_MS: i64 = 60_000;
    const HOUR_MS: i64 = 60 * MINUTE_MS;

    fn span(millis: i64) -> Span {
        Span::new(millis).unwrap()
    }

    fn range(start: i64, end: i64) -> TimeRange {
        TimeRange::new(Timestamp(start), Timestamp(end)).unwrap()
    }

    #[test]
    fn a_range_is_half_open() {
        let range = range(10, 20);
        assert!(!range.contains(Timestamp(9)));
        assert!(range.contains(Timestamp(10)));
        assert!(range.contains(Timestamp(19)));
        assert!(!range.contains(Timestamp(20)));
        assert_eq!((range.start(), range.end()), (Timestamp(10), Timestamp(20)));
    }

    #[test]
    fn a_range_cannot_run_backwards() {
        let error = TimeRange::new(Timestamp(2), Timestamp(1)).unwrap_err();
        assert_eq!(
            error,
            ReversedRange {
                start: Timestamp(2),
                end: Timestamp(1)
            }
        );
        assert_eq!(error.kind(), ErrorKind::Local);
        assert_eq!(error.to_string(), "range starts after it ends: 2 > 1");
    }

    #[test]
    fn an_empty_range_contains_nothing_and_covers_no_bucket() {
        let empty = range(5, 5);
        assert!(empty.is_empty());
        assert!(!empty.contains(Timestamp(5)));
        // Both off a bucket boundary and on one, and below the epoch.
        for at in [5, 10, 0, -5, -10, i64::MIN, i64::MAX] {
            assert!(
                span(10).covered(range(at, at)).is_empty(),
                "an empty range at {at} covers a bucket"
            );
        }
        assert!(!range(5, 6).is_empty());
    }

    #[test]
    fn widths_are_positive_whole_milliseconds() {
        assert_eq!(Span::new(0), None);
        assert_eq!(Span::new(-1), None);
        assert_eq!(Span::new(1).map(Span::millis), Some(1));
        assert_eq!(
            Span::from_duration(Duration::from_secs(60)),
            Ok(span(MINUTE_MS))
        );
        assert_eq!(Span::from_duration(Duration::from_millis(1)), Ok(span(1)));
    }

    #[test]
    fn durations_that_are_not_whole_milliseconds_are_rejected() {
        assert_eq!(
            Span::from_duration(Duration::ZERO),
            Err(DurationError::Zero)
        );

        let fine = Duration::from_micros(1_500);
        assert_eq!(
            Span::from_duration(fine),
            Err(DurationError::SubMillisecond { duration: fine })
        );
        // A duration below one millisecond is sub-millisecond, not zero.
        let tiny = Duration::from_nanos(1);
        assert_eq!(
            Span::from_duration(tiny),
            Err(DurationError::SubMillisecond { duration: tiny })
        );

        let longest = Duration::from_millis(i64::MAX as u64);
        assert_eq!(Span::from_duration(longest), Ok(span(i64::MAX)));
        let too_long = Duration::from_millis(i64::MAX as u64 + 1);
        assert_eq!(
            Span::from_duration(too_long),
            Err(DurationError::TooLong { duration: too_long })
        );
        let huge = Duration::from_secs(u64::MAX);
        assert_eq!(
            Span::from_duration(huge),
            Err(DurationError::TooLong { duration: huge })
        );
        assert_eq!(DurationError::Zero.kind(), ErrorKind::Local);
    }

    #[test]
    fn buckets_flip_exactly_on_the_boundary() {
        let minute = span(MINUTE_MS);
        assert_eq!(minute.index_of(Timestamp(59_999)), 0);
        assert_eq!(minute.index_of(Timestamp(60_000)), 1);
        assert_eq!(minute.index_of(Timestamp(60_001)), 1);
        let hour = span(HOUR_MS);
        assert_eq!(hour.index_of(Timestamp(HOUR_MS - 1)), 0);
        assert_eq!(hour.index_of(Timestamp(HOUR_MS)), 1);
        assert_eq!(hour.index_of(Timestamp(HOUR_MS + 1)), 1);
    }

    #[test]
    fn negative_timestamps_floor_toward_minus_infinity() {
        let minute = span(MINUTE_MS);
        assert_eq!(minute.index_of(Timestamp(-1)), -1);
        assert_eq!(minute.index_of(Timestamp(-MINUTE_MS)), -1);
        assert_eq!(minute.index_of(Timestamp(-MINUTE_MS - 1)), -2);
        assert_eq!(minute.floor(Timestamp(-1)), Some(Timestamp(-MINUTE_MS)));
        assert_eq!(
            minute.floor(Timestamp(-MINUTE_MS)),
            Some(Timestamp(-MINUTE_MS))
        );
        assert_eq!(minute.floor(Timestamp(61_234)), Some(Timestamp(60_000)));
        assert_eq!(minute.floor(Timestamp(0)), Some(Timestamp(0)));
    }

    #[test]
    fn a_bucket_window_is_where_it_starts_and_ends() {
        let minute = span(MINUTE_MS);
        assert_eq!(minute.window(3), Some(range(3 * MINUTE_MS, 4 * MINUTE_MS)));
        assert_eq!(minute.window(-1), Some(range(-MINUTE_MS, 0)));
        assert_eq!(minute.start_of(-2), Some(Timestamp(-2 * MINUTE_MS)));
    }

    #[test]
    fn timestamps_at_the_edge_of_i64_do_not_panic() {
        let minute = span(MINUTE_MS);
        // Bucket numbers are total.
        assert_eq!(
            minute.index_of(Timestamp(i64::MIN)),
            i64::MIN.div_euclid(MINUTE_MS)
        );
        assert_eq!(
            minute.index_of(Timestamp(i64::MAX)),
            i64::MAX.div_euclid(MINUTE_MS)
        );
        // Timestamps are `None` when they do not fit.
        assert_eq!(minute.floor(Timestamp(i64::MIN)), None);
        assert_eq!(minute.start_of(i64::MAX), None);
        assert_eq!(minute.start_of(i64::MIN), None);
        assert_eq!(minute.window(i64::MAX), None);
        // The last bucket whose start fits, but whose end does not.
        let last = i64::MAX.div_euclid(MINUTE_MS);
        assert!(minute.start_of(last).is_some());
        assert_eq!(minute.window(last), None);
        // A width of 1 is the identity and never overflows.
        let ms = span(1);
        assert_eq!(ms.floor(Timestamp(i64::MIN)), Some(Timestamp(i64::MIN)));
        assert_eq!(ms.floor(Timestamp(i64::MAX)), Some(Timestamp(i64::MAX)));
        assert_eq!(ms.window(i64::MAX - 1), Some(range(i64::MAX - 1, i64::MAX)));
        assert_eq!(ms.window(i64::MAX), None);
        // The widest width.
        let widest = span(i64::MAX);
        assert_eq!(widest.index_of(Timestamp(i64::MIN)), -2);
        assert_eq!(widest.index_of(Timestamp(i64::MAX)), 1);
    }

    #[test]
    fn a_range_covers_every_bucket_it_touches() {
        let ten = span(10);
        assert_eq!(ten.covered(range(0, 10)), 0..1);
        assert_eq!(ten.covered(range(0, 11)), 0..2);
        assert_eq!(ten.covered(range(9, 10)), 0..1);
        assert_eq!(ten.covered(range(9, 11)), 0..2);
        assert_eq!(ten.covered(range(10, 30)), 1..3);
        assert_eq!(ten.covered(range(5, 25)), 0..3);
        // Below the epoch.
        assert_eq!(ten.covered(range(-10, 0)), -1..0);
        assert_eq!(ten.covered(range(-11, 0)), -2..0);
        assert_eq!(ten.covered(range(-1, 1)), -1..1);
        assert_eq!(ten.covered(range(-25, -5)), -3..0);
    }

    #[test]
    fn covering_the_whole_i64_range_does_not_overflow() {
        let ms = span(1);
        assert_eq!(ms.covered(range(i64::MIN, i64::MAX)), i64::MIN..i64::MAX);
        let two = span(2);
        assert_eq!(
            two.covered(range(i64::MIN, i64::MAX)),
            i64::MIN / 2..i64::MAX / 2 + 1
        );
        let widest = span(i64::MAX);
        assert_eq!(widest.covered(range(i64::MIN, i64::MAX)), -2..1);
    }

    #[test]
    fn range_starts_align_down_to_the_bucket() {
        // Ported from `nyanpasu-traffic`'s `range_starts_align_down_to_the_bucket`.
        // 1000 hours and 30 minutes after the epoch.
        let now = Timestamp(1000 * HOUR_MS + 30 * MINUTE_MS);
        let minute = span(MINUTE_MS);
        let hour = span(HOUR_MS);
        assert_eq!(
            minute.lookback_index(now, span(HOUR_MS)),
            minute.index_of(Timestamp(999 * HOUR_MS + 30 * MINUTE_MS))
        );
        assert_eq!(
            minute.lookback_index(now, span(6 * HOUR_MS)),
            minute.index_of(Timestamp(994 * HOUR_MS + 30 * MINUTE_MS))
        );
        // The hour bucket that contains now - span is included whole.
        assert_eq!(hour.lookback_index(now, span(24 * HOUR_MS)), 976);
        assert_eq!(hour.lookback_index(now, span(7 * 24 * HOUR_MS)), 832);
        assert_eq!(hour.lookback_index(now, span(30 * 24 * HOUR_MS)), 280);
    }

    #[test]
    fn a_range_starts_on_the_boundary_bucket_just_after_it() {
        // Ported from `a_range_starts_on_the_boundary_bucket_just_after_it`.
        let minute = span(MINUTE_MS);
        let hour = span(HOUR_MS);
        let top_of_hour = 10 * HOUR_MS;
        let first = minute.index_of(Timestamp(top_of_hour));
        // Exactly 1h ago is the start of its own minute bucket; 1 ms later still is.
        assert_eq!(
            minute.lookback_index(Timestamp(top_of_hour + HOUR_MS), hour),
            first
        );
        assert_eq!(
            minute.lookback_index(Timestamp(top_of_hour + HOUR_MS + 1), hour),
            first
        );
        assert_eq!(
            minute.lookback_index(Timestamp(top_of_hour + HOUR_MS + MINUTE_MS), hour),
            first + 1
        );
    }

    #[test]
    fn range_start_times_are_where_their_first_bucket_begins() {
        // Ported from `range_start_times_are_where_their_first_bucket_begins`.
        let now = Timestamp(30 * HOUR_MS + 5 * MINUTE_MS + 7);
        let minute = span(MINUTE_MS);
        let hour = span(HOUR_MS);
        assert_eq!(
            minute.lookback_start(now, hour),
            Some(Timestamp(29 * HOUR_MS + 5 * MINUTE_MS))
        );
        assert_eq!(
            hour.lookback_start(now, span(24 * HOUR_MS)),
            Some(Timestamp(6 * HOUR_MS))
        );
        assert_eq!(
            hour.lookback_start(now, span(30 * 24 * HOUR_MS)),
            Some(Timestamp(-30 * 24 * HOUR_MS + 30 * HOUR_MS))
        );
    }

    #[test]
    fn ranges_reaching_before_the_epoch_start_below_zero() {
        // `nyanpasu-traffic` starts these at bucket 0 because its numbers are unsigned
        // (`ranges_reaching_before_the_epoch_start_at_zero`); here the bucket is real.
        let minute = span(MINUTE_MS);
        assert_eq!(minute.lookback_index(Timestamp(0), span(HOUR_MS)), -60);
        assert_eq!(
            minute.lookback_start(Timestamp(0), span(HOUR_MS)),
            Some(Timestamp(-HOUR_MS))
        );

        // The subtraction saturates instead of overflowing, so the first bucket of an
        // enormous look-back is the one holding `i64::MIN`, which has no start time.
        let hour = span(HOUR_MS);
        let thirty_days = span(30 * 24 * HOUR_MS);
        assert_eq!(
            hour.lookback_index(Timestamp(i64::MIN), thirty_days),
            hour.index_of(Timestamp(i64::MIN))
        );
        assert_eq!(hour.lookback_start(Timestamp(i64::MIN), thirty_days), None);
        // Reaching back from the largest timestamp does not overflow either.
        assert_eq!(
            hour.lookback_index(Timestamp(i64::MAX), span(i64::MAX)),
            hour.index_of(Timestamp(0))
        );
    }

    #[test]
    fn an_extent_is_a_whole_number_of_buckets() {
        assert_eq!(Grid::new(span(MINUTE_MS), span(HOUR_MS + 1)), None);
        assert_eq!(Grid::new(span(MINUTE_MS), span(MINUTE_MS / 2)), None);
        let grid = Grid::new(span(MINUTE_MS), span(HOUR_MS)).unwrap();
        assert_eq!(
            (grid.bucket(), grid.extent()),
            (span(MINUTE_MS), span(HOUR_MS))
        );
        // A bucket as wide as its extent.
        assert!(Grid::new(span(HOUR_MS), span(HOUR_MS)).is_some());
    }

    #[test]
    fn a_bucket_belongs_to_the_extent_that_holds_its_start() {
        let grid = Grid::new(span(MINUTE_MS), span(HOUR_MS)).unwrap();
        assert_eq!(grid.extent_of_bucket(0), 0);
        assert_eq!(grid.extent_of_bucket(59), 0);
        assert_eq!(grid.extent_of_bucket(60), 1);
        assert_eq!(grid.extent_of_bucket(-1), -1);
        assert_eq!(grid.extent_of_bucket(-60), -1);
        assert_eq!(grid.extent_of_bucket(-61), -2);
        assert_eq!(grid.extent_of_bucket(i64::MIN), i64::MIN.div_euclid(60));

        // The extent holding a bucket's start is the extent holding the whole bucket.
        for bucket in -130..130 {
            let start = grid.bucket().start_of(bucket).unwrap();
            assert_eq!(
                grid.extent().index_of(start),
                grid.extent_of_bucket(bucket),
                "bucket {bucket}"
            );
        }
    }
}
