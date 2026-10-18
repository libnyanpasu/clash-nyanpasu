//! The streaming chunk builder (§3.3).
//!
//! - The caller declares the columns, then appends batches of rows, with values for every
//!   column, the key first. A k-way merge can thus hand over rows as it produces them. A
//!   caller that holds one column of its rows at a time hands them over a column at a time
//!   instead ([`ChunkBuilder::append_column`]), and a large value a piece at a time
//!   ([`ChunkBuilder::large_value`]).
//! - Each column keeps one open block. When the block reaches about [`BLOCK_TARGET_LEN`]
//!   raw bytes, it is encoded, compressed when the builder has a compressor, and placed in
//!   the open data fragment, which is written as soon as the next block does not fit in it.
//!   Blocks of different columns therefore interleave in data fragments; the directory
//!   records where each one is.
//! - A large value is written to fragments of its own as soon as the builder reaches it.
//! - [`ChunkBuilder::finish`] closes the open blocks and writes fragment 0 last.
//!
//! The blocks are the same however the rows are batched: a column's block boundaries depend
//! on its values alone.
//!
//! # Memory
//!
//! Every heap buffer the builder owns gets its whole cap, the most a chunk planned within the
//! limits needs of it, the first time it is used, and keeps it from block to block. It never
//! grows after that, so it never reallocates: a reallocation would hold the old and the new
//! allocation together for a moment, past the sum of the caps. [`working_memory`] adds the caps
//! up, for staging to reserve (§7.4); a small chunk holds them too, which the reservation
//! counts anyway.
//!
//! - **Each column's open block.**
//!   - An integer column caps its values, and its validity flags when it is nullable, at the
//!     rows of a block, `BLOCK_TARGET_LEN / int_raw_len`: at most [`BLOCK_TARGET_LEN`]
//!     (32 KiB − 32) together, exactly what a full block holds.
//!   - A bytes column caps its ends (4 bytes each) and validity flags (1 byte each) at the
//!     most rows a block holds, `BLOCK_TARGET_LEN` over [`bytes_raw_len`] of its cheapest
//!     row: an empty or null value, or a full value of a non-null fixed column. It caps its
//!     heap at `BLOCK_TARGET_LEN`, or at those rows' values for a fixed column. A text
//!     column's large-value references add at most [`MAX_LARGE_VALUES`] of 8 bytes (2 KiB).
//!     That is 58.6 KiB for nullable text (6 bytes a row, 5,456 rows), and 63.9 KiB at most,
//!     for a non-null low-cardinality column (4 bytes a row, 8,184 rows).
//! - **The fragment buffer**, at most [`MAX_DATA_FRAGMENT_LEN`] (64 KiB − 20). It holds one
//!   data fragment, one piece of a large value, or fragment 0 at a time, each written as soon
//!   as it is complete. Fragment 0 of a planned chunk fits it, which is checked at compile
//!   time below.
//! - **The encoding scratch** of one block (see [`EncodeScratch`]), sized by the columns: the
//!   longest body, `BLOCK_TARGET_LEN` plus 6 bytes; with a compressor, zstd's bound for that
//!   body; with bytes columns, a `u32` per row of their largest block; and with a
//!   low-cardinality column, a `u32` per row of its largest block for the dictionary's order.
//! - **The directory**: the entries of a planned chunk's blocks, at most
//!   `columns + MAX_CHUNK_LEN / MIN_BLOCK_FILL`, which `MAX_BLOCKS` is far above; with a text
//!   column, the 255 large-value entries such a chunk can hold; and the column records
//!   [`finish`](ChunkBuilder::finish) makes.
//! - **The builder's table of its columns.**
//!
//! The rows of the chunk are never all in memory at once. The zstd context is the caller's
//! and is not counted.
//!
//! The caller keeps a chunk within the limits by planning it (see [`plan`](super::plan)).
//! If it does not, the builder fails with a `Local` error before it writes anything past a
//! limit. An unplanned directory may need a larger fragment-zero buffer, admitted before
//! serialization. The fragments written by then belong to
//! the batch the caller abandons.

use std::ops::Range;

use crate::{
    budget::{BudgetVec, MemoryAccount},
    codec::compress::Compressor,
};
use snafu::{ResultExt, ensure};

use super::{
    FragmentSink,
    error::{
        BuildError, ChunkTooLargeSnafu, ColumnCountSnafu, ColumnLengthSnafu, CompressBlockSnafu,
        FixedWidthSnafu, KeyColumnSnafu, LargeIncompleteSnafu, NoColumnsSnafu, NotLargeSnafu,
        NullsSnafu, PieceLengthSnafu, RowCountSnafu, TooManyBlocksSnafu, TooManyColumnsSnafu,
        TooManyLargeValuesSnafu, UnknownColumnSnafu, UnstorableColumnSnafu, ValidityLengthSnafu,
        ValueTooLongSnafu, ValuesTypeSnafu, WriteFragmentSnafu,
    },
    format::{
        BLOCK_TARGET_LEN, BlockRecord, ChunkKind, ColumnRecord, DATA_PAYLOAD_LEN, HeaderRecord,
        IntRange, IntStats, LargeEntry, MAX_BLOCKS, MAX_CHUNK_LEN, MAX_COLUMNS,
        MAX_DATA_FRAGMENT_LEN, MAX_LARGE_VALUE_LEN, MAX_LARGE_VALUES, MAX_ROWS, MIN_BLOCK_FILL,
        PhysicalType, large_stored_len, max_fragment0_len,
    },
};
use crate::codec::{
    MAX_INLINE_VALUE_LEN,
    block::{
        EncodeScratch, Encoded, MAX_BODY_OVERHEAD, ScratchCaps, bytes_raw_len, encode_bytes,
        encode_ints, int_raw_len,
    },
    bytes::{ByteValues, BytesEncoding, BytesHint},
    checksum::{self, TRAILER_LEN},
    compress,
    int::{IntType, IntValue},
};

/// A column of a chunk, as the write path declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ColumnLayout {
    pub(crate) ty: PhysicalType,
    /// Whether rows may be null. A nullable column costs a byte more per row, so plan with
    /// the same nullability (see [`int_cost`](super::plan::int_cost)).
    pub(crate) nullable: bool,
}

/// The key column every chunk starts with: non-null `i64` values, buckets for a series
/// chunk and keys for an event chunk, whose range the header records.
pub(crate) const KEY_COLUMN: ColumnLayout = ColumnLayout {
    ty: PhysicalType::Int {
        ty: IntType::I64,
        summed: false,
    },
    nullable: false,
};

