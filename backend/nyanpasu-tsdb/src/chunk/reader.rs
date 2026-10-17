//! The chunk reader. [`ChunkReader::open`] reads fragment 0 alone and validates all of it,
//! the length every other fragment must have included; after that, reading a block fetches
//! only the fragment that holds it, verifies that fragment's checksum and length, and
//! decodes the block into buffers the caller owns and reuses.
//!
//! Queries account for their memory (§7.4, §8.2), so the reader allocates nothing they have
//! not reserved:
//!
//! - an open reader holds a reservation of its directory's bytes, taken before fragment 0 is
//!   decoded and returned when the reader is dropped;
//! - a block's [`decode_memory`] is what reading it leaves the decoder and the decoded block
//!   holding, which the caller reserves as it says;
//! - a large value takes its length, [`LargeEntry::len`], in the caller's `Vec`.

use snafu::{OptionExt, ResultExt, ensure};

use super::{
    FragmentSource,
    error::{
        ChecksumSnafu, ColumnTypeSnafu, ContentLengthSnafu, DecodeBlockSnafu, FragmentLengthSnafu,
        MissingFragmentSnafu, NoSuchBlockSnafu, NoSuchColumnSnafu, NoSuchLargeValueSnafu,
        ReadError, ReadFragmentSnafu, ReserveDirectorySnafu,
    },
    format::{
        BlockEntry, ChunkKind, ColumnDesc, Directory, LargeEntry, MAX_LARGE_VALUES, PhysicalType,
        open_memory,
    },
};
use crate::{
    budget::{MemoryBudget, Reservation},
    codec::{
        block::{self, BlockDecoder, BlockShape, BytesBlock, DecodeMemory, IntBlock, IntView},
        bytes::BytesHint,
        checksum,
        int::IntValue,
    },
    storage::MAX_FRAGMENT_LEN,
};

/// What reading a block of `shape` of a column of `column` leaves the decoded block and the
/// decoder's scratch holding (see [`DecodeMemory`]); a text block is taken to refer to as many
/// large values as a chunk can hold, which makes its output up to 8 bytes a row more than a
/// chunk with fewer needs. Its shape is in the chunk directory, [`ChunkReader::blocks`].
///
/// A read sizes the buffers to exactly the block, and lets every one go of what it does not
/// keep before any grows, so it holds at most the larger of what the block before it and this
/// one declare. A query keeps, for each decoded block, a reservation of the output it holds,
/// and for each decoder one of its scratch; each starts at 0. Before a read, it grows each to
/// the larger of what it holds and this block's part, then reads; after the read, it shrinks
/// each to this block's part (§7.4, §8.2).
pub(crate) fn decode_memory(shape: BlockShape, column: PhysicalType) -> DecodeMemory {
    match column {
        PhysicalType::Int { ty, .. } => DecodeMemory::ints(shape, ty),
        PhysicalType::Bytes(hint) => DecodeMemory::bytes(shape, MAX_LARGE_VALUES as u32, hint),
    }
}

/// An open chunk: its validated directory. Fragments are read through a
/// [`FragmentSource`] passed to each call, so the reader holds no storage borrow.
#[derive(Debug)]
pub(crate) struct ChunkReader {
    directory: Directory,
    /// The directory's bytes, held for as long as the reader is open.
    #[allow(
        dead_code,
        reason = "held for its Drop, which returns the directory's bytes"
    )]
    reservation: Reservation,
}

impl ChunkReader {
    /// Opens the chunk whose fragments `source` reads. Before decoding fragment 0, it reserves
    /// from `budget` what parsing a fragment 0 of that length can hold ([`open_memory`]); once
    /// parsed, it keeps only the directory's [`memory`](Self::memory), until it is dropped.
    pub(crate) fn open(
        source: &impl FragmentSource,
        budget: &MemoryBudget,
    ) -> Result<Self, ReadError> {
        with_fragment(source, 0, |content| {
            let bound = open_memory(content.len());
            let mut reservation = budget
                .try_reserve("chunk directory", bound)
                .context(ReserveDirectorySnafu)?;
            let directory = Directory::parse(content)?;
            // open_memory bounds what parsing holds, and so what the directory keeps.
            reservation.shrink(bound - directory.memory());
            Ok(Self {
                directory,
                reservation,
            })
        })
    }

