//! The byte layout of a chunk (§3.3, §6.1), version [`FORMAT_VERSION`].
//!
//! A chunk is a run of fragments, each ending with a CRC32 trailer: fragment 0 is at most
//! [`MAX_FRAGMENT_LEN`] bytes, and every data fragment at most [`MAX_DATA_FRAGMENT_LEN`].
//!
//! - **Fragment 0**, written last, is the [`MAGIC`] and the [`FORMAT_VERSION`] (`u16`,
//!   little-endian), then the [postcard] encoding of a [`DirectoryRecord`]: the header, the
//!   column records (with each column's chunk stats), the column directory and the
//!   large-value directory. postcard 1.x has a stable, specified wire format
//!   (<https://postcard.jamesmunns.com/wire-format>), so the derived `Serialize` and
//!   `Deserialize` of the records are the layout: the declaration order of their fields, and
//!   of the variants of [`IntRange`], is part of the format. A stored tag is a `u8` written
//!   with `as u8` and read back with `from_repr`.
//! - **Data fragments** pack column blocks (see [`codec::block`](crate::codec::block)) at
//!   8-byte-aligned offsets. A block never crosses a fragment, and blocks of different columns
//!   may share one.
//! - **Large-value fragments**: each large value takes consecutive fragments of its own,
//!   every one full but the last.
//!
//! The blocks of a column are consecutive in the directory and cover its rows `[0, rows)`
//! in order; column 0 is the key whose range the header records.
//!
//! [`parse_directory`] checks every count, length, offset and index against the limits
//! below, and the decoder of fragment 0 stops a list at its limit, before anything sized by
//! them is allocated (§6.5). It also gives every fragment one owner and bounds the whole
//! chunk by [`MAX_CHUNK_LEN`], so a chunk that opens cannot make its reads exceed the limits.
//!
//! [postcard]: https://docs.rs/postcard

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, ensure};
use strum::FromRepr;

use super::error::{
    BlockProblem, ColumnProblem, DecodeDirectorySnafu, HeaderProblem, InvalidBlockEntrySnafu,
    InvalidColumnSnafu, InvalidHeaderSnafu, InvalidLargeEntrySnafu, LargeProblem, ReadError,
};
use crate::{
    codec::{
        Codec, Encoding, MAX_BLOCK_LEN, MAX_BLOCK_ROWS, MAX_INLINE_VALUE_LEN,
        block::{BlockShape, MAX_BLOCK_OVERHEAD},
        bytes::{BytesHint, BytesHintKind},
        checksum::TRAILER_LEN,
        int::IntType,
    },
    layout::record::bounded,
    storage::MAX_FRAGMENT_LEN,
};

/// "Nyanpasu TSdb Chunk".
pub(crate) const MAGIC: [u8; 4] = *b"NTSC";

/// Bumped whenever the layout changes; a reader rejects versions it does not know (§0.3
/// rule 6). The limits below belong to it too.
pub(crate) const FORMAT_VERSION: u16 = 1;

/// The magic and the version, before the postcard body of fragment 0.
pub(crate) const PREFIX_LEN: usize = MAGIC.len() + 2;

/// M, the rows of one chunk (§4.3). It bounds the directory and every decoded column.
pub(crate) const MAX_ROWS: usize = 65_536;

/// B, the bytes of one chunk, all fragments together (§4.3). It bounds what one
/// consolidation round rewrites and what one chunk adds to a transaction.
pub(crate) const MAX_CHUNK_LEN: u64 = 4 * 1024 * 1024;

/// The raw size the builder fills a block to. §6.3 rule 2 asks for 16–64 KiB: zstd's ratio
/// drops quickly below 16 KiB, and larger blocks cost random access. Since a single inline
/// value takes at most [`MAX_VALUE_RAW_LEN`], every block but a column's last holds at
/// least [`MIN_BLOCK_FILL`]. It is 32 bytes short of 32 KiB so that two full blocks, each
/// stored in at most [`MAX_BLOCK_OVERHEAD`] bytes more, share a data fragment (asserted
/// below); at 32 KiB, a fragment of raw blocks would hold one.
pub(crate) const BLOCK_TARGET_LEN: usize = 32 * 1024 - 32;

/// The largest raw size of one inline value: a nullable text value of
/// [`MAX_INLINE_VALUE_LEN`] bytes (see [`bytes_raw_len`](crate::codec::block::bytes_raw_len)).
pub(crate) const MAX_VALUE_RAW_LEN: usize = 4 + MAX_INLINE_VALUE_LEN + 2;

/// The raw size every block but a column's last reaches: the builder closes a block only
/// when the next value would take it past [`BLOCK_TARGET_LEN`].
pub(crate) const MIN_BLOCK_FILL: usize = BLOCK_TARGET_LEN - MAX_VALUE_RAW_LEN;

/// The longest large value; a chunk could not hold a longer one anyway.
pub(crate) const MAX_LARGE_VALUE_LEN: usize = MAX_CHUNK_LEN as usize;