/// An upper bound on the heap memory a [`ChunkBuilder`] for `columns` holds while it builds a
/// chunk planned within the limits (see [`plan`](super::plan)): the sum of the caps of every
/// buffer it owns, each allocated whole when first used (see [Memory](self#memory)). Staging
/// reserves it before building a chunk (§7.4).
///
/// It does not count the zstd context of a `compressed` builder's compressor, which the
/// caller owns and accounts for separately.
pub(crate) fn working_memory(columns: &[ColumnLayout], compressed: bool) -> u64 {
    let open: usize = columns
        .iter()
        .map(|&layout| caps(&OpenColumn::limits(layout)))
        .sum();
    let scratch = caps(&scratch_caps(columns, compressed).buffers());
    let directory = caps(&DirectoryCaps::of(columns).buffers());
    let table = columns.len() * size_of::<OpenColumn>();
    (open + MAX_DATA_FRAGMENT_LEN + scratch + directory + table) as u64
}

// The fragment buffer holds a data fragment at most, which fits fragment 0 of a chunk planned
// within the limits, whose directory has at most the entries of `DirectoryCaps`. A block the
// builder closes fits an empty data fragment twice, which format.rs asserts.
const _: () = assert!(
    max_fragment0_len(
        MAX_COLUMNS,
        MAX_COLUMNS + MAX_CHUNK_LEN as usize / MIN_BLOCK_FILL,
        MAX_LARGE_VALUES
    ) <= MAX_DATA_FRAGMENT_LEN
);

/// The sum of the caps of `buffers`, each a name, a capacity and a cap.
fn caps(buffers: &[(&'static str, usize, usize)]) -> usize {
    buffers.iter().map(|&(_, _, cap)| cap).sum()
}

/// The most rows a block of a column of `layout` holds, which caps its row buffers: a fixed
/// number for an integer column, and for a bytes column what its cheapest row allows, an
/// empty or null value, or a value of a non-null fixed column's width. Consolidation sizes
/// what reading a chunk's blocks holds by it (§7.4).
pub(crate) fn max_rows(layout: ColumnLayout) -> usize {
    let row = match layout.ty {
        PhysicalType::Int { ty, .. } => int_raw_len(ty, layout.nullable),
        PhysicalType::Bytes(hint) => {
            let len = match hint {
                BytesHint::Fixed(width) if !layout.nullable => usize::from(width),
                BytesHint::Fixed(_) | BytesHint::LowCardinality | BytesHint::Text => 0,
            };
            bytes_raw_len(len, hint, layout.nullable)
        }
    };
    BLOCK_TARGET_LEN / row
}

/// The most blocks and large values the directory of a chunk of `columns` planned within the
/// limits has (see [`plan`](super::plan)), which a reader that sizes what it holds before it
/// opens the chunk counts on (§7.4).
pub(crate) fn planned_directory(columns: &[ColumnLayout]) -> (usize, usize) {
    let caps = DirectoryCaps::of(columns);
    (caps.blocks, caps.large)
}

/// What the encoding scratch of a builder for `columns` needs at most: the longest body of a
/// block of at most [`BLOCK_TARGET_LEN`] raw bytes, its zstd bound when `compressed`, and the
/// rows of the largest bytes block and of the largest block a dictionary may encode.
fn scratch_caps(columns: &[ColumnLayout], compressed: bool) -> ScratchCaps {
    let body = BLOCK_TARGET_LEN + MAX_BODY_OVERHEAD;
    let rows = |of: fn(BytesHint) -> bool| {
        columns
            .iter()
            .filter(|layout| matches!(layout.ty, PhysicalType::Bytes(hint) if of(hint)))
            .map(|&layout| max_rows(layout))
            .max()
            .unwrap_or(0)
    };
    ScratchCaps {
        body,
        frame: if compressed {
            compress::frame_len(body)
        } else {
            0
        },
        bytes_rows: rows(|_| true),
        dictionary_rows: rows(|hint| hint.allows(BytesEncoding::Dictionary)),
    }
}

/// The most directory entries a builder holds: the blocks of a chunk planned within the
/// limits, which have at most [`BLOCK_TARGET_LEN`] raw bytes each and every one but a
/// column's last at least [`MIN_BLOCK_FILL`]; with a text column, the large values such a
/// chunk can hold, each longer than [`MAX_INLINE_VALUE_LEN`]; and a record per column.
#[derive(Clone, Copy, Debug)]
struct DirectoryCaps {
    blocks: usize,
    large: usize,
    columns: usize,
}

impl DirectoryCaps {
    fn of(columns: &[ColumnLayout]) -> Self {
        let text = columns
            .iter()
            .any(|layout| layout.ty == PhysicalType::Bytes(BytesHint::Text));
        Self {
            blocks: columns.len() + MAX_CHUNK_LEN as usize / MIN_BLOCK_FILL,
            large: if text {
                MAX_CHUNK_LEN as usize / large_stored_len(MAX_INLINE_VALUE_LEN + 1) as usize
            } else {
                0
            },
            columns: columns.len(),
        }
    }

    /// Each part's cap in bytes, as `(name, capacity, cap)` with nothing held.
    fn buffers(self) -> [(&'static str, usize, usize); 3] {
        [
            (
                "directory blocks",
                0,
                self.blocks * size_of::<BlockRecord>(),
            ),
            ("directory large", 0, self.large * size_of::<LargeEntry>()),
            (
                "directory columns",
                0,
                self.columns * size_of::<ColumnRecord>(),
            ),
        ]
    }
}

/// One column's values for the rows of a batch.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Values<'a> {
    I64(&'a [i64]),
    U64(&'a [u64]),
    U32(&'a [u32]),
    /// The value of a null row is ignored.
    Bytes(&'a [&'a [u8]]),
}

impl Values<'_> {
    fn len(&self) -> usize {
        match self {
            Self::I64(values) => values.len(),
            Self::U64(values) => values.len(),
            Self::U32(values) => values.len(),
            Self::Bytes(values) => values.len(),
        }
    }

    fn is_of(&self, ty: PhysicalType) -> bool {
        match (self, ty) {
            (Self::I64(_), PhysicalType::Int { ty, .. }) => ty == IntType::I64,
            (Self::U64(_), PhysicalType::Int { ty, .. }) => ty == IntType::U64,
            (Self::U32(_), PhysicalType::Int { ty, .. }) => ty == IntType::U32,
            (Self::Bytes(_), PhysicalType::Bytes(_)) => true,
            _ => false,
        }
    }
}

/// One column of a batch of rows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Column<'a> {
    pub(crate) values: Values<'a>,
    /// One flag per row, `false` for a null; `None` when every row has a value. Only a
    /// nullable column takes flags.
    pub(crate) validity: Option<&'a [bool]>,
}

/// What a finished chunk took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BuiltChunk {
    pub(crate) rows: u32,
    pub(crate) fragments: u32,
    /// Fragment 0's length, its trailer included. The manifest records it, so that a reader
    /// knows what fetching the directory holds before it fetches it (§7.4).
    pub(crate) fragment0: u32,
    /// All fragments together, which retention accounts by (§9.1).
    pub(crate) bytes: u64,
    pub(crate) key_range: (i64, i64),
}

pub(crate) struct ChunkBuilder<'a, S: FragmentSink> {
    kind: ChunkKind,
    fingerprint: u64,
    rows: usize,
    /// The smallest and the largest key so far.
    key_range: Option<(i64, i64)>,
    columns: BudgetVec<OpenColumn>,
    /// The caller's allocation envelope shared by all data buffers.
    account: MemoryAccount,
    encoder: BlockEncoder<'a>,
    out: FragmentWriter<'a, S>,
}