    /// The bytes the reader holds: its directory's heap, exactly.
    pub(crate) fn memory(&self) -> u64 {
        self.directory.memory()
    }

    pub(crate) fn kind(&self) -> ChunkKind {
        self.directory.header.kind
    }

    pub(crate) fn schema_fingerprint(&self) -> u64 {
        self.directory.header.fingerprint
    }

    pub(crate) fn rows(&self) -> u32 {
        self.directory.header.rows
    }

    /// The range of the key column: buckets for a series chunk, keys for an event chunk.
    pub(crate) fn key_range(&self) -> (i64, i64) {
        let header = &self.directory.header;
        (header.key_min, header.key_max)
    }

    pub(crate) fn fragments(&self) -> u32 {
        self.directory.header.fragments
    }

    /// The columns, with their chunk stats.
    pub(crate) fn columns(&self) -> &[ColumnDesc] {
        &self.directory.columns
    }

    /// A column's blocks in row order, with their stats.
    pub(crate) fn blocks(&self, column: u16) -> Result<&[BlockEntry], ReadError> {
        let desc = self.column(column)?;
        let first = desc.first_block as usize;
        Ok(&self.directory.blocks[first..first + desc.blocks as usize])
    }

    pub(crate) fn large_values(&self) -> &[LargeEntry] {
        &self.directory.large
    }

    /// Decodes block `block` of integer column `column` into `out`. The column must be of
    /// `T`'s type; a chunk whose column differs does not match the caller's schema.
    pub(crate) fn read_ints<T: IntValue>(
        &self,
        source: &impl FragmentSource,
        column: u16,
        block: u32,
        decoder: &mut BlockDecoder,
        out: &mut IntBlock<T>,
    ) -> Result<(), ReadError> {
        let entry = self.int_block::<T>(column, block)?;
        self.with_block(source, entry, |stored| {
            decoder
                .decode_ints(stored, entry.shape(), out)
                .context(DecodeBlockSnafu { column, block })
        })
    }

    /// Decodes block `block` of bytes column `column` into `out`.
    pub(crate) fn read_bytes(
        &self,
        source: &impl FragmentSource,
        column: u16,
        block: u32,
        decoder: &mut BlockDecoder,
        out: &mut BytesBlock,
    ) -> Result<(), ReadError> {
        let (entry, hint) = self.bytes_block(column, block)?;
        let large_count = self.large_count();
        self.with_block(source, entry, |stored| {
            decoder
                .decode_bytes(stored, entry.shape(), large_count, hint, out)
                .context(DecodeBlockSnafu { column, block })
        })
    }

