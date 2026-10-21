//! The chunk reader. [`ChunkReader::open`] reads fragment 0 alone and validates all of it,
//! the length every other fragment must have included; after that, reading a block fetches
//! only the fragment that holds it, verifies that fragment's checksum and length, and
//! decodes the block into buffers the caller owns and reuses.
//!
//! Queries account for their memory (§7.4, §8.2), so the reader allocates nothing they have
//! not reserved:
//!
//! - the fragment the backend lends while the reader parses fragment 0 or decodes a block,
//!   which the caller reserves before it opens the chunk ([`lent_memory`]), from fragment 0's
//!   length as the chunk's manifest entry records it;
//! - an open reader holds a reservation of its directory's bytes, taken before fragment 0 is
//!   fetched and returned when the reader is dropped;
//! - a block's [`decode_memory`] describes each buffer's need; the decoder admits replacements
//!   beside retained capacities before fetching the block;
//! - a large value takes its length, [`LargeEntry::len`], in the caller's charged buffer.

use snafu::{OptionExt, ResultExt, ensure};

use super::{
    FragmentSource,
    error::{
        ChecksumSnafu, ColumnTypeSnafu, ContentLengthSnafu, DecodeBlockSnafu, Fragment0LengthSnafu,
        MissingFragmentSnafu, NoSuchBlockSnafu, NoSuchColumnSnafu, NoSuchLargeValueSnafu,
        NoSuchPieceSnafu, ReadError, ReadFragmentSnafu, RecordedFragment0Snafu,
        ReserveDirectorySnafu,
    },
    format::{
        BlockEntry, ChunkKind, ColumnDesc, Directory, FRAGMENT0_LENS, LargeEntry, MAX_CHUNK_LEN,
        MAX_DATA_FRAGMENT_LEN, MAX_FRAGMENTS, MAX_LARGE_VALUES, MAX_ROWS, PhysicalType,
    },
};
use crate::{
    budget::{BudgetVec, MemoryAccount, MemoryPlan},
    codec::{
        block::{self, BlockShape, DecodeMemory, IntView, PreparedBytes, PreparedInts},
        bytes::BytesHint,
        checksum::{self, TRAILER_LEN},
        int::IntValue,
    },
};

/// The most bytes the backend lends a reader of a chunk at once (§7.4), from fragment 0's
/// length as the chunk's manifest entry records it, `fragment0`: fragment 0 while the reader
/// parses it, and then one data fragment at a time, at most [`MAX_DATA_FRAGMENT_LEN`], while a
/// block decodes or a large value is copied. The caller reserves it before it opens the chunk,
/// once [`check_recorded`] accepted `fragment0`.
pub(crate) fn lent_memory(fragment0: u32) -> u64 {
    u64::from(fragment0).max(MAX_DATA_FRAGMENT_LEN as u64)
}

/// Checks that `fragment0`, as a manifest entry records it, is a length fragment 0 can have. A
/// reader checks it before it sizes anything by it, so that a damaged entry is reported as
/// damage, not as an exhausted budget.
pub(crate) fn check_recorded(fragment0: u32) -> Result<(), ReadError> {
    ensure!(
        FRAGMENT0_LENS.contains(&(fragment0 as usize)),
        RecordedFragment0Snafu {
            recorded: fragment0
        }
    );
    Ok(())
}

/// Whether recorded chunk metadata is within the format limits, without reading payloads or
/// allocating. Callers check schema-specific row limits and placement separately, before
/// sizing a read or deleting a chunk. This does not prove the fragments match the metadata.
pub(crate) fn metadata_within_limits(
    kind: u8,
    rows: u32,
    key_range: (i64, i64),
    bytes: u64,
    fragment0: u32,
    fragments: u32,
) -> bool {
    ChunkKind::from_repr(kind).is_some()
        && (1..=MAX_ROWS).contains(&(rows as usize))
        && key_range.0 <= key_range.1
        && bytes <= MAX_CHUNK_LEN
        && (2..=MAX_FRAGMENTS).contains(&(fragments as usize))
        && check_recorded(fragment0).is_ok()
        && u64::from(fragment0) < bytes
}