impl<'a, S: FragmentSink> ChunkBuilder<'a, S> {
    /// Starts a chunk of `columns` written to `sink`; column 0 is the [`KEY_COLUMN`]. A
    /// `compressor` compresses the column blocks, which only consolidated chunks do (§6.3
    /// rule 2): L0 chunks are rewritten by consolidation soon, so compressing them would
    /// cost CPU twice. Passing one for an L0 chunk is a programming error of the write path,
    /// and panics.
    pub(crate) fn new(
        sink: &'a mut S,
        kind: ChunkKind,
        fingerprint: u64,
        compressor: Option<&'a mut Compressor>,
        columns: &[ColumnLayout],
        account: &MemoryAccount,
    ) -> Result<Self, BuildError> {
        assert!(
            compressor.is_none() || kind == ChunkKind::Consolidated,
            "only consolidated chunks are compressed"
        );
        ensure!(!columns.is_empty(), NoColumnsSnafu);
        ensure!(columns.len() <= MAX_COLUMNS, TooManyColumnsSnafu);
        ensure!(columns[0] == KEY_COLUMN, KeyColumnSnafu);
        for (column, layout) in columns.iter().enumerate() {
            ensure!(
                layout.ty.is_storable(),
                UnstorableColumnSnafu {
                    column,
                    ty: layout.ty
                }
            );
        }
        let mut opened = account
            .buffer(columns.len())
            .expect("the builder table was admitted");
        for (index, &layout) in columns.iter().enumerate() {
            opened.push(OpenColumn::new(account, index as u16, layout));
        }
        Ok(Self {
            kind,
            fingerprint,
            rows: 0,
            key_range: None,
            columns: opened,
            account: account.clone(),
            encoder: BlockEncoder {
                scratch: EncodeScratch::new(account, scratch_caps(columns, compressor.is_some())),
                compressor,
            },
            out: FragmentWriter {
                sink,
                fragment: account.buffer(0).expect("empty storage is free"),
                open_from: 0,
                next_index: 1,
                written: 0,
                blocks: account.buffer(0).expect("empty storage is free"),
                large: account.buffer(0).expect("empty storage is free"),
                caps: DirectoryCaps::of(columns),
            },
        })
    }