/// The most bytes a data fragment takes, its trailer included: blocks, or a piece of a large
/// value. A builder fills one data fragment at a time, and the backend lends a reader the one
/// a block is in while the block decodes, so this bounds what both hold of a chunk's data
/// (§7.4). It is far below the storage port's [`MAX_FRAGMENT_LEN`], which only fragment 0
/// needs: a directory of [`MAX_BLOCKS`] blocks is longer than a data fragment.
///
/// The format validates against this constant whatever the backend, but its value fits
/// redb, the first backend. redb 4.4 stores a value of at least a page in a leaf of its own,
/// of the value's length plus 20 bytes: the leaf's 4-byte header, the value's 4-byte length,
/// and the fragment key, a `(u64, u32)` of 12 bytes. `allocate_helper` (`page_manager.rs`)
/// rounds every allocation up to a power of two of 4 KiB pages. So a full fragment of
/// 64 KiB − 20 fills 16 pages exactly, where 64 KiB would take 32.
pub(crate) const MAX_DATA_FRAGMENT_LEN: usize = 64 * 1024 - 20;

/// What a data fragment holds before its trailer.
pub(crate) const DATA_PAYLOAD_LEN: usize = MAX_DATA_FRAGMENT_LEN - TRAILER_LEN;

/// What a large value of `len` bytes takes in storage: its bytes, and a trailer for each
/// fragment of them.
pub(crate) const fn large_stored_len(len: usize) -> u64 {
    (len + TRAILER_LEN * len.div_ceil(DATA_PAYLOAD_LEN)) as u64
}

/// Columns of one chunk. With [`MAX_BLOCKS`] and [`MAX_LARGE_VALUES`] it keeps fragment 0
/// within one fragment, checked below. Consumers need a handful: a series chunk has the
/// bucket, the combination and one column per field; closed connections have eight.
pub(crate) const MAX_COLUMNS: usize = 64;

/// Column blocks of one chunk. A chunk planned within [`MAX_CHUNK_LEN`] has at most
/// `MAX_COLUMNS + MAX_CHUNK_LEN / MIN_BLOCK_FILL` (320) blocks; the limit leaves room for
/// chunks whose blocks compress well.
pub(crate) const MAX_BLOCKS: usize = 2048;

/// Large values of one chunk. Each is longer than [`MAX_INLINE_VALUE_LEN`], so
/// [`MAX_CHUNK_LEN`] already allows no more than this.
pub(crate) const MAX_LARGE_VALUES: usize = 256;

/// Fragments of one chunk: fragment 0, at most one data fragment per block (a data fragment
/// is written only with a block in it), and the large values' fragments, of which there
/// are at most one per value plus one per [`DATA_PAYLOAD_LEN`] of their total length.
pub(crate) const MAX_FRAGMENTS: usize =
    1 + MAX_BLOCKS + MAX_LARGE_VALUES + MAX_LARGE_VALUE_LEN.div_ceil(DATA_PAYLOAD_LEN);

// The longest postcard encoding of each record of fragment 0. postcard writes a `u8` and a
// `bool` as one byte, an `Option` and an enum variant with a one-byte tag first, and every
// wider integer as a varint of 7 bits per byte: at most 3 bytes for a `u16`, 5 for a `u32`,
// and 10 for a `u64` or a zigzag `i64`. A test encodes each record at its longest.

/// [`IntStats`]: a range of two `i64` or two `u64`, a sum and a presence set.
pub(crate) const MAX_STATS_LEN: usize = (1 + 1 + 10 + 10) + (1 + 10) + (1 + 10);
/// [`HeaderRecord`].
pub(crate) const MAX_HEADER_LEN: usize = 1 + 10 + 5 + 5 + 10 + 10;
/// [`ColumnRecord`].
pub(crate) const MAX_COLUMN_LEN: usize = 1 + 1 + 3 + 1 + 1 + 5 + MAX_STATS_LEN;
/// [`BlockRecord`].
pub(crate) const MAX_BLOCK_ENTRY_LEN: usize = 3 + 1 + 1 + 6 * 5 + MAX_STATS_LEN;
/// [`LargeEntry`].
pub(crate) const MAX_LARGE_ENTRY_LEN: usize = 3 * 5;
/// The length prefix of a list of fragment 0. Every list is shorter than 2^14 records.
pub(crate) const MAX_LIST_PREFIX_LEN: usize = 2;

// The shortest postcard encoding of the records that parsing copies, a byte per field with
// every `Option` absent, which bounds how many of them a fragment 0 can hold. A test encodes
// each record at its shortest.

/// [`HeaderRecord`].
pub(crate) const MIN_HEADER_LEN: usize = 6;
/// [`ColumnRecord`].
pub(crate) const MIN_COLUMN_LEN: usize = 6 + 3;
/// [`BlockRecord`].
pub(crate) const MIN_BLOCK_ENTRY_LEN: usize = 9 + 3;
/// [`LargeEntry`].
pub(crate) const MIN_LARGE_ENTRY_LEN: usize = 3;

/// The shortest fragment 0, trailer included: the prefix, a header, the three lists' one-byte
/// length prefixes, and the one column and one block every chunk has.
pub(crate) const MIN_FRAGMENT0_LEN: usize =
    PREFIX_LEN + MIN_HEADER_LEN + 3 + MIN_COLUMN_LEN + MIN_BLOCK_ENTRY_LEN + TRAILER_LEN;

/// The lengths fragment 0 can have, trailer included. A manifest entry that records another is
/// damaged.
pub(crate) const FRAGMENT0_LENS: RangeInclusive<usize> = MIN_FRAGMENT0_LEN..=MAX_FRAGMENT_LEN;