/// What reading a block of `shape` of a column of `column` needs in the decoded block and the
/// decoder's scratch (see [`DecodeMemory`]); a text block is taken to refer to as many
/// large values as a chunk can hold, which makes its output up to 8 bytes a row more than a
/// chunk with fewer needs. Its shape is in the chunk directory, [`ChunkReader::blocks`].
///
/// Each buffer retains its largest need so far, with its lease, up to the per-column format
/// bounds in [`block`]'s Memory section. Covered buffers are cleared; all undersized storage
/// drops before any replacement allocates exactly its new need. Admission covers each buffer's
/// larger retained/required capacity, not the maximum of aggregate block sizes. Buffers never
/// shrink after a read; the owner may explicitly release them between phases (§7.4, §8.2).
/// For `R = MAX_BLOCK_ROWS`, `B = MAX_BLOCK_LEN`, integer output is at most
/// `R*width + ceil(R/8)`, bytes output `4*(R+1) + B + ceil(R/8)` plus at most 2,048 text-reference
/// bytes. Scratch is bounded by `B` for integers and `B + 8*R` across all bytes encodings.
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
    /// The caller's envelope covering loans and data buffers.
    account: MemoryAccount,
}

impl ChunkReader {
    /// Opens the chunk whose fragments `source` reads, whose fragment 0 is `fragment0` bytes
    /// long, trailer included, as its manifest entry records. Before fetching fragment 0, it
    /// reserves through `account` what parsing a fragment 0 of that length can hold
    /// ([`open_memory`](super::format::open_memory)); the fetch lends no more than that length.
    /// A fragment 0 of another length is damage, found before it is parsed. Once parsed, the
    /// reader keeps only the directory's [`memory`](Self::memory), until it is dropped.
    pub(crate) fn open(
        source: &impl FragmentSource,
        account: &MemoryAccount,
        fragment0: u32,
    ) -> Result<Self, ReadError> {
        check_recorded(fragment0)?;
        let directory = account
            .value(
                "chunk directory",
                MemoryPlan::bytes(super::format::open_memory(fragment0 as usize - TRAILER_LEN)),
                || {
                    with_fragment(source, account, 0, fragment0 as usize, |content| {
                        let actual = content.len() + TRAILER_LEN;
                        ensure!(
                            actual == fragment0 as usize,
                            Fragment0LengthSnafu {
                                recorded: fragment0,
                                actual
                            }
                        );
                        super::format::parse_directory(content, |_| {})
                    })
                },
            )
            .context(ReserveDirectorySnafu)?
            .transpose()?;
        Ok(Self {
            directory: Directory::from_payload(directory),
            account: account.clone(),
        })
    }

    /// The operation envelope used for block and large-value allocations and fragment loans.
    pub(crate) fn account(&self) -> &MemoryAccount {
        &self.account
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

    /// Decodes block `block` into its previously admitted integer output. The column must be of
    /// `T`'s type; a chunk whose column differs does not match the caller's schema.
    pub(crate) fn read_prepared_ints<T: IntValue>(
        &self,
        source: &impl FragmentSource,
        column: u16,
        block: u32,
        prepared: PreparedInts<'_, T>,
    ) -> Result<(), ReadError> {
        let entry = self.int_block::<T>(column, block)?;
        self.with_block(source, entry, |stored| {
            prepared
                .decode(stored, entry.shape())
                .context(DecodeBlockSnafu { column, block })
        })
    }

    /// Decodes block `block` into its previously admitted bytes output.
    pub(crate) fn read_prepared_bytes(
        &self,
        source: &impl FragmentSource,
        column: u16,
        block: u32,
        prepared: PreparedBytes<'_>,
    ) -> Result<(), ReadError> {
        let (entry, _) = self.bytes_block(column, block)?;
        self.with_block(source, entry, |stored| {
            prepared
                .decode(stored, entry.shape())
                .context(DecodeBlockSnafu { column, block })
        })
    }

    /// Hands `f` the values of a raw, uncompressed integer block in place, valid while the
    /// fragment is borrowed. `None` when the block is stored otherwise; then
    /// [`read_prepared_ints`](Self::read_prepared_ints) decodes it.
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
        out: &mut BudgetVec<u8>,
    ) -> Result<(), ReadError> {
        let entry = self
            .directory
            .large
            .get(index as usize)
            .context(NoSuchLargeValueSnafu { index })?;
        // Validated against the largest large value when the chunk was opened.
        let len = entry.len as usize;
        if out.capacity() < len {
            out.reset_to(len).context(ReserveDirectorySnafu)?;
        }
        out.clear();
        for fragment in entry.first_fragment..entry.first_fragment + entry.fragments {
            self.with_data(source, fragment, |content| {
                out.extend_from_slice(content);
                Ok(())
            })?;
        }
        Ok(())
    }