    /// Appends a batch of rows: one [`Column`] for each declared column, all of the same
    /// length.
    ///
    /// A mistake in the batch itself (its shape, a column's type, length or validity, a
    /// value's length) is reported before any of it is taken and leaves the builder as it
    /// was. Any other error leaves the chunk incomplete: drop the builder and abandon the
    /// batch.
    pub(crate) fn append(&mut self, batch: &[Column<'_>]) -> Result<(), BuildError> {
        self.check(batch)?;
        self.take_keys(&batch[0]);
        for (open, column) in self.columns.iter_mut().zip(batch) {
            open.append(column, &mut self.encoder, &mut self.out)?;
        }
        Ok(())
    }

    /// Appends rows to `column` alone, for a caller that hands the rows over a column at a
    /// time, as a merge does that holds one column of them at once. They follow the rows the
    /// column holds already, and [`finish`](Self::finish) checks that every column holds as
    /// many as the key column. A column's blocks depend on its own values alone, so the chunk
    /// holds the blocks [`append`](Self::append) would build; only their places in the data
    /// fragments differ.
    ///
    /// A mistake in the values is reported before any of them is taken, as with `append`.
    pub(crate) fn append_column(
        &mut self,
        column: usize,
        values: &Column<'_>,
    ) -> Result<(), BuildError> {
        ensure!(column < self.columns.len(), UnknownColumnSnafu { column });
        let rows = values.values.len();
        let held = self.columns[column].rows();
        ensure!(held + rows <= MAX_ROWS, RowCountSnafu { rows: held + rows });
        self.check_column(column, values, rows)?;
        if column == 0 {
            self.take_keys(values);
        }
        self.columns[column].append(values, &mut self.encoder, &mut self.out)
    }

    /// Starts the large value of the next row of text column `column`, `len` bytes long, which
    /// the caller then hands over a piece at a time ([`LargeValue::put`]) and records with
    /// [`LargeValue::finish`]. A copy from another chunk thus holds one fragment of the value
    /// at a time, never all of it (§6.5, §7.4). The pieces are those [`append`](Self::append)
    /// cuts a value into, and a chunk's reader hands them out the same way, so each fragment
    /// of the value copied becomes one fragment here. Dropping the guard before it finishes
    /// leaves the chunk incomplete, as any error does.
    pub(crate) fn large_value(
        &mut self,
        column: usize,
        len: usize,
    ) -> Result<LargeValue<'_, 'a, S>, BuildError> {
        ensure!(column < self.columns.len(), UnknownColumnSnafu { column });
        let Self {
            columns,
            encoder,
            out,
            ..
        } = self;
        let open = &mut columns[column];
        let row = open.rows();
        ensure!(row < MAX_ROWS, RowCountSnafu { rows: row + 1 });
        let state = &mut open.state;
        let is_text = state.layout.ty == PhysicalType::Bytes(BytesHint::Text);
        ensure!(
            is_text && (MAX_INLINE_VALUE_LEN + 1..=MAX_LARGE_VALUE_LEN).contains(&len),
            NotLargeSnafu { column, len }
        );
        let OpenValues::Bytes(bytes) = &mut open.values else {
            unreachable!("a text column has bytes values");
        };
        // As `append_bytes` places a large row: the block it closes goes first.
        let cost = bytes_raw_len(0, BytesHint::Text, state.layout.nullable);
        if !bytes.ends.is_empty() && state.raw + cost > BLOCK_TARGET_LEN {
            state.close_bytes(bytes, encoder, out)?;
        }
        bytes.ends.reserve(1, state.max_rows);
        let flag = usize::from(state.layout.nullable);
        state.validity.reserve(flag, state.max_rows);
        bytes.large.reserve(1, MAX_LARGE_VALUES);
        let first_fragment = out.begin_large(len)?;
        Ok(LargeValue {
            builder: self,
            column,
            len,
            received: 0,
            first_fragment,
        })
    }

    /// Takes the keys of `keys`, the key column of rows being appended: the key range, and
    /// the chunk's rows.
    fn take_keys(&mut self, keys: &Column<'_>) {
        let Values::I64(keys) = keys.values else {
            unreachable!("the key column holds i64 values, checked before");
        };
        for &key in keys {
            let (min, max) = self.key_range.get_or_insert((key, key));
            (*min, *max) = ((*min).min(key), (*max).max(key));
        }
        self.rows += keys.len();
    }

    /// Writes the open blocks and fragment 0, and returns what the chunk took.
    pub(crate) fn finish(self) -> Result<BuiltChunk, BuildError> {
        self.finish_observed(|_, _| {})
    }

    /// Like [`finish`](Self::finish), and shows `observe` the builder and the directory when
    /// they hold the most: every block written, and fragment 0 encoded but not yet stored.
    /// The tests check the memory bound there.
    pub(super) fn finish_observed(
        mut self,
        observe: impl FnOnce(&Self, &DirectoryBuffers),
    ) -> Result<BuiltChunk, BuildError> {
        let Some(key_range) = self.key_range else {
            return RowCountSnafu { rows: 0usize }.fail();
        };
        // Columns appended one at a time hold as many rows as the key column only when the
        // caller gave each the same rows.
        for (column, open) in self.columns.iter().enumerate() {
            ensure!(
                open.rows() == self.rows,
                ColumnLengthSnafu {
                    column,
                    expected: self.rows,
                    actual: open.rows()
                }
            );
        }
        for open in self.columns.iter_mut() {
            open.close_block(&mut self.encoder, &mut self.out)?;
        }
        self.out.flush()?;
        let header = HeaderRecord {
            kind: self.kind as u8,
            fingerprint: self.fingerprint,
            rows: self.rows as u32,
            fragments: self.out.next_index,
            key_min: key_range.0,
            key_max: key_range.1,
        };
        // The directory lists each column's blocks together, in row order. Sorting by both
        // gives that order in place: no two blocks share a column and a first row.
        let mut blocks = std::mem::take(&mut self.out.blocks);
        blocks.sort_unstable_by_key(|block| (block.column, block.first_row));
        let mut columns = self
            .account
            .buffer(self.columns.len())
            .expect("the directory list was admitted");
        for open in self.columns.iter() {
            let ColumnState {
                layout,
                blocks,
                stats,
                ..
            } = open.state;
            columns.push(ColumnRecord::new(layout.ty, layout.nullable, blocks, stats));
        }
        let directory = DirectoryBuffers {
            header,
            columns,
            blocks,
            large: std::mem::take(&mut self.out.large),
        };
        // Planned directories fit the existing fragment buffer. An unplanned chunk can have
        // more entries, so admit its format-bounded fragment-zero capacity before writing it.
        let fragment = &mut self.out.fragment;
        let directory_cap = max_fragment0_len(
            directory.columns.len(),
            directory.blocks.len(),
            directory.large.len(),
        );
        fragment
            .try_grow_to(directory_cap)
            .expect("the admitted chunk covers its fragment-zero directory");
        super::format::write_directory(&directory, fragment);
        checksum::seal(fragment);
        let fragment0 = fragment.len() as u32;
        observe(&self, &directory);
        self.out.put(0)?;
        Ok(BuiltChunk {
            rows: header.rows,
            fragments: header.fragments,
            fragment0,
            bytes: self.out.written,
            key_range,
        })
    }

    /// Checks the whole batch before any of it is taken.
    fn check(&self, batch: &[Column<'_>]) -> Result<(), BuildError> {
        ensure!(
            batch.len() == self.columns.len(),
            ColumnCountSnafu {
                expected: self.columns.len(),
                actual: batch.len()
            }
        );
        let rows = batch[0].values.len();
        ensure!(
            self.rows + rows <= MAX_ROWS,
            RowCountSnafu {
                rows: self.rows + rows
            }
        );
        for (column, values) in batch.iter().enumerate() {
            self.check_column(column, values, rows)?;
        }
        Ok(())
    }

    /// Checks `values`, which are to be `rows` rows of `column`.
    fn check_column(
        &self,
        column: usize,
        values: &Column<'_>,
        rows: usize,
    ) -> Result<(), BuildError> {
        let open = &self.columns[column];
        let layout = open.state.layout;
        ensure!(values.values.is_of(layout.ty), ValuesTypeSnafu { column });
        let len = values.values.len();
        ensure!(
            len == rows,
            ColumnLengthSnafu {
                column,
                expected: rows,
                actual: len
            }
        );
        if let Some(validity) = values.validity {
            ensure!(layout.nullable, NullsSnafu { column });
            ensure!(
                validity.len() == rows,
                ValidityLengthSnafu {
                    column,
                    expected: rows,
                    actual: validity.len()
                }
            );
        }
        if let (Values::Bytes(bytes), PhysicalType::Bytes(hint)) = (values.values, layout.ty) {
            self.check_bytes(column, open.rows(), bytes, values.validity, hint)?;
        }
        Ok(())
    }

    /// Checks the values of a bytes column, whose first is row `first_row` of the column.
    fn check_bytes(
        &self,
        column: usize,
        first_row: usize,
        values: &[&[u8]],
        validity: Option<&[bool]>,
        hint: BytesHint,
    ) -> Result<(), BuildError> {
        let valid = |row: usize| validity.is_none_or(|validity| validity[row]);
        for (row, value) in values.iter().enumerate().filter(|&(row, _)| valid(row)) {
            let (row, len) = (first_row + row, value.len());
            if let BytesHint::Fixed(width) = hint {
                ensure!(
                    len == usize::from(width),
                    FixedWidthSnafu {
                        column,
                        row,
                        len,
                        width
                    }
                );
            }
            let limit = match hint {
                BytesHint::Text => MAX_LARGE_VALUE_LEN,
                BytesHint::LowCardinality | BytesHint::Fixed(_) => MAX_INLINE_VALUE_LEN,
            };
            ensure!(len <= limit, ValueTooLongSnafu { column, row, len });
        }
        Ok(())
    }
}

#[cfg(test)]
impl<S: FragmentSink> ChunkBuilder<'_, S> {
    /// Every heap buffer the builder owns: its name, its capacity and its cap, in bytes. The
    /// caps add up to [`working_memory`], and the directory's records are what
    /// [`finish_observed`](Self::finish_observed) shows.
    pub(crate) fn memory(&self) -> Vec<(String, usize, usize)> {
        let named = |prefix: String| {
            move |(name, capacity, cap): (&str, usize, usize)| {
                (format!("{prefix}{name}"), capacity, cap)
            }
        };
        let mut buffers: Vec<_> = self
            .columns
            .iter()
            .enumerate()
            .flat_map(|(index, open)| {
                open.buffers()
                    .into_iter()
                    .map(named(format!("column {index} ")))
            })
            .collect();
        buffers.extend(
            self.encoder
                .scratch
                .buffers()
                .into_iter()
                .map(named(String::new())),
        );
        let caps = self.out.caps.buffers();
        buffers.extend([
            (
                "fragment".to_owned(),
                self.out.fragment.capacity(),
                MAX_DATA_FRAGMENT_LEN,
            ),
            (
                caps[0].0.to_owned(),
                self.out.blocks.capacity() * size_of::<BlockRecord>(),
                caps[0].2,
            ),
            (
                caps[1].0.to_owned(),
                self.out.large.capacity() * size_of::<LargeEntry>(),
                caps[1].2,
            ),
            (caps[2].0.to_owned(), 0, caps[2].2),
            (
                "columns".to_owned(),
                self.columns.capacity() * size_of::<OpenColumn>(),
                self.columns.len() * size_of::<OpenColumn>(),
            ),
        ]);
        buffers
    }

    /// The capacity of each buffer of column `column`'s open block, with its cap: its name,
    /// its capacity and its cap, in elements.
    pub(crate) fn open_buffers(&self, column: usize) -> Vec<(&'static str, usize, usize)> {
        let OpenColumn { state, values } = &self.columns[column];
        let rows = state.max_rows;
        let mut buffers = vec![("validity", state.validity.capacity(), rows)];
        match values {
            OpenValues::I64(open) => buffers.push(("values", open.capacity(), rows)),
            OpenValues::U64(open) => buffers.push(("values", open.capacity(), rows)),
            OpenValues::U32(open) => buffers.push(("values", open.capacity(), rows)),
            OpenValues::Bytes(open) => buffers.extend([
                ("heap", open.heap.capacity(), BLOCK_TARGET_LEN),
                ("ends", open.ends.capacity(), rows),
                ("large", open.large.capacity(), MAX_LARGE_VALUES),
            ]),
        }
        buffers
    }
}

/// A large value being written a piece at a time ([`ChunkBuilder::large_value`]).
pub(crate) struct LargeValue<'b, 'a, S: FragmentSink> {
    builder: &'b mut ChunkBuilder<'a, S>,
    column: usize,
    len: usize,
    /// The bytes of the pieces written so far.
    received: usize,
    first_fragment: u32,
}

impl<S: FragmentSink> LargeValue<'_, '_, S> {
    /// Writes the next piece of the value as a fragment of its own: [`DATA_PAYLOAD_LEN`] bytes,
    /// or for the last piece, what is left of the value.
    pub(crate) fn put(&mut self, piece: &[u8]) -> Result<(), BuildError> {
        let expected = (self.len - self.received).min(DATA_PAYLOAD_LEN);
        ensure!(
            expected > 0 && piece.len() == expected,
            PieceLengthSnafu {
                expected,
                actual: piece.len()
            }
        );
        self.builder.out.put_piece(piece)?;
        self.received += piece.len();
        Ok(())
    }