/// The longest fragment 0 of a chunk with these counts, trailer included.
pub(crate) const fn max_fragment0_len(columns: usize, blocks: usize, large_values: usize) -> usize {
    PREFIX_LEN
        + MAX_HEADER_LEN
        + 3 * MAX_LIST_PREFIX_LEN
        + columns * MAX_COLUMN_LEN
        + blocks * MAX_BLOCK_ENTRY_LEN
        + large_values * MAX_LARGE_ENTRY_LEN
        + TRAILER_LEN
}

const _: () = assert!(MAX_COLUMNS < 1 << 14 && MAX_BLOCKS < 1 << 14 && MAX_LARGE_VALUES < 1 << 14);
const _: () =
    assert!(max_fragment0_len(MAX_COLUMNS, MAX_BLOCKS, MAX_LARGE_VALUES) <= MAX_FRAGMENT_LEN);
const _: () = assert!(MAX_DATA_FRAGMENT_LEN <= MAX_FRAGMENT_LEN);
const _: () =
    assert!(2 * (BLOCK_TARGET_LEN + MAX_BLOCK_OVERHEAD).next_multiple_of(8) <= DATA_PAYLOAD_LEN);
const _: () = assert!(MAX_BLOCKS * BLOCK_TARGET_LEN >= MAX_CHUNK_LEN as usize);
const _: () = assert!(BLOCK_TARGET_LEN <= MAX_BLOCK_LEN && MIN_BLOCK_FILL > 0);

/// The discriminant is the kind tag of the header, so a variant is never renumbered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub(crate) enum ChunkKind {
    /// Journal records turned into a chunk (§4.2).
    L0Base = 1,
    /// Late writes to a bucket that already has a chunk (§4.3).
    L0Delta = 2,
    /// All L0 chunks of an extent, with compressed column blocks (§4.3).
    Consolidated = 3,
}

/// How a column is stored. The write path maps schema columns onto these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysicalType {
    /// `summed` applies to `u64` columns only: their blocks and the chunk record saturating
    /// sums, so a query can total a series field from headers alone (§6.1).
    Int {
        ty: IntType,
        summed: bool,
    },
    Bytes(BytesHint),
}

/// The widths of a fixed bytes column: at least one byte, as the schema's fixed columns
/// are, and no longer than an inline value.
const FIXED_WIDTHS: RangeInclusive<usize> = 1..=MAX_INLINE_VALUE_LEN;

impl PhysicalType {
    /// Whether a chunk can store a column of this type: only a `u64` column is summed, and a
    /// fixed width is one of [`FIXED_WIDTHS`].
    pub(crate) fn is_storable(self) -> bool {
        match self {
            Self::Int { ty, summed } => !summed || ty == IntType::U64,
            Self::Bytes(BytesHint::Fixed(width)) => FIXED_WIDTHS.contains(&usize::from(width)),
            Self::Bytes(BytesHint::LowCardinality | BytesHint::Text) => true,
        }
    }
}

/// The type tag of a column record: the physical type with each integer type numbered
/// apart. The discriminant is the stored value, so a variant is never renumbered. A bytes
/// column's [`BytesHintKind`] follows in the record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub(crate) enum ColumnTag {
    I64 = 1,
    U64 = 2,
    U32 = 3,
    Bytes = 4,
}

impl ColumnTag {
    /// The integer type of the tag, or `None` for a bytes column.
    fn int_type(self) -> Option<IntType> {
        match self {
            Self::I64 => Some(IntType::I64),
            Self::U64 => Some(IntType::U64),
            Self::U32 => Some(IntType::U32),
            Self::Bytes => None,
        }
    }
}

impl From<PhysicalType> for ColumnTag {
    fn from(ty: PhysicalType) -> Self {
        match ty {
            PhysicalType::Int {
                ty: IntType::I64, ..
            } => Self::I64,
            PhysicalType::Int {
                ty: IntType::U64, ..
            } => Self::U64,
            PhysicalType::Int {
                ty: IntType::U32, ..
            } => Self::U32,
            PhysicalType::Bytes(_) => Self::Bytes,
        }
    }
}

/// An integer column's range of non-null values, in the column's own order. Its variant
/// follows the column's type, and their order is part of the format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum IntRange {
    Signed { min: i64, max: i64 },
    Unsigned { min: u64, max: u64 },
}

impl IntRange {
    /// From the native bits of the smallest and largest value of a column of type `ty`.
    pub(crate) fn from_bits(ty: IntType, min: u64, max: u64) -> Self {
        match ty {
            IntType::I64 => Self::Signed {
                min: min as i64,
                max: max as i64,
            },
            IntType::U64 | IntType::U32 => Self::Unsigned { min, max },
        }
    }

    /// Whether the range is of the kind a column of type `ty` has, and not inverted.
    fn fits(self, ty: IntType) -> bool {
        match self {
            Self::Signed { min, max } => ty == IntType::I64 && min <= max,
            Self::Unsigned { min, max } => ty != IntType::I64 && min <= max,
        }
    }

    fn union(self, other: Self) -> Self {
        match (self, other) {
            (
                Self::Signed { min, max },
                Self::Signed {
                    min: low,
                    max: high,
                },
            ) => Self::Signed {
                min: min.min(low),
                max: max.max(high),
            },
            (
                Self::Unsigned { min, max },
                Self::Unsigned {
                    min: low,
                    max: high,
                },
            ) => Self::Unsigned {
                min: min.min(low),
                max: max.max(high),
            },
            // One column's blocks share its type.
            (range, _) => range,
        }
    }
}

