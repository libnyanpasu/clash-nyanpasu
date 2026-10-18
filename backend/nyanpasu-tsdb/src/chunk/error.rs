//! The errors of building, planning and reading chunks. Each variant names the step that
//! failed; `kind()` gives the recovery decision of §7.5:
//!
//! - a storage failure delegates to the [`StorageError`];
//! - a structural or checksum failure of stored data is `Corrupted`;
//! - a caller exceeding a format limit is `Local`: the batch is abandoned, and the
//!   fragments already written go with it.

use snafu::Snafu;

use super::format::{
    MAX_BLOCKS, MAX_CHUNK_LEN, MAX_COLUMNS, MAX_LARGE_VALUES, MAX_ROWS, PhysicalType,
};
use crate::{ErrorKind, budget::BudgetExceeded, codec::DecodeError, storage::StorageError};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum BuildError {
    #[snafu(display("failed to write fragment {index}"))]
    WriteFragment { index: u32, source: StorageError },

    #[snafu(display("failed to compress a block of column {column}"))]
    CompressBlock {
        column: usize,
        source: std::io::Error,
    },

    #[snafu(display("a chunk has 1 to {MAX_ROWS} rows, not {rows}"))]
    RowCount { rows: usize },

    #[snafu(display("a batch has {actual} columns, but the chunk has {expected}"))]
    ColumnCount { expected: usize, actual: usize },

    #[snafu(display("column {column} of a batch has {actual} rows, not {expected}"))]
    ColumnLength {
        column: usize,
        expected: usize,
        actual: usize,
    },

    #[snafu(display("column {column} has {actual} validity flags for {expected} rows"))]
    ValidityLength {
        column: usize,
        expected: usize,
        actual: usize,
    },

    #[snafu(display("a chunk has at most {MAX_COLUMNS} columns"))]
    TooManyColumns,

    #[snafu(display("column 0 is the key and must be a non-null i64 column"))]
    KeyColumn,

    #[snafu(display("column {column} cannot be stored as {ty:?}"))]
    UnstorableColumn { column: usize, ty: PhysicalType },

    #[snafu(display("the values of column {column} in a batch are not of its type"))]
    ValuesType { column: usize },

    #[snafu(display("column {column} has no nulls, but a batch gives it validity flags"))]
    Nulls { column: usize },

    #[snafu(display("row {row} of column {column} holds {len} bytes, past its column's limit"))]
    ValueTooLong {
        column: usize,
        row: usize,
        len: usize,
    },

    #[snafu(display("row {row} of column {column} holds {len} bytes, not the fixed {width}"))]
    FixedWidth {
        column: usize,
        row: usize,
        len: usize,
        width: u16,
    },

    #[snafu(display("a chunk has at most {MAX_LARGE_VALUES} large values"))]
    TooManyLargeValues,

    #[snafu(display("a chunk has at most {MAX_BLOCKS} column blocks"))]
    TooManyBlocks,

    #[snafu(display(
        "{len} more bytes would take the chunk from {written} past {MAX_CHUNK_LEN} bytes"
    ))]
    ChunkTooLarge { written: u64, len: u64 },

    #[snafu(display("a chunk needs at least one column"))]
    NoColumns,

    #[snafu(display("the chunk has no column {column}"))]
    UnknownColumn { column: usize },

    /// A value written a piece at a time is a large value of a text column: longer than an
    /// inline value, and no longer than a chunk.
    #[snafu(display("column {column} cannot take a large value of {len} bytes"))]
    NotLarge { column: usize, len: usize },

    /// A piece of a large value is not the length the builder cuts a value into.
    #[snafu(display("a piece of a large value has {actual} bytes, not {expected}"))]
    PieceLength { expected: usize, actual: usize },

    #[snafu(display("a large value of {len} bytes ended after {received}"))]
    LargeIncomplete { len: usize, received: usize },
}

impl BuildError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::WriteFragment { source, .. } => source.kind(),
            // zstd fails only when it cannot allocate its context's memory; nothing is
            // wrong with the backend or the data, so the batch alone fails.
            Self::CompressBlock { .. }
            | Self::RowCount { .. }
            | Self::ColumnCount { .. }
            | Self::ColumnLength { .. }
            | Self::ValidityLength { .. }
            | Self::TooManyColumns
            | Self::KeyColumn
            | Self::UnstorableColumn { .. }
            | Self::ValuesType { .. }
            | Self::Nulls { .. }
            | Self::ValueTooLong { .. }
            | Self::FixedWidth { .. }
            | Self::TooManyLargeValues
            | Self::TooManyBlocks
            | Self::ChunkTooLarge { .. }
            | Self::NoColumns
            | Self::UnknownColumn { .. }
            | Self::NotLarge { .. }
            | Self::PieceLength { .. }
            | Self::LargeIncomplete { .. } => ErrorKind::Local,
        }
    }
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum PlanError {
    #[snafu(display("a chunk has at most {MAX_COLUMNS} columns, not {columns}"))]
    PlanColumns { columns: usize },

    #[snafu(display("row {row} alone exceeds the limits of a chunk"))]
    RowTooLarge { row: usize },
}