    /// Records the value in its row of the column, once all of it is written.
    pub(crate) fn finish(self) -> Result<(), BuildError> {
        ensure!(
            self.received == self.len,
            LargeIncompleteSnafu {
                len: self.len,
                received: self.received
            }
        );
        let ChunkBuilder { columns, out, .. } = self.builder;
        let index = out.end_large(self.first_fragment, self.len);
        let open = &mut columns[self.column];
        let OpenValues::Bytes(bytes) = &mut open.values else {
            unreachable!("a large value is in a text column, checked when it started");
        };
        let state = &mut open.state;
        bytes.large.push((bytes.ends.len() as u32, index));
        bytes.ends.push(bytes.heap.len() as u32);
        state.take_validity(None, 0..1);
        state.raw += bytes_raw_len(0, BytesHint::Text, state.layout.nullable);
        Ok(())
    }
}

/// Charged directory lists serialized directly in the existing postcard field order.
#[derive(serde::Serialize)]
pub(super) struct DirectoryBuffers {
    /// The bounded header, serialized before the data-sized lists.
    header: HeaderRecord,
    /// One record per column, charged through fragment-zero serialization.
    pub(super) columns: BudgetVec<ColumnRecord>,
    /// Block records moved from the fragment writer without reallocating.
    pub(super) blocks: BudgetVec<BlockRecord>,
    /// Large-value records retaining their original allocation charges.
    pub(super) large: BudgetVec<LargeEntry>,
}

/// The compressor and the buffers that encode one block at a time.
struct BlockEncoder<'a> {
    compressor: Option<&'a mut Compressor>,
    scratch: EncodeScratch,
}

/// A column being built: its open block, and what its closed blocks add up to.
struct OpenColumn {
    state: ColumnState,
    values: OpenValues,
}

/// The rows of a column's open block.
enum OpenValues {
    I64(BudgetVec<i64>),
    U64(BudgetVec<u64>),
    U32(BudgetVec<u32>),
    Bytes(OpenBytes),
}

