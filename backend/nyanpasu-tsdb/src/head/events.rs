//! Event row buffers (§4.2): one per stream, holding the stream's journal records. A buffer
//! becomes an L0 chunk at 4,096 rows, at 64 KB, or once its oldest row has waited T; until
//! L0 chunking, it only grows.
//!
//! A buffer keeps each record as its stored bytes, a [segment](Segment), behind its own
//! `Arc`: the bytes are the most compact form of the rows, they were validated when they
//! were staged or read back, and a new version shares every segment of the last one.

use std::sync::Arc;

use super::ARC_HEADER;
use crate::Timestamp;
#[cfg(test)]
use crate::{head::journal::EventRecord, layout::record::decode};

/// One journal record of a stream: the rows one batch wrote.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Segment {
    /// An encoded, validated [`EventRecord`](super::journal::EventRecord).
    record: Box<[u8]>,
    rows: u64,
    /// The `now` of the batch that wrote it.
    written: Timestamp,
    /// The record's bytes in storage, key included.
    journal_bytes: u64,
}

impl Segment {
    pub(crate) fn new(
        record: Box<[u8]>,
        rows: u64,
        written: Timestamp,
        journal_bytes: u64,
    ) -> Self {
        Self {
            record,
            rows,
            written,
            journal_bytes,
        }
    }

    fn bytes(&self) -> u64 {
        (ARC_HEADER + size_of::<Segment>() + self.record.len()) as u64
    }

    #[cfg(test)]
    pub(crate) fn record(&self) -> EventRecord<'_> {
        decode(&self.record).expect("a segment holds a validated record")
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

    /// The buffer with `segments` appended, sharing every segment it had.
    pub(crate) fn appended(&self, segments: Vec<Segment>) -> Self {
        let all = self
            .segments
            .iter()
            .cloned()
            .chain(segments.into_iter().map(Arc::new))
            .collect();
        Self::from_segments(all)
    }

    fn from_segments(segments: Box<[Arc<Segment>]>) -> Self {
        let own =
            ARC_HEADER + size_of::<EventBuffer>() + segments.len() * size_of::<Arc<Segment>>();
        Self {
            rows: segments.iter().map(|segment| segment.rows).sum(),
            journal_bytes: segments.iter().map(|segment| segment.journal_bytes).sum(),
            oldest: segments.first().map(|segment| segment.written),
            bytes: own as u64 + segments.iter().map(|segment| segment.bytes()).sum::<u64>(),
            segments,
        }
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }

    #[cfg(test)]
    pub(crate) fn segments(&self) -> &[Arc<Segment>] {
        &self.segments
    }

    #[cfg(test)]
    pub(crate) fn stats(&self) -> (u64, u64, Option<Timestamp>) {
        (self.rows, self.journal_bytes, self.oldest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(written: i64) -> Segment {
        Segment::new(vec![0; 10].into_boxed_slice(), 3, Timestamp(written), 20)
    }

    #[test]
    fn appending_shares_the_segments_and_keeps_the_oldest() {
        let empty = EventBuffer::empty();
        assert_eq!(empty.stats(), (0, 0, None));
        let one = empty.appended(vec![segment(100)]);
        let two = one.appended(vec![segment(50)]);
        assert_eq!(two.stats(), (6, 40, Some(Timestamp(100))));
        assert!(Arc::ptr_eq(&one.segments[0], &two.segments[0]));
        assert_eq!(*two.segments()[1], segment(50));

        let arc = ARC_HEADER as u64;
        let segment_bytes = arc + size_of::<Segment>() as u64 + 10;
        let own = arc + size_of::<EventBuffer>() as u64 + 2 * size_of::<Arc<Segment>>() as u64;
        assert_eq!(two.bytes(), own + 2 * segment_bytes);
    }
}
