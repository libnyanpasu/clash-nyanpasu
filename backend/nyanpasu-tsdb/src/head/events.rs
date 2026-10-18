//! Event row buffers (§4.2): one per stream, holding the stream's journal records. A buffer
//! becomes L0 chunks at `event_buffer_rows` rows or `event_buffer_bytes` journal bytes, or
//! once its oldest row has waited `event_buffer_age` (`write::chunking`).
//!
//! A buffer keeps each record as its stored bytes, a [segment](Segment), behind its own
//! `Arc`: the bytes are the most compact form of the rows, they were validated when they
//! were staged or read back, and a new version shares every segment of the last one.

use std::sync::Arc;

use super::{ARC_HEADER, journal::EventRecord};
use crate::{
    Timestamp,
    budget::{BudgetVec, Reserved, Retained},
};

/// One journal record of a stream: the rows one batch wrote to one extent.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Segment {
    /// The record's journal sequence. A buffer keeps its segments in this order, the order
    /// the batches wrote them.
    seq: u64,
    /// An encoded, validated [`EventRecord`].
    record: Box<[u8]>,
    rows: u64,
    /// The `now` of the batch that wrote it.
    written: Timestamp,
    /// The record's bytes in storage, key included.
    journal_bytes: u64,
    extent: i64,
}

impl Segment {
    pub(crate) fn new(
        seq: u64,
        record: Box<[u8]>,
        rows: u64,
        written: Timestamp,
        journal_bytes: u64,
        extent: i64,
    ) -> Self {
        Self {
            seq,
            record,
            rows,
            written,
            journal_bytes,
            extent,
        }
    }

    fn bytes(&self) -> u64 {
        Self::bytes_for(self.record.len())
    }

    /// The allocations of a segment of a record of `len` bytes: its `Arc`, its struct and the
    /// record.
    pub(crate) fn bytes_for(len: usize) -> u64 {
        (ARC_HEADER + size_of::<Segment>() + len) as u64
    }

    /// The record, whose rows are decoded only as they are read.
    pub(crate) fn record(&self) -> EventRecord<'_> {
        EventRecord::open(&self.record).expect("a segment holds a validated record")
    }

    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }

    pub(crate) fn rows(&self) -> u64 {
        self.rows
    }

    /// The length of its record.
    pub(crate) fn record_len(&self) -> usize {
        self.record.len()
    }

    /// The `now` of the batch that wrote it.
    pub(crate) fn written(&self) -> Timestamp {
        self.written
    }

    /// The record's bytes in storage, key included.
    pub(crate) fn journal_bytes(&self) -> u64 {
        self.journal_bytes
    }

    pub(crate) fn extent(&self) -> i64 {
        self.extent
    }
}

/// The journal records of one stream, oldest first.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EventBuffer {
    segments: Box<[Arc<Segment>]>,
    rows: u64,
    journal_bytes: u64,
    /// The `now` of the batch that wrote the oldest row, which has waited longest.
    oldest: Option<Timestamp>,
    /// Every allocation the buffer reaches, shared with other versions or not.
    bytes: u64,
}

impl EventBuffer {
    pub(crate) fn empty() -> Self {
        Self::from_segments(Box::default())
    }

    /// The buffer with `segments` appended, sharing every segment it had. Its list is
    /// allocated once, at its length.
    pub(crate) fn appended(
        &self,
        segments: BudgetVec<Reserved<Segment>>,
        retained: &mut Retained<'_>,
    ) -> Self {
        let all = self
            .segments
            .iter()
            .cloned()
            .chain(
                segments
                    .into_iter()
                    .map(|segment| Arc::new(retained.absorb(segment))),
            )
            .collect();
        Self::from_segments(all)
    }

    /// What [`appended`](Self::appended) allocates: the new buffer's own allocations and each
    /// new segment's `Arc`, not their records, which move into it.
    pub(crate) fn appended_bytes(&self, segments: usize) -> u64 {
        Self::own_bytes(self.segments.len() + segments) + segments as u64 * Segment::bytes_for(0)
    }

    /// The buffer without the records of `extent`, which retention expired (§9.1), sharing
    /// every other segment. Its list is allocated once, at its length:
    /// [`without_bytes`](Self::without_bytes) is what it allocates.
    pub(crate) fn without(&self, extent: i64) -> Self {
        let kept = self.kept(extent).count();
        let mut segments = Vec::with_capacity(kept);
        segments.extend(self.kept(extent).cloned());
        Self::from_segments(segments.into_boxed_slice())
    }