/// Everything about a column but the values of its open block.
struct ColumnState {
    index: u16,
    layout: ColumnLayout,
    /// One flag per row of the open block, for a nullable column.
    validity: BudgetVec<bool>,
    /// The most rows a block of the column holds, which caps its row buffers: a fixed number
    /// for an integer column, and what its cheapest row allows for a bytes column.
    max_rows: usize,
    /// The column's row the open block starts at.
    first_row: u32,
    /// The raw size of the open block of a bytes column.
    raw: usize,
    /// The merged stats of the closed blocks.
    stats: IntStats,
    blocks: u32,
}

/// The open block of a bytes column: the inline values back to back, where each row ends,
/// and the rows whose value is a large value. A null or large row holds no bytes.
struct OpenBytes {
    heap: BudgetVec<u8>,
    ends: BudgetVec<u32>,
    /// (row in the block, large-value index), in row order.
    large: BudgetVec<(u32, u32)>,
}

impl ByteValues for OpenBytes {
    fn len(&self) -> usize {
        self.ends.len()
    }

    fn value(&self, row: usize) -> &[u8] {
        let start = row.checked_sub(1).map_or(0, |before| self.ends[before]);
        &self.heap[start as usize..self.ends[row] as usize]
    }
}

impl OpenColumn {
    fn new(account: &MemoryAccount, index: u16, layout: ColumnLayout) -> Self {
        let (values, stats) = match layout.ty {
            PhysicalType::Int { ty, summed } => {
                let values = match ty {
                    IntType::I64 => {
                        OpenValues::I64(account.buffer(0).expect("empty storage is free"))
                    }
                    IntType::U64 => {
                        OpenValues::U64(account.buffer(0).expect("empty storage is free"))
                    }
                    IntType::U32 => {
                        OpenValues::U32(account.buffer(0).expect("empty storage is free"))
                    }
                };
                (values, IntStats::empty(summed))
            }
            PhysicalType::Bytes(_) => (
                OpenValues::Bytes(OpenBytes {
                    heap: account.buffer(0).expect("empty storage is free"),
                    ends: account.buffer(0).expect("empty storage is free"),
                    large: account.buffer(0).expect("empty storage is free"),
                }),
                IntStats::default(),
            ),
        };
        Self {
            state: ColumnState {
                index,
                layout,
                validity: account.buffer(0).expect("empty storage is free"),
                max_rows: max_rows(layout),
                first_row: 0,
                raw: 0,
                stats,
                blocks: 0,
            },
            values,
        }
    }