    /// Hands `f` the values of a raw, uncompressed integer block in place, valid while the
    /// fragment is borrowed. `None` when the block is stored otherwise; then
    /// [`read_ints`](Self::read_ints) decodes it.
    pub(crate) fn view_ints<T: IntValue, R>(
        &self,
        source: &impl FragmentSource,
        column: u16,
        block: u32,
        f: impl FnOnce(IntView<'_, T>) -> R,
    ) -> Result<Option<R>, ReadError> {
        let entry = self.int_block::<T>(column, block)?;
        self.with_block(source, entry, |stored| {
            let view = block::view_ints::<T>(stored, entry.shape())
                .context(DecodeBlockSnafu { column, block })?;
            Ok(view.map(f))
        })
    }

    /// Reads large value `index` into `out`, replacing its contents. When `out` has room for
    /// less than the value's length, the `len` of its entry in
    /// [`large_values`](Self::large_values), it lets go of its allocation and then gets room for
    /// exactly that, so that the two never coexist; the caller reserves the length first
    /// (§7.4). Otherwise it keeps its room.
    pub(crate) fn read_large(
        &self,
        source: &impl FragmentSource,
        index: u32,
        out: &mut Vec<u8>,
    ) -> Result<(), ReadError> {
        let entry = self
            .directory
            .large
            .get(index as usize)
            .context(NoSuchLargeValueSnafu { index })?;
        // Validated against the largest large value when the chunk was opened.
        let len = entry.len as usize;
        if out.capacity() < len {
            *out = Vec::new();
        }
        out.clear();
        out.reserve_exact(len);
        for fragment in entry.first_fragment..entry.first_fragment + entry.fragments {
            self.with_data(source, fragment, |content| {
                out.extend_from_slice(content);
                Ok(())
            })?;
        }
        Ok(())
    }

    fn column(&self, column: u16) -> Result<&ColumnDesc, ReadError> {
        self.directory
            .columns
            .get(usize::from(column))
            .context(NoSuchColumnSnafu { column })
    }

    fn block(&self, column: u16, block: u32) -> Result<&BlockEntry, ReadError> {
        self.blocks(column)?
            .get(block as usize)
            .context(NoSuchBlockSnafu { column, block })
    }

    fn bytes_block(&self, column: u16, block: u32) -> Result<(&BlockEntry, BytesHint), ReadError> {
        let entry = self.block(column, block)?;
        let actual = self.directory.columns[usize::from(column)].ty;
        let PhysicalType::Bytes(hint) = actual else {
            return ColumnTypeSnafu { column, actual }.fail();
        };
        Ok((entry, hint))
    }

    fn large_count(&self) -> u32 {
        self.directory.large.len() as u32
    }

    fn int_block<T: IntValue>(&self, column: u16, block: u32) -> Result<&BlockEntry, ReadError> {
        let entry = self.block(column, block)?;
        let actual = self.directory.columns[usize::from(column)].ty;
        ensure!(
            matches!(actual, PhysicalType::Int { ty, .. } if ty == T::TYPE),
            ColumnTypeSnafu { column, actual }
        );
        Ok(entry)
    }

    /// Hands `f` the stored bytes of `entry`, within its verified fragment.
    fn with_block<R>(
        &self,
        source: &impl FragmentSource,
        entry: &BlockEntry,
        f: impl FnOnce(&[u8]) -> Result<R, ReadError>,
    ) -> Result<R, ReadError> {
        self.with_data(source, entry.fragment, |content| {
            // The fragment has the length the directory gives it, which the opening checked
            // reaches past every block in it.
            let start = entry.offset as usize;
            f(&content[start..start + entry.stored_len as usize])
        })
    }

    /// Fetches data fragment `index` and checks it like [`with_fragment`], and its length
    /// against the directory.
    fn with_data<R>(
        &self,
        source: &impl FragmentSource,
        index: u32,
        f: impl FnOnce(&[u8]) -> Result<R, ReadError>,
    ) -> Result<R, ReadError> {
        let expected = self.directory.fragment_lens[index as usize] as usize;
        with_fragment(source, index, |content| {
            ensure!(
                content.len() == expected,
                ContentLengthSnafu {
                    index,
                    expected,
                    actual: content.len()
                }
            );
            f(content)
        })
    }
}

/// Fetches fragment `index`, checks its length and checksum, and hands `f` its content.
fn with_fragment<R>(
    source: &impl FragmentSource,
    index: u32,
    f: impl FnOnce(&[u8]) -> Result<R, ReadError>,
) -> Result<R, ReadError> {
    source
        .get(index, |bytes| {
            let bytes = bytes.context(MissingFragmentSnafu { index })?;
            ensure!(
                bytes.len() <= MAX_FRAGMENT_LEN,
                FragmentLengthSnafu {
                    index,
                    len: bytes.len()
                }
            );
            let content = checksum::verify(bytes).context(ChecksumSnafu { index })?;
            f(content)
        })
        .context(ReadFragmentSnafu { index })?
}