    /// Hands `f` piece `piece` of large value `index`, the content of its fragment, valid
    /// while the fragment is borrowed: [`DATA_PAYLOAD_LEN`] bytes for every piece but the last,
    /// the pieces the builder writes ([`LargeValue::put`]). A copy of a large value thus holds
    /// one fragment of it at a time, the one the backend lends, never all of it (§6.5, §7.4).
    ///
    /// [`DATA_PAYLOAD_LEN`]: super::format::DATA_PAYLOAD_LEN
    /// [`LargeValue::put`]: super::builder::LargeValue::put
    pub(crate) fn read_large_piece<R>(
        &self,
        source: &impl FragmentSource,
        index: u32,
        piece: u32,
        f: impl FnOnce(&[u8]) -> R,
    ) -> Result<R, ReadError> {
        let entry = self
            .directory
            .large
            .get(index as usize)
            .context(NoSuchLargeValueSnafu { index })?;
        ensure!(piece < entry.fragments, NoSuchPieceSnafu { index, piece });
        // The opening checked that the value's fragments are its own and of its length.
        self.with_data(source, entry.first_fragment + piece, |content| {
            Ok(f(content))
        })
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

    pub(crate) fn large_count(&self) -> u32 {
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

    /// Fetches data fragment `index`, no longer than the directory gives it, and checks it
    /// like [`with_fragment`], and its length against the directory.
    fn with_data<R>(
        &self,
        source: &impl FragmentSource,
        index: u32,
        f: impl FnOnce(&[u8]) -> Result<R, ReadError>,
    ) -> Result<R, ReadError> {
        let expected = self.directory.fragment_lens[index as usize] as usize;
        with_fragment(
            source,
            &self.account,
            index,
            expected + TRAILER_LEN,
            |content| {
                ensure!(
                    content.len() == expected,
                    ContentLengthSnafu {
                        index,
                        expected,
                        actual: content.len()
                    }
                );
                f(content)
            },
        )
    }
}

/// Fetches fragment `index`, which the source lends only when it is at most `limit` bytes
/// long, checks its checksum, and hands `f` its content.
fn with_fragment<R>(
    source: &impl FragmentSource,
    account: &MemoryAccount,
    index: u32,
    limit: usize,
    f: impl FnOnce(&[u8]) -> Result<R, ReadError>,
) -> Result<R, ReadError> {
    account
        .lend(limit as u64, || {
            source
                .get(index, limit, |bytes| {
                    let bytes = bytes.context(MissingFragmentSnafu { index })?;
                    let content = checksum::verify(bytes).context(ChecksumSnafu { index })?;
                    f(content)
                })
                .context(ReadFragmentSnafu { index })?
        })
        .context(ReserveDirectorySnafu)?
}

#[cfg(test)]
impl ChunkReader {
    /// Test convenience that admits integer storage through the same production context.
    pub(crate) fn read_ints<T: IntValue>(
        &self,
        source: &impl FragmentSource,
        column: u16,
        block: u32,
        decoder: &mut crate::codec::block::BlockDecoder,
        out: &mut crate::codec::block::IntBlock<T>,
    ) -> Result<(), ReadError> {
        let shape = self.int_block::<T>(column, block)?.shape();
        let prepared = decoder
            .prepare_ints(shape, out)
            .map_err(|source| ReadError::ReserveDirectory { source })?;
        self.read_prepared_ints(source, column, block, prepared)
    }

    /// Test convenience that admits bytes storage through the production context.
    pub(crate) fn read_bytes(
        &self,
        source: &impl FragmentSource,
        column: u16,
        block: u32,
        decoder: &mut crate::codec::block::BlockDecoder,
        out: &mut crate::codec::block::BytesBlock,
    ) -> Result<(), ReadError> {
        let (entry, hint) = self.bytes_block(column, block)?;
        let prepared = decoder
            .prepare_bytes(entry.shape(), self.large_count(), hint, out)
            .map_err(|source| ReadError::ReserveDirectory { source })?;
        self.read_prepared_bytes(source, column, block, prepared)
    }
}