/// The statistics a query skips blocks and chunks by (§6.1). All of them are absent for a
/// bytes column. The fields are stored in this order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IntStats {
    /// `None` when every value is null.
    pub(crate) range: Option<IntRange>,
    /// The saturating sum of a summed column's values.
    pub(crate) sum: Option<u64>,
    /// Bit `v` is set when value `v` occurs; present only when every non-null value lies in
    /// `0..64`. The core-log level filter skips blocks by it (§11.2).
    pub(crate) presence: Option<u64>,
}

impl IntStats {
    /// The statistics of an integer column with no blocks yet, to merge blocks into.
    pub(crate) fn empty(summed: bool) -> Self {
        Self {
            range: None,
            sum: summed.then_some(0),
            presence: Some(0),
        }
    }

    pub(crate) fn merge(&mut self, block: &Self) {
        self.range = match (self.range, block.range) {
            (Some(range), Some(other)) => Some(range.union(other)),
            (range, other) => range.or(other),
        };
        self.sum = self
            .sum
            .zip(block.sum)
            .map(|(sum, other)| sum.saturating_add(other));
        self.presence = self
            .presence
            .zip(block.presence)
            .map(|(bits, other)| bits | other);
    }

    /// Whether these are statistics a column of type `ty` has.
    fn fits(&self, ty: PhysicalType) -> bool {
        match ty {
            PhysicalType::Int { ty, summed } => {
                self.sum.is_some() == summed && self.range.is_none_or(|range| range.fits(ty))
            }
            PhysicalType::Bytes(_) => *self == Self::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) kind: ChunkKind,
    /// The schema the chunk was written under, as the caller fingerprints it.
    pub(crate) fingerprint: u64,
    pub(crate) rows: u32,
    pub(crate) fragments: u32,
    /// The key range: buckets for a series chunk, keys for an event chunk.
    pub(crate) key_min: i64,
    pub(crate) key_max: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ColumnDesc {
    pub(crate) ty: PhysicalType,
    pub(crate) nullable: bool,
    /// The column's first entry in the directory. Not stored: the blocks of the columns
    /// follow each other.
    pub(crate) first_block: u32,
    pub(crate) blocks: u32,
    /// The chunk stats: those of all the column's blocks merged.
    pub(crate) stats: IntStats,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlockEntry {
    pub(crate) column: u16,
    pub(crate) codec: Codec,
    pub(crate) encoding: Encoding,
    pub(crate) first_row: u32,
    pub(crate) rows: u32,
    pub(crate) fragment: u32,
    pub(crate) offset: u32,
    pub(crate) stored_len: u32,
    pub(crate) uncompressed_len: u32,
    pub(crate) stats: IntStats,
}

impl BlockEntry {
    pub(crate) fn shape(&self) -> BlockShape {
        BlockShape {
            codec: self.codec,
            encoding: self.encoding,
            uncompressed_len: self.uncompressed_len as usize,
            rows: self.rows as usize,
        }
    }

    fn end(&self) -> u32 {
        self.offset + self.stored_len
    }
}

/// An entry of the large-value directory, stored as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LargeEntry {
    pub(crate) first_fragment: u32,
    pub(crate) fragments: u32,
    pub(crate) len: u32,
}

/// Fragment 0 after its prefix, as stored. The builder fills it in; [`parse_directory`]
/// validates it. Each list stops decoding at its limit.
#[cfg_attr(test, derive(Clone))]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DirectoryRecord {
    pub(crate) header: HeaderRecord,
    #[serde(deserialize_with = "bounded::<_, _, MAX_COLUMNS>")]
    pub(crate) columns: Vec<ColumnRecord>,
    #[serde(deserialize_with = "bounded::<_, _, MAX_BLOCKS>")]
    pub(crate) blocks: Vec<BlockRecord>,
    #[serde(deserialize_with = "bounded::<_, _, MAX_LARGE_VALUES>")]
    pub(crate) large: Vec<LargeEntry>,
}

/// The chunk's header. Its counts are the lengths of the lists that follow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HeaderRecord {
    /// A [`ChunkKind`].
    pub(crate) kind: u8,
    pub(crate) fingerprint: u64,
    pub(crate) rows: u32,
    pub(crate) fragments: u32,
    pub(crate) key_min: i64,
    pub(crate) key_max: i64,
}

/// A column: its type, and the stats of all its blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ColumnRecord {
    /// A [`ColumnTag`].
    pub(crate) ty: u8,
    /// A bytes column's [`BytesHintKind`]; 0 for an integer column.
    pub(crate) hint: u8,
    /// The width of a fixed bytes column; 0 otherwise.
    pub(crate) width: u16,
    pub(crate) nullable: bool,
    pub(crate) summed: bool,
    pub(crate) blocks: u32,
    pub(crate) stats: IntStats,
}

impl ColumnRecord {
    pub(crate) fn new(ty: PhysicalType, nullable: bool, blocks: u32, stats: IntStats) -> Self {
        let (hint, width, summed) = match ty {
            PhysicalType::Int { summed, .. } => (0, 0, summed),
            PhysicalType::Bytes(hint) => {
                let width = match hint {
                    BytesHint::Fixed(width) => width,
                    BytesHint::LowCardinality | BytesHint::Text => 0,
                };
                (BytesHintKind::from(hint) as u8, width, false)
            }
        };
        Self {
            ty: ColumnTag::from(ty) as u8,
            hint,
            width,
            nullable,
            summed,
            blocks,
            stats,
        }
    }
}