    /// Planned capacities for one column, without constructing an allocation owner.
    fn limits(layout: ColumnLayout) -> [(&'static str, usize, usize); 4] {
        let rows = max_rows(layout);
        let validity = ("validity", 0, if layout.nullable { rows } else { 0 });
        match layout.ty {
            PhysicalType::Int { ty, .. } => [
                validity,
                ("values", 0, rows * ty.width()),
                ("ends", 0, 0),
                ("large", 0, 0),
            ],
            PhysicalType::Bytes(hint) => [
                validity,
                ("heap", 0, heap_cap(hint, rows)),
                ("ends", 0, rows * size_of::<u32>()),
                (
                    "large",
                    0,
                    if hint == BytesHint::Text {
                        MAX_LARGE_VALUES * size_of::<(u32, u32)>()
                    } else {
                        0
                    },
                ),
            ],
        }
    }

    /// Each buffer of the open block: its name, its capacity and its cap, in bytes.
    fn buffers(&self) -> [(&'static str, usize, usize); 4] {
        let ColumnState {
            layout,
            max_rows: rows,
            ref validity,
            ..
        } = self.state;
        let flags = if layout.nullable { rows } else { 0 };
        let validity = ("validity", validity.capacity(), flags);
        let int = |capacity: usize, width: usize| {
            [
                validity,
                ("values", capacity * width, rows * width),
                ("ends", 0, 0),
                ("large", 0, 0),
            ]
        };
        match &self.values {
            OpenValues::I64(open) => int(open.capacity(), size_of::<i64>()),
            OpenValues::U64(open) => int(open.capacity(), size_of::<u64>()),
            OpenValues::U32(open) => int(open.capacity(), size_of::<u32>()),
            OpenValues::Bytes(open) => {
                let PhysicalType::Bytes(hint) = layout.ty else {
                    unreachable!("only a bytes column has bytes values");
                };
                let large = if hint == BytesHint::Text {
                    MAX_LARGE_VALUES
                } else {
                    0
                };
                let entry = size_of::<(u32, u32)>();
                [
                    validity,
                    ("heap", open.heap.capacity(), heap_cap(hint, rows)),
                    (
                        "ends",
                        open.ends.capacity() * size_of::<u32>(),
                        rows * size_of::<u32>(),
                    ),
                    ("large", open.large.capacity() * entry, large * entry),
                ]
            }
        }
    }

    /// The rows the column holds: those of its closed blocks and of its open one.
    fn rows(&self) -> usize {
        let open = match &self.values {
            OpenValues::I64(open) => open.len(),
            OpenValues::U64(open) => open.len(),
            OpenValues::U32(open) => open.len(),
            OpenValues::Bytes(open) => open.ends.len(),
        };
        self.state.first_row as usize + open
    }

    fn append<S: FragmentSink>(
        &mut self,
        column: &Column<'_>,
        encoder: &mut BlockEncoder<'_>,
        out: &mut FragmentWriter<'_, S>,
    ) -> Result<(), BuildError> {
        let state = &mut self.state;
        let validity = column.validity;
        match (&mut self.values, column.values) {
            (OpenValues::I64(open), Values::I64(values)) => {
                state.append_ints(open, values, validity, encoder, out)
            }
            (OpenValues::U64(open), Values::U64(values)) => {
                state.append_ints(open, values, validity, encoder, out)
            }
            (OpenValues::U32(open), Values::U32(values)) => {
                state.append_ints(open, values, validity, encoder, out)
            }
            (OpenValues::Bytes(open), Values::Bytes(values)) => {
                state.append_bytes(open, values, validity, encoder, out)
            }
            _ => unreachable!("a batch's values are of their column's type, checked first"),
        }
    }

    fn close_block<S: FragmentSink>(
        &mut self,
        encoder: &mut BlockEncoder<'_>,
        out: &mut FragmentWriter<'_, S>,
    ) -> Result<(), BuildError> {
        let state = &mut self.state;
        match &mut self.values {
            OpenValues::I64(open) => state.close_ints(open, encoder, out),
            OpenValues::U64(open) => state.close_ints(open, encoder, out),
            OpenValues::U32(open) => state.close_ints(open, encoder, out),
            OpenValues::Bytes(open) => state.close_bytes(open, encoder, out),
        }
    }
}

impl ColumnState {
    /// Every integer value costs the same, so a block is a fixed number of rows.
    fn append_ints<T: IntValue, S: FragmentSink>(
        &mut self,
        open: &mut BudgetVec<T>,
        values: &[T],
        validity: Option<&[bool]>,
        encoder: &mut BlockEncoder<'_>,
        out: &mut FragmentWriter<'_, S>,
    ) -> Result<(), BuildError> {
        let rows_per_block = self.max_rows;
        let flags = usize::from(self.layout.nullable);
        let mut start = 0;
        while start < values.len() {
            let rows = start..values.len().min(start + rows_per_block - open.len());
            open.reserve(rows.len(), rows_per_block);
            self.validity.reserve(flags * rows.len(), rows_per_block);
            open.extend_from_slice(&values[rows.clone()]);
            self.take_validity(validity, rows.clone());
            start = rows.end;
            if open.len() == rows_per_block {
                self.close_ints(open, encoder, out)?;
            }
        }
        Ok(())
    }

    fn append_bytes<S: FragmentSink>(
        &mut self,
        open: &mut OpenBytes,
        values: &[&[u8]],
        validity: Option<&[bool]>,
        encoder: &mut BlockEncoder<'_>,
        out: &mut FragmentWriter<'_, S>,
    ) -> Result<(), BuildError> {
        let PhysicalType::Bytes(hint) = self.layout.ty else {
            unreachable!("only a bytes column has bytes values");
        };
        for (row, value) in values.iter().enumerate() {
            let valid = validity.is_none_or(|validity| validity[row]);
            let len = if valid { value.len() } else { 0 };
            let large = hint == BytesHint::Text && len > MAX_INLINE_VALUE_LEN;
            let cost = bytes_raw_len(if large { 0 } else { len }, hint, self.layout.nullable);
            if !open.ends.is_empty() && self.raw + cost > BLOCK_TARGET_LEN {
                self.close_bytes(open, encoder, out)?;
            }
            // The row is within the block now: its inline bytes within the heap's cap, and its
            // end and flag within the rows a block holds.
            let inline = if large || !valid { 0 } else { len };
            open.heap.reserve(inline, heap_cap(hint, self.max_rows));
            open.ends.reserve(1, self.max_rows);
            let flag = usize::from(self.layout.nullable);
            self.validity.reserve(flag, self.max_rows);
            if large {
                // A chunk has at most MAX_LARGE_VALUES, which `write_large` enforces.
                open.large.reserve(1, MAX_LARGE_VALUES);
                let index = out.write_large(value)?;
                open.large.push((open.ends.len() as u32, index));
            } else if valid {
                open.heap.extend_from_slice(value);
            }
            open.ends.push(open.heap.len() as u32);
            self.take_validity(validity, row..row + 1);
            self.raw += cost;
        }
        Ok(())
    }

    /// Records the validity of `rows` of a batch in the open block of a nullable column.
    fn take_validity(&mut self, validity: Option<&[bool]>, rows: Range<usize>) {
        if !self.layout.nullable {
            return;
        }
        match validity {
            Some(validity) => self.validity.extend_from_slice(&validity[rows]),
            None => self.validity.resize(self.validity.len() + rows.len(), true),
        }
    }

    fn close_ints<T: IntValue, S: FragmentSink>(
        &mut self,
        open: &mut BudgetVec<T>,
        encoder: &mut BlockEncoder<'_>,
        out: &mut FragmentWriter<'_, S>,
    ) -> Result<(), BuildError> {
        if open.is_empty() {
            return Ok(());
        }
        let validity = self.layout.nullable.then_some(&self.validity[..]);
        let summed = matches!(self.layout.ty, PhysicalType::Int { summed: true, .. });
        let stats = int_stats(open, validity, summed);
        let encoded = encode_ints(
            open,
            validity,
            encoder.compressor.as_deref_mut(),
            &mut encoder.scratch,
        )
        .context(CompressBlockSnafu {
            column: usize::from(self.index),
        })?;
        out.append(self.index, self.first_row, open.len(), &encoded, stats)?;
        self.closed(open.len(), &stats);
        open.clear();
        Ok(())
    }

    fn close_bytes<S: FragmentSink>(
        &mut self,
        open: &mut OpenBytes,
        encoder: &mut BlockEncoder<'_>,
        out: &mut FragmentWriter<'_, S>,
    ) -> Result<(), BuildError> {
        let PhysicalType::Bytes(hint) = self.layout.ty else {
            unreachable!("only a bytes column has bytes values");
        };
        let rows = open.ends.len();
        if rows == 0 {
            return Ok(());
        }
        let encoded = encode_bytes(
            &*open,
            self.layout.nullable.then_some(&self.validity[..]),
            &open.large,
            hint,
            encoder.compressor.as_deref_mut(),
            &mut encoder.scratch,
        )
        .context(CompressBlockSnafu {
            column: usize::from(self.index),
        })?;
        let stats = IntStats::default();
        out.append(self.index, self.first_row, rows, &encoded, stats)?;
        self.closed(rows, &stats);
        open.heap.clear();
        open.ends.clear();
        open.large.clear();
        Ok(())
    }

    /// Accounts for the block of `rows` rows just written, and empties the open block's
    /// validity.
    fn closed(&mut self, rows: usize, stats: &IntStats) {
        self.stats.merge(stats);
        self.blocks += 1;
        self.first_row += rows as u32;
        self.raw = 0;
        self.validity.clear();
    }
}

/// The most inline bytes a block of a bytes column holds: the values of its rows for a fixed
/// column, where every row with a value has the column's width, and [`BLOCK_TARGET_LEN`]
/// otherwise.
fn heap_cap(hint: BytesHint, rows: usize) -> usize {
    match hint {
        BytesHint::Fixed(width) => BLOCK_TARGET_LEN.min(rows * usize::from(width)),
        BytesHint::LowCardinality | BytesHint::Text => BLOCK_TARGET_LEN,
    }
}

/// The block statistics over the non-null values.
fn int_stats<T: IntValue>(values: &[T], validity: Option<&[bool]>, summed: bool) -> IntStats {
    let ty = T::TYPE;
    let mut range: Option<(u64, u64)> = None;
    let mut sum = 0u64;
    let mut presence = Some(0u64);
    for (row, value) in values.iter().enumerate() {
        if validity.is_some_and(|validity| !validity[row]) {
            continue;
        }
        let bits = value.to_bits();
        let ordered = ty.ordered(bits);
        range = Some(range.map_or((ordered, ordered), |(min, max)| {
            (min.min(ordered), max.max(ordered))
        }));
        sum = sum.saturating_add(bits);
        let small = match ty {
            IntType::I64 => (0..64).contains(&(bits as i64)),
            IntType::U64 | IntType::U32 => bits < 64,
        };
        presence = presence.filter(|_| small).map(|set| set | 1 << bits);
    }
    IntStats {
        range: range.map(|(min, max)| IntRange::from_bits(ty, ty.ordered(min), ty.ordered(max))),
        sum: summed.then_some(sum),
        presence,
    }
}

/// Places blocks and large values in fragments and writes the fragments.
struct FragmentWriter<'a, S> {
    sink: &'a mut S,
    /// The open data fragment, without its trailer.
    fragment: BudgetVec<u8>,
    /// The first directory entry in the open fragment. The fragment gets its index when it
    /// is written, so that a large value met meanwhile can take the indices before it.
    open_from: usize,
    next_index: u32,
    written: u64,
    /// The directory entries in the order the blocks were placed.
    blocks: BudgetVec<BlockRecord>,
    large: BudgetVec<LargeEntry>,
    caps: DirectoryCaps,
}

impl<S: FragmentSink> FragmentWriter<'_, S> {
    fn append(
        &mut self,
        column: u16,
        first_row: u32,
        rows: usize,
        encoded: &Encoded<'_>,
        stats: IntStats,
    ) -> Result<(), BuildError> {
        ensure!(self.blocks.len() < MAX_BLOCKS, TooManyBlocksSnafu);
        let len = encoded.stored_len();
        let mut offset = self.fragment.len().next_multiple_of(8);
        if offset + len > DATA_PAYLOAD_LEN {
            self.flush()?;
            offset = 0;
        }
        // A block of at most BLOCK_TARGET_LEN raw bytes fits an empty data fragment (format.rs
        // asserts it).
        debug_assert!(len <= DATA_PAYLOAD_LEN);
        // With the trailer that `flush` seals the fragment with, so that sealing never grows it.
        let additional = offset + len + TRAILER_LEN - self.fragment.len();
        self.fragment.reserve(additional, MAX_DATA_FRAGMENT_LEN);
        self.fragment.resize(offset, 0);
        encoded.write(&mut self.fragment);
        // A chunk planned within the limits has at most the planned blocks; one that is not
        // can grow to the format's limit.
        let max = if self.blocks.len() < self.caps.blocks {
            self.caps.blocks
        } else {
            MAX_BLOCKS
        };
        if self.blocks.len() == self.blocks.capacity() {
            self.blocks
                .try_grow_to(max)
                .expect("the admitted chunk covers its block directory");
        }
        self.blocks.push(BlockRecord {
            column,
            codec: encoded.codec as u8,
            encoding: encoded.encoding.tag(),
            first_row,
            rows: rows as u32,
            fragment: 0,
            offset: offset as u32,
            stored_len: len as u32,
            uncompressed_len: encoded.uncompressed_len as u32,
            stats,
        });
        Ok(())
    }