impl PlanError {
    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Local
    }
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum ReadError {
    #[snafu(display("failed to read fragment {index}"))]
    ReadFragment { index: u32, source: StorageError },

    #[snafu(display("failed to reserve the memory to open a chunk"))]
    ReserveDirectory { source: BudgetExceeded },

    #[snafu(display("fragment {index} is missing"))]
    MissingFragment { index: u32 },

    #[snafu(display("checksum mismatch in fragment {index}"))]
    Checksum { index: u32 },

    /// The chunk's manifest entry records a length no fragment 0 has
    /// ([`FRAGMENT0_LENS`](super::format::FRAGMENT0_LENS)).
    #[snafu(display("the manifest entry records a fragment 0 of {recorded} bytes"))]
    RecordedFragment0 { recorded: u32 },

    /// Fragment 0 does not have the length the chunk's manifest entry records, by which the
    /// reader reserved what parsing it holds.
    #[snafu(display(
        "fragment 0 has {actual} bytes, not the {recorded} its manifest entry records"
    ))]
    Fragment0Length { recorded: u32, actual: usize },

    #[snafu(display("fragment 0 does not decode as a directory"))]
    DecodeDirectory { source: postcard::Error },

    #[snafu(display("invalid chunk header: {problem:?}"))]
    InvalidHeader { problem: HeaderProblem },

    #[snafu(display("invalid descriptor of column {column}: {problem:?}"))]
    InvalidColumn { column: u16, problem: ColumnProblem },

    #[snafu(display("invalid directory entry {entry}: {problem:?}"))]
    InvalidBlockEntry { entry: u32, problem: BlockProblem },

    #[snafu(display("invalid large-value entry {entry}: {problem:?}"))]
    InvalidLargeEntry { entry: u32, problem: LargeProblem },

    #[snafu(display("failed to decode block {block} of column {column}"))]
    DecodeBlock {
        column: u16,
        block: u32,
        source: DecodeError,
    },

    #[snafu(display("column {column} is stored as {actual:?}, not the type asked for"))]
    ColumnType {
        column: u16,
        actual: super::format::PhysicalType,
    },

    #[snafu(display("the chunk has no column {column}"))]
    NoSuchColumn { column: u16 },

    #[snafu(display("column {column} has no block {block}"))]
    NoSuchBlock { column: u16, block: u32 },

    #[snafu(display("the chunk has no large value {index}"))]
    NoSuchLargeValue { index: u32 },

    #[snafu(display("large value {index} has no piece {piece}"))]
    NoSuchPiece { index: u32, piece: u32 },

    #[snafu(display("fragment {index} holds {actual} bytes, not the {expected} of its directory"))]
    ContentLength {
        index: u32,
        expected: usize,
        actual: usize,
    },
}

impl ReadError {
    /// Everything but a storage failure or an exhausted budget means the chunk does not
    /// match its format, or not the schema the caller expects of it. The schema fingerprint
    /// makes the second a corruption too: a chunk of another schema is never opened (§0.3
    /// rule 6).
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ReadFragment { source, .. } => source.kind(),
            Self::ReserveDirectory { source } => source.kind(),
            Self::DecodeBlock { source, .. } => source.kind(),
            Self::MissingFragment { .. }
            | Self::Checksum { .. }
            | Self::RecordedFragment0 { .. }
            | Self::Fragment0Length { .. }
            | Self::DecodeDirectory { .. }
            | Self::InvalidHeader { .. }
            | Self::InvalidColumn { .. }
            | Self::InvalidBlockEntry { .. }
            | Self::InvalidLargeEntry { .. }
            | Self::ColumnType { .. }
            | Self::NoSuchColumn { .. }
            | Self::NoSuchBlock { .. }
            | Self::NoSuchLargeValue { .. }
            | Self::NoSuchPiece { .. }
            | Self::ContentLength { .. } => ErrorKind::Corrupted,
        }
    }
}

/// What is wrong with a chunk header, or with the chunk as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderProblem {
    /// Fragment 0 does not start with the magic.
    Magic,
    Version(u16),
    /// Bytes follow the directory in fragment 0.
    TrailingBytes(usize),
    Kind(u8),
    Rows(u32),
    Columns(u16),
    Blocks(u32),
    Fragments(u32),
    KeyRange {
        min: i64,
        max: i64,
    },
    /// No block or large value is in this fragment.
    UnownedFragment(u32),
    /// The fragments the directory refers to take this many bytes, past the chunk limit.
    ChunkLength(u64),
}

/// What is wrong with a column descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnProblem {
    Type(u8),
    /// Summed, but not a `u64` column.
    Summed,
    Hint(u8),
    FixedWidth(u16),
    /// Column 0 is not a non-null `i64` key.
    Key,
    /// No blocks, or more than the header counts.
    Blocks(u32),
    /// The blocks cover `actual` rows, not the `expected` rows of the chunk.
    Rows {
        expected: u32,
        actual: u32,
    },
    Stats,
}

/// What is wrong with an entry of the column directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockProblem {
    /// The entry is not where its column's blocks are.
    Column(u16),
    Codec(u8),
    /// Unknown, or not one the column's type allows.
    Encoding(u8),
    /// Not the next rows of the column, empty, or past the block or chunk row limit.
    RowRange {
        first: u32,
        rows: u32,
    },
    Fragment(u32),
    /// Unaligned, too short for the tags, or past the largest fragment.
    Extent {
        offset: u32,
        len: u32,
    },
    UncompressedLength(u32),
    Stats,
}

/// What is wrong with an entry of the large-value directory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LargeProblem {
    /// Not long enough to be a large value, or past the largest one.
    Length(u32),
    /// Not the fragments its length needs, or not data fragments of this chunk.
    Fragments { first: u32, count: u32 },
    /// The fragment also holds a block or another large value.
    Shared(u32),
}