/// An entry of the column directory: where one block of a column is, and what it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BlockRecord {
    pub(crate) column: u16,
    /// A [`Codec`].
    pub(crate) codec: u8,
    /// An [`Encoding`] tag.
    pub(crate) encoding: u8,
    pub(crate) first_row: u32,
    pub(crate) rows: u32,
    pub(crate) fragment: u32,
    pub(crate) offset: u32,
    pub(crate) stored_len: u32,
    pub(crate) uncompressed_len: u32,
    pub(crate) stats: IntStats,
}

impl DirectoryRecord {
    /// Appends the content of fragment 0, without its trailer.
    pub(crate) fn write(&self, out: &mut impl std::io::Write) {
        write_directory(self, out);
    }
}

/// Serializes a directory's existing field order directly into an admitted output buffer.
pub(super) fn write_directory(record: &impl serde::Serialize, out: &mut impl std::io::Write) {
    out.write_all(&MAGIC)
        .expect("directory prefix fits its admitted buffer");
    out.write_all(&FORMAT_VERSION.to_le_bytes())
        .expect("directory version fits its admitted buffer");
    // Serializing fails only for a sequence of unknown length, and the records have
    // none.
    postcard::to_io(record, out).expect("a directory record always serializes");
}

impl DirectoryRecord {
    /// Decodes fragment 0's content, its length and checksum already verified, without
    /// validating it. The content is at most one fragment.
    pub(crate) fn read(content: &[u8]) -> Result<Self, ReadError> {
        let header_problem = |problem| InvalidHeaderSnafu { problem };
        let (prefix, body) = content
            .split_first_chunk::<PREFIX_LEN>()
            .context(header_problem(HeaderProblem::Magic))?;
        let (magic, version) = prefix.split_at(MAGIC.len());
        ensure!(magic == MAGIC, header_problem(HeaderProblem::Magic));
        let version = u16::from_le_bytes([version[0], version[1]]);
        ensure!(
            version == FORMAT_VERSION,
            header_problem(HeaderProblem::Version(version))
        );
        let (record, rest) = postcard::take_from_bytes(body).context(DecodeDirectorySnafu)?;
        ensure!(
            rest.is_empty(),
            header_problem(HeaderProblem::TrailingBytes(rest.len()))
        );
        Ok(record)
    }
}

/// Fragment 0, decoded and validated.
#[derive(Debug)]
pub(crate) struct Directory {
    /// Directory storage drops before its aggregate dependency reservation.
    payload: crate::budget::Reserved<DirectoryData>,
}

/// Immutable directory storage retained under one aggregate allocation claim.
#[derive(Debug)]
pub(crate) struct DirectoryData {
    pub(crate) header: Header,
    pub(crate) columns: Vec<ColumnDesc>,
    pub(crate) blocks: Vec<BlockEntry>,
    pub(crate) large: Vec<LargeEntry>,
    /// The content length of every fragment, trailer excluded, by index.
    pub(crate) fragment_lens: Vec<u32>,
}

impl DirectoryData {
    /// Retained directory capacities after parsing workspace has dropped.
    fn memory(&self) -> u64 {
        (held(&self.columns) + held(&self.blocks) + held(&self.large) + held(&self.fragment_lens))
            as u64
    }
}