    pub(crate) fn without_bytes(&self, extent: i64) -> u64 {
        Self::own_bytes(self.kept(extent).count())
    }

    fn kept(&self, extent: i64) -> impl Iterator<Item = &Arc<Segment>> {
        self.segments
            .iter()
            .filter(move |segment| segment.extent != extent)
    }

    /// The segments of `extent`, in journal sequence order.
    pub(crate) fn of_extent(&self, extent: i64) -> impl Iterator<Item = &Segment> {
        self.segments
            .iter()
            .filter(move |segment| segment.extent == extent)
            .map(|segment| &**segment)
    }

    /// A buffer of `segments`, which are in journal sequence order.
    pub(crate) fn from_arcs(segments: Vec<Arc<Segment>>) -> Self {
        debug_assert!(segments.is_sorted_by_key(|segment| segment.seq));
        Self::from_segments(segments.into_boxed_slice())
    }

    fn from_segments(segments: Box<[Arc<Segment>]>) -> Self {
        Self {
            rows: segments.iter().map(|segment| segment.rows).sum(),
            journal_bytes: segments.iter().map(|segment| segment.journal_bytes).sum(),
            oldest: segments.first().map(|segment| segment.written),
            bytes: Self::own_bytes(segments.len())
                + segments.iter().map(|segment| segment.bytes()).sum::<u64>(),
            segments,
        }
    }

    /// The buffer's own allocations: its `Arc`, its struct and its list of segments.
    pub(crate) fn own_bytes(segments: usize) -> u64 {
        (ARC_HEADER + size_of::<EventBuffer>() + segments * size_of::<Arc<Segment>>()) as u64
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The bytes of this buffer that `base` does not share: its own allocations unless it is
    /// `base` itself, and the segments appended to `base`'s.
    #[cfg(test)]
    pub(crate) fn copied_from(&self, base: &EventBuffer) -> u64 {
        if std::ptr::eq(self, base) {
            return 0;
        }
        let copied: u64 = self
            .segments
            .iter()
            .enumerate()
            .filter(|&(index, segment)| {
                base.segments
                    .get(index)
                    .is_none_or(|shared| !Arc::ptr_eq(segment, shared))
            })
            .map(|(_, segment)| segment.bytes())
            .sum();
        Self::own_bytes(self.segments.len()) + copied
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub(crate) fn segments(&self) -> &[Arc<Segment>] {
        &self.segments
    }

    pub(crate) fn rows(&self) -> u64 {
        self.rows
    }

    /// The bytes of the stream's journal records, keys included.
    pub(crate) fn journal_bytes(&self) -> u64 {
        self.journal_bytes
    }

    pub(crate) fn oldest(&self) -> Option<Timestamp> {
        self.oldest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(written: i64) -> Segment {
        Segment::new(
            0,
            vec![0; 10].into_boxed_slice(),
            3,
            Timestamp(written),
            20,
            0,
        )
    }

    #[test]
    fn appending_shares_the_segments_and_keeps_the_oldest() {
        let empty = EventBuffer::empty();
        assert_eq!(
            (empty.rows(), empty.journal_bytes(), empty.oldest()),
            (0, 0, None)
        );
        let account = crate::budget::test_account();
        let mut retained = Retained::new(&account);
        let one = empty.appended(
            crate::budget::test_buffer([crate::budget::test_value(segment(100))]),
            &mut retained,
        );
        let two = one.appended(
            crate::budget::test_buffer([crate::budget::test_value(segment(50))]),
            &mut retained,
        );
        assert_eq!(
            (two.rows(), two.journal_bytes(), two.oldest()),
            (6, 40, Some(Timestamp(100)))
        );
        assert!(Arc::ptr_eq(&one.segments[0], &two.segments[0]));
        assert_eq!(*two.segments()[1], segment(50));

        let arc = ARC_HEADER as u64;
        let segment_bytes = arc + size_of::<Segment>() as u64 + 10;
        let own = arc + size_of::<EventBuffer>() as u64 + 2 * size_of::<Arc<Segment>>() as u64;
        assert_eq!(two.bytes(), own + 2 * segment_bytes);
        // Appending copies the buffer's own allocations and the new segment, nothing else.
        assert_eq!(two.copied_from(&one), own + segment_bytes);
        assert_eq!(two.copied_from(&two), 0);
    }
}