    /// Writes the open data fragment, if it holds a block.
    fn flush(&mut self) -> Result<(), BuildError> {
        if self.fragment.is_empty() {
            return Ok(());
        }
        let index = self.next_index;
        for block in &mut self.blocks[self.open_from..] {
            block.fragment = index;
        }
        self.open_from = self.blocks.len();
        checksum::seal(&mut self.fragment);
        self.put(index)?;
        self.next_index += 1;
        self.fragment.clear();
        Ok(())
    }

    /// Writes `value` to fragments of its own and returns its large-value index.
    fn write_large(&mut self, value: &[u8]) -> Result<u32, BuildError> {
        let first_fragment = self.begin_large(value.len())?;
        for piece in value.chunks(DATA_PAYLOAD_LEN) {
            self.put_piece(piece)?;
        }
        Ok(self.end_large(first_fragment, value.len()))
    }

    /// Makes room for a large value of `len` bytes and returns its first fragment's index:
    /// the value shares the one fragment buffer, so the open data fragment goes first.
    fn begin_large(&mut self, len: usize) -> Result<u32, BuildError> {
        ensure!(self.large.len() < MAX_LARGE_VALUES, TooManyLargeValuesSnafu);
        self.flush()?;
        let stored = large_stored_len(len);
        ensure!(
            self.written + stored <= MAX_CHUNK_LEN,
            ChunkTooLargeSnafu {
                written: self.written,
                len: stored
            }
        );
        Ok(self.next_index)
    }

    /// Writes the next piece of the large value being written, as a fragment of its own.
    fn put_piece(&mut self, piece: &[u8]) -> Result<(), BuildError> {
        self.fragment.clear();
        self.fragment
            .reserve(piece.len() + TRAILER_LEN, MAX_DATA_FRAGMENT_LEN);
        self.fragment.extend_from_slice(piece);
        checksum::seal(&mut self.fragment);
        self.put(self.next_index)?;
        self.next_index += 1;
        Ok(())
    }

    /// Records the large value of `len` bytes whose pieces start at `first_fragment`, and
    /// returns its index.
    fn end_large(&mut self, first_fragment: u32, len: usize) -> u32 {
        self.fragment.clear();
        // Like the blocks: within a planned chunk's large values, or else the format's limit.
        let max = if self.large.len() < self.caps.large {
            self.caps.large
        } else {
            MAX_LARGE_VALUES
        };
        if self.large.len() == self.large.capacity() {
            self.large
                .try_grow_to(max)
                .expect("the admitted chunk covers its large-value directory");
        }
        self.large.push(LargeEntry {
            first_fragment,
            fragments: self.next_index - first_fragment,
            len: len as u32,
        });
        self.large.len() as u32 - 1
    }

    /// Writes the sealed `fragment` as fragment `index`, unless it would take the chunk past
    /// B.
    fn put(&mut self, index: u32) -> Result<(), BuildError> {
        let len = self.fragment.len() as u64;
        ensure!(
            self.written + len <= MAX_CHUNK_LEN,
            ChunkTooLargeSnafu {
                written: self.written,
                len
            }
        );
        self.sink
            .put(index, &self.fragment)
            .context(WriteFragmentSnafu { index })?;
        self.written += len;
        Ok(())
    }
}