/// Parses and validates bounded directory lists under the caller's admitted allowance.
pub(super) fn parse_directory(
    content: &[u8],
    observe: impl FnOnce(&[(&'static str, usize)]),
) -> Result<DirectoryData, ReadError> {
    let record = DirectoryRecord::read(content)?;
    let header = parse_header(&record)?;

    let mut columns = Vec::with_capacity(record.columns.len());
    let mut first_block = 0u32;
    for (column, column_record) in record.columns.iter().enumerate() {
        let desc = parse_column(column_record, column as u16, first_block, &record)?;
        first_block += desc.blocks;
        columns.push(desc);
    }
    let blocks_len = record.blocks.len() as u32;
    ensure!(
        first_block == blocks_len,
        InvalidHeaderSnafu {
            problem: HeaderProblem::Blocks(blocks_len)
        }
    );

    let mut blocks = Vec::with_capacity(record.blocks.len());
    for (column, desc) in columns.iter().enumerate() {
        let mut next_row = 0u32;
        let first = desc.first_block as usize;
        for (entry, block_record) in record
            .blocks
            .iter()
            .enumerate()
            .skip(first)
            .take(desc.blocks as usize)
        {
            let block = parse_block(
                block_record,
                entry as u32,
                column as u16,
                desc,
                next_row,
                &header,
            )?;
            next_row += block.rows;
            blocks.push(block);
        }
        ensure!(
            next_row == header.rows,
            InvalidColumnSnafu {
                column: column as u16,
                problem: ColumnProblem::Rows {
                    expected: header.rows,
                    actual: next_row
                }
            }
        );
    }

    for (entry, large) in record.large.iter().enumerate() {
        check_large(large, entry as u32, &header)?;
    }
    // Bounded by MAX_FRAGMENTS, which the header check enforced.
    let mut owners = vec![Owner::None; header.fragments as usize];
    let fragment_lens = fragment_lens(content.len(), &mut owners, &blocks, &record.large)?;
    observe(&[
        ("decoded columns", held(&record.columns)),
        ("decoded blocks", held(&record.blocks)),
        ("decoded large values", held(&record.large)),
        ("columns", held(&columns)),
        ("blocks", held(&blocks)),
        ("fragment owners", held(&owners)),
        ("fragment lengths", held(&fragment_lens)),
    ]);

    Ok(DirectoryData {
        header,
        columns,
        blocks,
        large: record.large,
        fragment_lens,
    })
}

impl Directory {
    /// Retains only validated directory capacities after temporary parsing storage drops.
    pub(super) fn from_payload(payload: crate::budget::Reserved<DirectoryData>) -> Self {
        let bytes = payload.get().memory();
        Self {
            payload: payload.retain_bytes(bytes),
        }
    }

    /// Parses a charged directory and shows `observe` its peak buffer capacities, by
    /// name and in bytes: everything is allocated, and the decoded records are not yet let go.
    #[cfg(test)]
    pub(super) fn parse_observed(
        content: &[u8],
        account: &crate::budget::MemoryAccount,
        observe: impl FnOnce(&[(&'static str, usize)]),
    ) -> Result<Self, ReadError> {
        let payload = account
            .value(
                "chunk directory",
                crate::budget::MemoryPlan::bytes(open_memory(content.len())),
                || parse_directory(content, observe),
            )
            .context(super::error::ReserveDirectorySnafu)?
            .transpose()?;
        Ok(Self::from_payload(payload))
    }

    /// The heap the directory holds: what an open chunk keeps for as long as it is open.
    pub(crate) fn memory(&self) -> u64 {
        (held(&self.columns) + held(&self.blocks) + held(&self.large) + held(&self.fragment_lens))
            as u64
    }
}

/// The [`memory`](Directory::memory) of the directory of a chunk with these counts: parsing
/// gives each list exactly its length, so a caller that knows the counts, as consolidation
/// does from the manifest (§7.4), knows it before it opens the chunk.
pub(crate) const fn directory_memory(
    columns: usize,
    blocks: usize,
    large: usize,
    fragments: usize,
) -> u64 {
    (columns * size_of::<ColumnDesc>()
        + blocks * size_of::<BlockEntry>()
        + large * size_of::<LargeEntry>()
        + fragments * size_of::<u32>()) as u64
}

/// The most [`memory`](Directory::memory) the directory of a chunk of `columns` columns and
/// `fragments` fragments holds, once parsed from a fragment 0 of `len` content bytes: no more
/// blocks and large values than `len` holds at their shortest encodings, nor than `blocks` and
/// `large`, the most the caller's chunks have. A caller that sizes a chunk's reading before it
/// opens it (§7.4) counts on it; a chunk that needs more is not one of its chunks.
pub(crate) const fn directory_within(
    columns: usize,
    len: usize,
    blocks: usize,
    large: usize,
    fragments: usize,
) -> u64 {
    let blocks = if blocks < len / MIN_BLOCK_ENTRY_LEN {
        blocks
    } else {
        len / MIN_BLOCK_ENTRY_LEN
    };
    let large = if large < len / MIN_LARGE_ENTRY_LEN {
        large
    } else {
        len / MIN_LARGE_ENTRY_LEN
    };
    directory_memory(columns, blocks, large, fragments)
}

/// The bytes `buffer` holds.
fn held<T>(buffer: &Vec<T>) -> usize {
    buffer.capacity() * size_of::<T>()
}

/// An upper bound on the heap [`parse_directory`] holds at once for a fragment 0 of `len`
/// content bytes, which a reader knows before it decodes a byte of it (§6.5, §7.4). At its
/// peak, parsing holds:
///
/// - the decoded records: each list has room for no more records than its limit, nor than
///   `len` has bytes, which is what its length prefix can claim (see [`bounded`]);
/// - their validated copies, one per column and per block: no more than `len` holds at a
///   record's shortest encoding;
/// - an owner and a length per fragment, of which the header declares at most
///   [`MAX_FRAGMENTS`].
///
/// The parsed directory then keeps its [`memory`](Directory::memory), which is less.
pub(crate) fn open_memory(len: usize) -> u64 {
    let decoded = |limit: usize, size: usize| len.min(limit) * size;
    let validated = |limit: usize, min_len: usize, size: usize| (len / min_len).min(limit) * size;
    let records = decoded(MAX_COLUMNS, size_of::<ColumnRecord>())
        + decoded(MAX_BLOCKS, size_of::<BlockRecord>())
        + decoded(MAX_LARGE_VALUES, size_of::<LargeEntry>());
    let parsed = validated(MAX_COLUMNS, MIN_COLUMN_LEN, size_of::<ColumnDesc>())
        + validated(MAX_BLOCKS, MIN_BLOCK_ENTRY_LEN, size_of::<BlockEntry>());
    let fragments = MAX_FRAGMENTS * (size_of::<Owner>() + size_of::<u32>());
    (records + parsed + fragments) as u64
}

fn parse_header(record: &DirectoryRecord) -> Result<Header, ReadError> {
    let header_problem = |problem| InvalidHeaderSnafu { problem };
    let HeaderRecord {
        kind,
        fingerprint,
        rows,
        fragments,
        key_min,
        key_max,
    } = record.header;
    let kind = ChunkKind::from_repr(kind).context(header_problem(HeaderProblem::Kind(kind)))?;
    ensure!(
        (1..=MAX_ROWS).contains(&(rows as usize)),
        header_problem(HeaderProblem::Rows(rows))
    );
    let columns = record.columns.len();
    ensure!(
        columns >= 1,
        header_problem(HeaderProblem::Columns(columns as u16))
    );
    // Every column has at least one block, since a chunk has at least one row.
    let blocks = record.blocks.len();
    ensure!(
        blocks >= columns,
        header_problem(HeaderProblem::Blocks(blocks as u32))
    );
    ensure!(
        (1..=MAX_FRAGMENTS).contains(&(fragments as usize)),
        header_problem(HeaderProblem::Fragments(fragments))
    );
    ensure!(
        key_min <= key_max,
        header_problem(HeaderProblem::KeyRange {
            min: key_min,
            max: key_max
        })
    );
    Ok(Header {
        kind,
        fingerprint,
        rows,
        fragments,
        key_min,
        key_max,
    })
}

fn parse_column(
    record: &ColumnRecord,
    column: u16,
    first_block: u32,
    directory: &DirectoryRecord,
) -> Result<ColumnDesc, ReadError> {
    let problem = |problem| InvalidColumnSnafu { column, problem };
    let ColumnRecord {
        ty,
        hint,
        width,
        nullable,
        summed,
        blocks,
        stats,
    } = *record;
    let tag = ColumnTag::from_repr(ty).context(problem(ColumnProblem::Type(ty)))?;
    let ty = if let Some(int) = tag.int_type() {
        ensure!(
            !summed || int == IntType::U64,
            problem(ColumnProblem::Summed)
        );
        ensure!(hint == 0, problem(ColumnProblem::Hint(hint)));
        ensure!(width == 0, problem(ColumnProblem::FixedWidth(width)));
        PhysicalType::Int { ty: int, summed }
    } else {
        ensure!(!summed, problem(ColumnProblem::Summed));
        let hint =
            match BytesHintKind::from_repr(hint).context(problem(ColumnProblem::Hint(hint)))? {
                BytesHintKind::LowCardinality => BytesHint::LowCardinality,
                BytesHintKind::Text => BytesHint::Text,
                BytesHintKind::Fixed => BytesHint::Fixed(width),
            };
        let width_allowed = match hint {
            BytesHint::Fixed(width) => FIXED_WIDTHS.contains(&usize::from(width)),
            _ => width == 0,
        };
        ensure!(width_allowed, problem(ColumnProblem::FixedWidth(width)));
        PhysicalType::Bytes(hint)
    };
    ensure!(
        column != 0
            || (ty
                == PhysicalType::Int {
                    ty: IntType::I64,
                    summed: false
                }
                && !nullable),
        problem(ColumnProblem::Key)
    );
    ensure!(
        blocks >= 1 && u64::from(first_block) + u64::from(blocks) <= directory.blocks.len() as u64,
        problem(ColumnProblem::Blocks(blocks))
    );
    ensure!(stats.fits(ty), problem(ColumnProblem::Stats));
    Ok(ColumnDesc {
        ty,
        nullable,
        first_block,
        blocks,
        stats,
    })
}

fn parse_block(
    record: &BlockRecord,
    entry: u32,
    column: u16,
    desc: &ColumnDesc,
    next_row: u32,
    header: &Header,
) -> Result<BlockEntry, ReadError> {
    let problem = |problem| InvalidBlockEntrySnafu { entry, problem };
    let BlockRecord {
        column: stored_column,
        codec,
        encoding,
        first_row,
        rows,
        fragment,
        offset,
        stored_len,
        uncompressed_len,
        stats,
    } = *record;
    ensure!(
        stored_column == column,
        problem(BlockProblem::Column(stored_column))
    );
    let codec = Codec::from_repr(codec).context(problem(BlockProblem::Codec(codec)))?;
    let encoding_tag = encoding;
    let encoding = Encoding::from_tag(encoding)
        .filter(|&encoding| match (desc.ty, encoding) {
            (PhysicalType::Int { .. }, Encoding::Int(_)) => true,
            (PhysicalType::Bytes(hint), Encoding::Bytes(encoding)) => hint.allows(encoding),
            _ => false,
        })
        .context(problem(BlockProblem::Encoding(encoding_tag)))?;
    ensure!(
        first_row == next_row
            && (1..=MAX_BLOCK_ROWS).contains(&(rows as usize))
            && u64::from(first_row) + u64::from(rows) <= u64::from(header.rows),
        problem(BlockProblem::RowRange {
            first: first_row,
            rows
        })
    );
    ensure!(
        (1..header.fragments).contains(&fragment),
        problem(BlockProblem::Fragment(fragment))
    );
    ensure!(
        offset % 8 == 0
            && stored_len >= 2
            && u64::from(offset) + u64::from(stored_len) <= DATA_PAYLOAD_LEN as u64,
        problem(BlockProblem::Extent {
            offset,
            len: stored_len
        })
    );
    ensure!(
        uncompressed_len as usize <= MAX_BLOCK_LEN
            && (codec != Codec::Raw || stored_len - 2 == uncompressed_len),
        problem(BlockProblem::UncompressedLength(uncompressed_len))
    );
    ensure!(stats.fits(desc.ty), problem(BlockProblem::Stats));
    Ok(BlockEntry {
        column,
        codec,
        encoding,
        first_row,
        rows,
        fragment,
        offset,
        stored_len,
        uncompressed_len,
        stats,
    })
}

fn check_large(entry_record: &LargeEntry, entry: u32, header: &Header) -> Result<(), ReadError> {
    let problem = |problem| InvalidLargeEntrySnafu { entry, problem };
    let LargeEntry {
        first_fragment,
        fragments,
        len,
    } = *entry_record;
    ensure!(
        (MAX_INLINE_VALUE_LEN + 1..=MAX_LARGE_VALUE_LEN).contains(&(len as usize)),
        problem(LargeProblem::Length(len))
    );
    ensure!(
        first_fragment >= 1
            && fragments as usize == (len as usize).div_ceil(DATA_PAYLOAD_LEN)
            && u64::from(first_fragment) + u64::from(fragments) <= u64::from(header.fragments),
        problem(LargeProblem::Fragments {
            first: first_fragment,
            count: fragments
        })
    );
    Ok(())
}

/// Who a fragment belongs to, while [`fragment_lens`] assigns them.
#[derive(Clone, Copy)]
enum Owner {
    None,
    /// The directory itself: fragment 0.
    Directory,
    /// Blocks, the furthest of which ends at `end`.
    Blocks {
        end: u32,
    },
    Large,
}

/// Gives every fragment exactly one owner, and returns the content length each one must
/// have:
///
/// - the blocks of a data fragment lie within it, and it ends where the furthest of them
///   does, since the builder writes nothing after its last block;
/// - each large value owns its run of fragments alone, all full but the last;
/// - no fragment is left without an owner.
///
/// It then bounds the chunk: everything the directory refers to, fragment 0 and every
/// trailer included, takes at most [`MAX_CHUNK_LEN`]. Each block and large value passed its
/// own checks already; this is what they cannot see alone (§6.5).
fn fragment_lens(
    directory_len: usize,
    owners: &mut [Owner],
    blocks: &[BlockEntry],
    large: &[LargeEntry],
) -> Result<Vec<u32>, ReadError> {
    owners[0] = Owner::Directory;
    for block in blocks {
        let owner = &mut owners[block.fragment as usize];
        *owner = match *owner {
            Owner::Blocks { end } => Owner::Blocks {
                end: end.max(block.end()),
            },
            _ => Owner::Blocks { end: block.end() },
        };
    }
    let mut lens = vec![0u32; owners.len()];
    lens[0] = directory_len as u32;
    for (entry, value) in large.iter().enumerate() {
        let mut remaining = value.len as usize;
        for fragment in value.first_fragment..value.first_fragment + value.fragments {
            let owner = &mut owners[fragment as usize];
            ensure!(
                matches!(owner, Owner::None),
                InvalidLargeEntrySnafu {
                    entry: entry as u32,
                    problem: LargeProblem::Shared(fragment)
                }
            );
            *owner = Owner::Large;
            let len = remaining.min(DATA_PAYLOAD_LEN);
            lens[fragment as usize] = len as u32;
            remaining -= len;
        }
    }

    let mut total = (directory_len + TRAILER_LEN) as u64;
    for (fragment, owner) in owners.iter().enumerate() {
        match *owner {
            Owner::None => {
                return InvalidHeaderSnafu {
                    problem: HeaderProblem::UnownedFragment(fragment as u32),
                }
                .fail();
            }
            Owner::Directory => {}
            Owner::Blocks { end } => {
                lens[fragment] = end;
                total += u64::from(end) + TRAILER_LEN as u64;
            }
            Owner::Large => total += u64::from(lens[fragment]) + TRAILER_LEN as u64,
        }
    }
    ensure!(
        total <= MAX_CHUNK_LEN,
        InvalidHeaderSnafu {
            problem: HeaderProblem::ChunkLength(total)
        }
    );
    Ok(lens)
}

impl std::ops::Deref for Directory {
    type Target = DirectoryData;
    /// Borrows the immutable directory while its aggregate charge remains owned.
    fn deref(&self) -> &DirectoryData {
        self.payload.get()
    }
}

#[cfg(test)]
mod tests {
    use serde::de::value::{Error, SeqDeserializer};

    use super::*;

    /// The records `0..records`, under a length prefix that claims `claimed`, or none.
    struct Claimed {
        records: std::ops::Range<u8>,
        claimed: Option<usize>,
    }

    impl Iterator for Claimed {
        type Item = u8;

        fn next(&mut self) -> Option<u8> {
            self.records.next()
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            (self.claimed.unwrap_or(0), self.claimed)
        }
    }

    fn list(records: u8, claimed: Option<usize>) -> Result<Vec<u8>, Error> {
        let claimed = Claimed {
            records: 0..records,
            claimed,
        };
        bounded::<_, _, MAX_BLOCKS>(SeqDeserializer::new(claimed))
    }

    /// A list gets room for exactly the records it claims, once. One that claims no length, or
    /// more records than it holds, fails rather than growing.
    #[test]
    fn a_list_is_allocated_once_at_its_claimed_length() {
        let records = list(3, Some(3)).unwrap();
        assert_eq!(records, [0, 1, 2]);
        assert_eq!(records.capacity(), 3);
        assert!(list(3, None).is_err());
        assert!(list(3, Some(2)).is_err());
        assert!(list(0, Some(MAX_BLOCKS + 1)).is_err());
    }
}
