//! The column block (§6.1): what a chunk stores for one column over a range of rows, and
//! the unit of compression (§6.3).
//!
//! A stored block is
//!
//! ```text
//! codec: u8 | encoding: u8 | payload
//! ```
//!
//! The tags stay outside any compression, so a reader knows how to decode a block before it
//! touches the payload, and new encodings and codecs can join the old ones without
//! rewriting them. The payload is the encoded body as is ([`Codec::Raw`]) or one zstd frame
//! of it ([`Codec::Zstd`]). The body is
//!
//! ```text
//! flags: u8 | rows: u32 | [validity] | [large bitmap | large references] | values
//! ```
//!
//! - **Validity**, only when a row is null: one bit per row, set when the row has a value.
//! - **Large values**, only in a text block that has one: one bit per row, set when the
//!   row's value is a large value of the chunk, then the index of each such row's entry in
//!   the chunk's large-value directory, `u32`, in row order. The indices increase: the builder
//!   numbers large values as it writes them, so a block refers to each at most once, and
//!   holds no more references than the chunk has large values. A text value longer than
//!   [`MAX_INLINE_VALUE_LEN`] lives in fragments of its own (§3.3, §6.5); its row keeps only
//!   the reference. A text block thus mixes inline values and references row by row, and a
//!   reader that never asks for the large values never reads their fragments.
//! - **Values**: an integer block encodes every row, nulls included, so that a raw block
//!   reads in place with row `i` at index `i`. A null row repeats the previous value (or the
//!   block's first value), which costs a one-byte delta and never widens frame of
//!   reference. A bytes block encodes only its present rows, those valid and inline.
//!
//! Bitmaps are least significant bit first.
//!
//! # Memory
//!
//! Decoding a block first sizes every buffer it fills to exactly the block's share of it,
//! which the block's shape and its column give before a byte of the block is read (see
//! [`DecodeMemory`]), and then never grows one. A buffer whose share changes from the last
//! block is let go of, and all of them are, before any is allocated again: the decode never
//! holds a buffer's old and new allocation at once, as a reallocation would, nor the old
//! shares of some buffers with the new shares of others. So it holds at most the larger of
//! what the last block and this one declare, and afterwards exactly what this one declares.

use std::io;

use crate::{
    budget::{BudgetExceeded, BudgetVec, MemoryAccount},
    codec::compress::{Compressor, Decompressor},
};
use snafu::{OptionExt, ensure};
use zerocopy::FromBytes;

use super::{
    BlockFlagsSnafu, Codec, ColumnWidthSnafu, CompressedSnafu, DeclaredLengthSnafu, DecodeError,
    Encoding, LargeOutsideTextSnafu, LargeRefOrderSnafu, LargeRefSnafu, MAX_BLOCK_LEN,
    MAX_BLOCK_ROWS, MissingTagsSnafu, PayloadLengthSnafu, RowCountSnafu, TagMismatchSnafu,
    TooManyRowsSnafu, TrailingBytesSnafu, TruncatedHeaderSnafu, TruncatedIntsSnafu,
    TruncatedLargeRefsSnafu, TruncatedValiditySnafu, WrongEncodingSnafu,
    bytes::{self, ByteValues, BytesEncoding, BytesHint, BytesScratch, BytesSizes},
    compress,
    cursor::Cursor,
    int::{self, IntEncoding, IntSizes, IntType, IntValue},
};

const FLAG_NULLS: u8 = 1;
const FLAG_LARGE: u8 = 2;

/// How far a stored block can exceed the raw size of its rows, as [`int_raw_len`] and
/// [`bytes_raw_len`] count it: the two tags, the flags and the row count (7 bytes), and the
/// tag of a bytes block's nested offset column (1 byte). Every encoder has a candidate no
/// larger than the raw size, and compression is kept only when it shrinks the body, so
/// nothing else adds to it. Planning chunks by raw size relies on this bound (§4.3).
pub(crate) const MAX_BLOCK_OVERHEAD: usize = 8;

/// How far a block's body, without the two tags, can exceed the raw size of its rows.
pub(crate) const MAX_BODY_OVERHEAD: usize = MAX_BLOCK_OVERHEAD - 2;

/// The flags and the row count that start every body.
const HEADER_LEN: usize = 5;

/// The raw size of an integer value: its width, plus a byte in a nullable column, which
/// pays for the validity bitmap (a bit per row, rounded up once per block).
pub(crate) fn int_raw_len(ty: IntType, nullable: bool) -> usize {
    ty.width() + usize::from(nullable)
}

/// The raw size of a bytes value of `len` bytes held inline; a null row or a large-value
/// reference counts as `len = 0`. Four bytes pay for its offset (or a large row's
/// reference), one for the validity bitmap of a nullable column, and one for the
/// large-value bitmap of a text column.
pub(crate) fn bytes_raw_len(len: usize, hint: BytesHint, nullable: bool) -> usize {
    4 + len + usize::from(nullable) + usize::from(hint == BytesHint::Text)
}

/// The most an [`EncodeScratch`] grows each of its buffers to: what the largest block its
/// owner encodes needs of it. The chunk builder derives them from its columns, so a builder
/// of integer columns only pays nothing for the bytes encoders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScratchCaps {
    /// The longest body, in bytes.
    pub(crate) body: usize,
    /// The longest compressed frame, in bytes: [`compress::frame_len`] of the longest body,
    /// or 0 when nothing is compressed.
    pub(crate) frame: usize,
    /// The most rows of a bytes block, whose present values `present` lists; 0 without
    /// bytes columns.
    pub(crate) bytes_rows: usize,
    /// The most rows of a block a dictionary may encode, which the dictionary's order holds;
    /// 0 when no column allows a dictionary.
    pub(crate) dictionary_rows: usize,
}

/// Room for any block the format allows, for callers that encode single blocks.
impl Default for ScratchCaps {
    fn default() -> Self {
        Self {
            body: MAX_BLOCK_LEN,
            frame: compress::frame_len(MAX_BLOCK_LEN),
            bytes_rows: MAX_BLOCK_ROWS,
            dictionary_rows: MAX_BLOCK_ROWS,
        }
    }
}

impl ScratchCaps {
    /// Planned encoding capacities, without constructing or allocating scratch storage.
    pub(crate) fn buffers(self) -> [(&'static str, usize, usize); 4] {
        [
            ("scratch body", 0, self.body),
            ("scratch frame", 0, self.frame),
            ("scratch present", 0, self.bytes_rows * size_of::<u32>()),
            ("scratch order", 0, self.dictionary_rows * size_of::<u32>()),
        ]
    }
}

/// Buffers the encoder reuses from block to block: the body, its compressed frame, the rows
/// of a bytes block with a value, and the dictionary's order. Each gets its whole cap in
/// [`ScratchCaps`] when first used, and is never given back: the integer encoder reads its values
/// with nulls carried over as it goes, and the offsets of plain and dictionary blocks are
/// computed as they are read, so no buffer holds a copy of the values.
#[derive(Debug)]
pub(crate) struct EncodeScratch {
    body: BudgetVec<u8>,
    frame: BudgetVec<u8>,
    present: BudgetVec<u32>,
    bytes: BytesScratch,
    caps: ScratchCaps,
}

impl EncodeScratch {
    /// Attaches empty encoding buffers to their caller's admitted chunk envelope.
    pub(crate) fn new(account: &MemoryAccount, caps: ScratchCaps) -> Self {
        Self {
            body: account.buffer(0).expect("empty storage is free"),
            frame: account.buffer(0).expect("empty storage is free"),
            present: account.buffer(0).expect("empty storage is free"),
            bytes: BytesScratch::new(account, caps.dictionary_rows),
            caps,
        }
    }

    /// Each buffer: its name, its capacity and its cap, in bytes.
    pub(crate) fn buffers(&self) -> [(&'static str, usize, usize); 4] {
        let rows = size_of::<u32>();
        let (order, max_order) = self.bytes.capacity();
        [
            ("scratch body", self.body.capacity(), self.caps.body),
            ("scratch frame", self.frame.capacity(), self.caps.frame),
            (
                "scratch present",
                self.present.capacity() * rows,
                self.caps.bytes_rows * rows,
            ),
            ("scratch order", order, max_order),
        ]
    }
}

/// A block encoded into an [`EncodeScratch`], ready to store.
#[derive(Debug)]
pub(crate) struct Encoded<'a> {
    pub(crate) codec: Codec,
    pub(crate) encoding: Encoding,
    /// The length of the body before compression.
    pub(crate) uncompressed_len: usize,
    payload: &'a [u8],
}

impl Encoded<'_> {
    pub(crate) fn stored_len(&self) -> usize {
        2 + self.payload.len()
    }

    pub(crate) fn write(&self, out: &mut crate::budget::BudgetVec<u8>) {
        out.push(self.codec as u8);
        out.push(self.encoding.tag());
        out.extend_from_slice(self.payload);
    }
}

/// Encodes one block of an integer column. `validity`, when given, has one flag per value.
/// The caller keeps the block within [`MAX_BLOCK_ROWS`] and [`MAX_BLOCK_LEN`].
pub(crate) fn encode_ints<'s, T: IntValue>(
    values: &[T],
    validity: Option<&[bool]>,
    compressor: Option<&mut Compressor>,
    scratch: &'s mut EncodeScratch,
) -> io::Result<Encoded<'s>> {
    let EncodeScratch {
        body, frame, caps, ..
    } = scratch;
    let nulls = validity.filter(|validity| validity.contains(&false));
    let flags = if nulls.is_some() { FLAG_NULLS } else { 0 };
    let valid = move |row: usize| nulls.is_none_or(|validity| validity[row]);
    // Every row's value, a null row's being the previous value, as the encoders read them.
    let first = (0..values.len())
        .find(|&row| valid(row))
        .map_or(0, |row| values[row].to_bits());
    let bits = values
        .iter()
        .enumerate()
        .scan(first, move |carried, (row, value)| {
            if valid(row) {
                *carried = value.to_bits();
            }
            Some(*carried)
        });
    let (encoding, size) = IntSizes::measure(T::TYPE, bits.clone()).smallest();
    let bitmap = if nulls.is_some() {
        values.len().div_ceil(8)
    } else {
        0
    };
    let len = HEADER_LEN + bitmap + size;
    start_body(body, len, caps.body, values.len(), flags);
    if let Some(validity) = nulls {
        write_bitmap(body, validity.iter().copied());
    }
    int::encode(T::TYPE, encoding, bits, body);
    debug_assert_eq!(body.len(), len);
    finish(Encoding::Int(encoding), compressor, body, frame, caps.frame)
}

/// Encodes one block of a bytes column. `large` lists the rows (within the block) whose
/// value is a large value, each with its large-value index, in row order; their bytes in
/// `values` are ignored, as are those of null rows.
pub(crate) fn encode_bytes<'s>(
    values: &(impl ByteValues + ?Sized),
    validity: Option<&[bool]>,
    large: &[(u32, u32)],
    hint: BytesHint,
    compressor: Option<&mut Compressor>,
    scratch: &'s mut EncodeScratch,
) -> io::Result<Encoded<'s>> {
    let EncodeScratch {
        body,
        frame,
        present,
        bytes: bytes_scratch,
        caps,
    } = scratch;
    let nulls = validity.filter(|validity| validity.contains(&false));
    let mut flags = 0;
    if nulls.is_some() {
        flags |= FLAG_NULLS;
    }
    if !large.is_empty() {
        flags |= FLAG_LARGE;
    }
    let mut large_rows = large.iter().map(|&(row, _)| row as usize).peekable();
    present.clear();
    present.reserve(values.len(), caps.bytes_rows);
    for row in 0..values.len() {
        if large_rows.next_if_eq(&row).is_none() && nulls.is_none_or(|validity| validity[row]) {
            present.push(row as u32);
        }
    }
    let (encoding, size) = BytesSizes::measure(values, present, hint, bytes_scratch).smallest();
    let bitmap = values.len().div_ceil(8);
    let len = HEADER_LEN
        + if nulls.is_some() { bitmap } else { 0 }
        + if large.is_empty() {
            0
        } else {
            bitmap + 4 * large.len()
        }
        + size;
    start_body(body, len, caps.body, values.len(), flags);
    if let Some(validity) = nulls {
        write_bitmap(body, validity.iter().copied());
    }
    if !large.is_empty() {
        let mut rows = large.iter().map(|&(row, _)| row as usize).peekable();
        write_bitmap(
            body,
            (0..values.len()).map(|row| rows.next_if_eq(&row).is_some()),
        );
        for &(_, index) in large {
            body.extend_from_slice(&index.to_le_bytes());
        }
    }
    bytes::encode(encoding, values, present, bytes_scratch, body);
    debug_assert_eq!(body.len(), len);
    finish(
        Encoding::Bytes(encoding),
        compressor,
        body,
        frame,
        caps.frame,
    )
}

/// Empties `body`, makes room for the `len` bytes it is about to hold, within `max`, and
/// writes the header.
fn start_body(body: &mut BudgetVec<u8>, len: usize, max: usize, rows: usize, flags: u8) {
    debug_assert!(rows <= MAX_BLOCK_ROWS);
    body.clear();
    body.reserve(len, max);
    body.push(flags);
    body.extend_from_slice(&(rows as u32).to_le_bytes());
}

fn write_bitmap(out: &mut crate::budget::BudgetVec<u8>, bits: impl Iterator<Item = bool>) {
    let mut byte = 0u8;
    let mut count = 0;
    for bit in bits {
        byte |= u8::from(bit) << (count % 8);
        count += 1;
        if count % 8 == 0 {
            out.push(byte);
            byte = 0;
        }
    }
    if count % 8 != 0 {
        out.push(byte);
    }
}

fn finish<'s>(
    encoding: Encoding,
    compressor: Option<&mut Compressor>,
    body: &'s [u8],
    frame: &'s mut BudgetVec<u8>,
    max_frame: usize,
) -> io::Result<Encoded<'s>> {
    debug_assert!(body.len() <= MAX_BLOCK_LEN);
    let codec = match compressor {
        Some(compressor) => compress::compress(compressor, body, frame, max_frame)?,
        None => Codec::Raw,
    };
    Ok(Encoded {
        codec,
        encoding,
        uncompressed_len: body.len(),
        payload: match codec {
            Codec::Raw => body,
            Codec::Zstd => frame,
        },
    })
}

/// What the chunk directory says about a block. The decoder checks the stored bytes against
/// it rather than trusting either alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlockShape {
    pub(crate) codec: Codec,
    pub(crate) encoding: Encoding,
    pub(crate) uncompressed_len: usize,
    pub(crate) rows: usize,
}

/// What decoding one block holds, in bytes, once decoded: in the block it decodes into, and in
/// the [`BlockDecoder`]'s scratch. It follows from the block's shape and its column, as the
/// chunk directory states them, so that a query reserves it before it reads a byte of the
/// block (§7.4, §8.2). Each part is the sum of the shares a decode sizes its buffers to (see
/// [Memory](self#memory)):
///
/// - **An integer block**: its `rows` values of the type's width, and a validity bit per row.
/// - **A bytes block**: `rows + 1` offsets of 4 bytes; a validity bit per row; for a text
///   column, a large-value reference of 8 bytes per row, at most one per large value of the
///   chunk; and the heap. A plain or fixed block's heap is copied out of its body, so it is at
///   most the body. A dictionary block's values are its entries expanded, each at most the
///   body, and all together at most [`MAX_BLOCK_LEN`].
/// - **The scratch**: a zstd block's decompressed body; the end offsets of a bytes block's
///   present values, at most one per row; and a dictionary's entries, no more than its values.
///
/// The zstd context belongs to the decoder and is not counted (§6.3 rule 5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DecodeMemory {
    pub(crate) output: u64,
    pub(crate) scratch: u64,
}

impl DecodeMemory {
    /// For a block of an integer column of `ty`.
    pub(crate) fn ints(shape: BlockShape, ty: IntType) -> Self {
        let (values, validity) = int_shares(shape);
        Self {
            output: (values * ty.width() + validity) as u64,
            scratch: Scratch::memory(shape),
        }
    }

    /// For a block of a bytes column of `hint`, in a chunk of `large_count` large values.
    pub(crate) fn bytes(shape: BlockShape, large_count: u32, hint: BytesHint) -> Self {
        let (offsets, heap, validity, large) = BytesBlock::shares(shape, large_count, hint);
        Self {
            output: (offsets * size_of::<u32>() + heap + validity + large * size_of::<LargeRef>())
                as u64,
            scratch: Scratch::memory(shape),
        }
    }
}

/// The elements each buffer of an [`IntBlock`] holds for a block of `shape`: its values, and
/// its validity bitmap.
fn int_shares(shape: BlockShape) -> (usize, usize) {
    (shape.rows, shape.rows.div_ceil(8))
}

/// The bytes `buffer` holds.
#[cfg(test)]
fn held<T>(buffer: &BudgetVec<T>) -> usize {
    buffer.capacity() * size_of::<T>()
}

/// Which rows of a decoded block have a value.
#[derive(Debug, Default)]
pub(crate) struct Validity {
    bits: BudgetVec<u8>,
    nulls: bool,
}

impl Validity {
    pub(crate) fn has_nulls(&self) -> bool {
        self.nulls
    }

    /// Panics when `row` is past the block, like indexing.
    pub(crate) fn is_valid(&self, row: usize) -> bool {
        !self.nulls || bit(&self.bits, row)
    }
}

fn bit(bits: &[u8], row: usize) -> bool {
    bits[row / 8] >> (row % 8) & 1 == 1
}

/// A decoded integer block, reused from block to block.
#[derive(Debug, Default)]
pub(crate) struct IntBlock<T> {
    values: BudgetVec<T>,
    validity: Validity,
}

impl<T: IntValue> IntBlock<T> {
    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    /// Every row's value. A null row holds some other value of the block; check
    /// [`validity`](Self::validity).
    pub(crate) fn values(&self) -> &[T] {
        &self.values
    }

    pub(crate) fn validity(&self) -> &Validity {
        &self.validity
    }

    pub(crate) fn get(&self, row: usize) -> Option<T> {
        self.validity.is_valid(row).then(|| self.values[row])
    }

    /// Clears reusable capacities and retires every obsolete allocation before replacement.
    fn discard(&mut self, account: &MemoryAccount, shape: BlockShape) {
        let (values, validity) = int_shares(shape);
        self.values.attach(account);
        self.values.discard_unless(values);
        self.validity.bits.attach(account);
        self.validity.bits.discard_unless(validity);
    }

    /// Allocates only the capacities whose old storage has already been discarded.
    fn allocate(&mut self, shape: BlockShape) -> Result<(), BudgetExceeded> {
        let (values, validity) = int_shares(shape);
        self.values.try_grow_to(values)?;
        self.validity.bits.try_grow_to(validity)?;
        Ok(())
    }

    /// Actual heap capacity retained by this decoded integer output.
    pub(crate) fn memory(&self) -> u64 {
        (self.values.capacity() * size_of::<T>() + self.validity.bits.capacity()) as u64
    }

    /// Each buffer: its name and the bytes it holds.
    #[cfg(test)]
    pub(crate) fn buffers(&self) -> [(&'static str, usize); 2] {
        [
            ("values", held(&self.values)),
            ("validity", held(&self.validity.bits)),
        ]
    }
}

/// A row whose value is a large value: the index of its large-value directory entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LargeRef {
    pub(crate) row: u32,
    pub(crate) index: u32,
}

/// A decoded bytes block, reused from block to block: row offsets into one heap, plus the
/// rows that hold large-value references.
#[derive(Debug, Default)]
pub(crate) struct BytesBlock {
    /// `rows + 1` offsets; a null or large row has an empty range.
    offsets: BudgetVec<u32>,
    heap: BudgetVec<u8>,
    validity: Validity,
    large: BudgetVec<LargeRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BytesValue<'a> {
    Null,
    Inline(&'a [u8]),
    /// The index of a large value, which the chunk reader reads into a caller's buffer.
    Large(u32),
}

impl BytesBlock {
    pub(crate) fn len(&self) -> usize {
        self.offsets.len().saturating_sub(1)
    }

    pub(crate) fn validity(&self) -> &Validity {
        &self.validity
    }

    /// The large-value references, in row order.
    pub(crate) fn large_values(&self) -> &[LargeRef] {
        &self.large
    }

    /// Panics when `row` is past the block, like indexing.
    pub(crate) fn get(&self, row: usize) -> BytesValue<'_> {
        if !self.validity.is_valid(row) {
            return BytesValue::Null;
        }
        if let Ok(found) = self
            .large
            .binary_search_by_key(&(row as u32), |large| large.row)
        {
            return BytesValue::Large(self.large[found].index);
        }
        let range = self.offsets[row] as usize..self.offsets[row + 1] as usize;
        BytesValue::Inline(&self.heap[range])
    }

    /// The elements each buffer holds for a block of `shape` of a column of `hint`, in a
    /// chunk of `large_count` large values: its offsets, its heap, its validity bitmap and its
    /// large-value references.
    fn shares(
        shape: BlockShape,
        large_count: u32,
        hint: BytesHint,
    ) -> (usize, usize, usize, usize) {
        let heap = match shape.encoding {
            // Each value is an entry, which is part of the body; the decoder stops the values
            // at MAX_BLOCK_LEN.
            Encoding::Bytes(BytesEncoding::Dictionary) => shape
                .rows
                .saturating_mul(shape.uncompressed_len)
                .min(MAX_BLOCK_LEN),
            // The values are copied out of the body.
            Encoding::Bytes(BytesEncoding::Plain | BytesEncoding::Fixed) | Encoding::Int(_) => {
                shape.uncompressed_len
            }
        };
        // Only a text block refers to large values, and to each at most once.
        let large = if hint == BytesHint::Text {
            shape.rows.min(large_count as usize)
        } else {
            0
        };
        (shape.rows + 1, heap, shape.rows.div_ceil(8), large)
    }

    fn discard(
        &mut self,
        account: &MemoryAccount,
        shape: BlockShape,
        large_count: u32,
        hint: BytesHint,
    ) {
        let (offsets, heap, validity, large) = Self::shares(shape, large_count, hint);
        self.offsets.attach(account);
        self.offsets.discard_unless(offsets);
        self.heap.attach(account);
        self.heap.discard_unless(heap);
        self.validity.bits.attach(account);
        self.validity.bits.discard_unless(validity);
        self.large.attach(account);
        self.large.discard_unless(large);
    }

    fn allocate(
        &mut self,
        shape: BlockShape,
        large_count: u32,
        hint: BytesHint,
    ) -> Result<(), BudgetExceeded> {
        let (offsets, heap, validity, large) = Self::shares(shape, large_count, hint);
        self.offsets.try_grow_to(offsets)?;
        self.heap.try_grow_to(heap)?;
        self.validity.bits.try_grow_to(validity)?;
        self.large.try_grow_to(large)?;
        Ok(())
    }

    /// Actual heap capacity retained by this decoded bytes output.
    pub(crate) fn memory(&self) -> u64 {
        (self.offsets.capacity() * size_of::<u32>()
            + self.heap.capacity()
            + self.validity.bits.capacity()
            + self.large.capacity() * size_of::<LargeRef>()) as u64
    }

    /// Each buffer: its name and the bytes it holds.
    #[cfg(test)]
    pub(crate) fn buffers(&self) -> [(&'static str, usize); 4] {
        [
            ("offsets", held(&self.offsets)),
            ("heap", held(&self.heap)),
            ("validity", held(&self.validity.bits)),
            ("large", held(&self.large)),
        ]
    }
}

/// The buffers a [`BlockDecoder`] decodes through, which every block it decodes shares: a
/// zstd block's decompressed body, and the end offsets of a bytes block's present values and
/// of its dictionary's entries.
#[derive(Debug, Default)]
struct Scratch {
    body: BudgetVec<u8>,
    ends: BudgetVec<u32>,
    dict_ends: BudgetVec<u32>,
}

impl Scratch {
    /// The elements each buffer holds for a block of `shape`. A block has a value per row at
    /// most, and a dictionary no more entries than values.
    fn shares(shape: BlockShape) -> (usize, usize, usize) {
        let body = match shape.codec {
            Codec::Zstd => shape.uncompressed_len,
            Codec::Raw => 0,
        };
        let (ends, dict_ends) = match shape.encoding {
            Encoding::Int(_) => (0, 0),
            Encoding::Bytes(BytesEncoding::Dictionary) => (shape.rows, shape.rows),
            Encoding::Bytes(BytesEncoding::Plain | BytesEncoding::Fixed) => (shape.rows, 0),
        };
        (body, ends, dict_ends)
    }

    fn memory(shape: BlockShape) -> u64 {
        let (body, ends, dict_ends) = Self::shares(shape);
        (body + (ends + dict_ends) * size_of::<u32>()) as u64
    }

    /// Clears reusable capacities and retires every obsolete allocation before replacement.
    fn discard(&mut self, account: &MemoryAccount, shape: BlockShape) {
        let (body, ends, dict_ends) = Self::shares(shape);
        self.body.attach(account);
        self.body.discard_unless(body);
        self.ends.attach(account);
        self.ends.discard_unless(ends);
        self.dict_ends.attach(account);
        self.dict_ends.discard_unless(dict_ends);
    }

    /// Actual capacity retained across the scratch buffers.
    fn retained(&self) -> u64 {
        (self.body.capacity()
            + (self.ends.capacity() + self.dict_ends.capacity()) * size_of::<u32>()) as u64
    }

    /// Allocates only the capacities whose old storage has already been discarded.
    fn allocate(&mut self, shape: BlockShape) -> Result<(), BudgetExceeded> {
        let (body, ends, dict_ends) = Self::shares(shape);
        self.body.try_grow_to(body)?;
        self.ends.try_grow_to(ends)?;
        self.dict_ends.try_grow_to(dict_ends)?;
        Ok(())
    }
}

/// Decodes blocks into caller buffers. It owns the decompression context and the buffers
/// a decode needs on the side, so one decoder serves every block of a query without
/// allocating behind its back: what a decode leaves it and the block it fills holding is the
/// block's [`DecodeMemory`].
pub(crate) struct BlockDecoder {
    /// `None` for a decoder of raw blocks only, which needs no zstd context.
    decompressor: Option<Decompressor>,
    scratch: Scratch,
    /// The operation envelope shared by outputs and scratch.
    account: MemoryAccount,
}

impl BlockDecoder {
    /// Isolates a fixture's buffer budget from its independently charged dependency workspace.
    #[cfg(test)]
    pub(crate) fn for_buffers(account: &MemoryAccount) -> Self {
        Self {
            decompressor: Some(compress::new_decompressor(&crate::budget::test_account()).unwrap()),
            scratch: Scratch::default(),
            account: account.clone(),
        }
    }

    /// Creates a decoder with charged dependency workspace and initially empty scratch.
    pub(crate) fn new(account: &MemoryAccount) -> io::Result<Self> {
        Ok(Self {
            decompressor: Some(compress::new_decompressor(account)?),
            scratch: Scratch::default(),
            account: account.clone(),
        })
    }

    /// A decoder of raw blocks only, without a zstd context: for L0 chunks, which are never
    /// compressed (§6.3 rule 2). A compressed block is damage to it
    /// ([`DecodeError::Compressed`]).
    pub(crate) fn raw(account: &MemoryAccount) -> Self {
        Self {
            decompressor: None,
            scratch: Scratch::default(),
            account: account.clone(),
        }
    }

    /// The operation envelope shared by decoder outputs, scratch and fragment readers.
    pub(crate) fn account(&self) -> &MemoryAccount {
        &self.account
    }

    /// Each scratch buffer: its name and the bytes it holds. The zstd context is not among
    /// them.
    #[cfg(test)]
    pub(crate) fn buffers(&self) -> [(&'static str, usize); 3] {
        [
            ("body", held(&self.scratch.body)),
            ("ends", held(&self.scratch.ends)),
            ("dictionary ends", held(&self.scratch.dict_ends)),
        ]
    }

    /// Drops all obsolete output and scratch storage before admitting any replacement.
    pub(crate) fn prepare_ints<'a, T: IntValue>(
        &'a mut self,
        shape: BlockShape,
        out: &'a mut IntBlock<T>,
    ) -> Result<PreparedInts<'a, T>, BudgetExceeded> {
        debug_assert!(check_shape(shape).is_ok());
        self.scratch.discard(&self.account, shape);
        out.discard(&self.account, shape);
        let target = Scratch::memory(shape) + DecodeMemory::ints(shape, T::TYPE).output;
        self.account
            .admit(target - self.scratch.retained() - out.memory())?;
        self.scratch.allocate(shape)?;
        out.allocate(shape)?;
        Ok(PreparedInts {
            decoder: self,
            out,
            shape,
        })
    }

    /// Admits a bytes block after every obsolete buffer has dropped, retaining equal capacities.
    pub(crate) fn prepare_bytes<'a>(
        &'a mut self,
        shape: BlockShape,
        large_count: u32,
        hint: BytesHint,
        out: &'a mut BytesBlock,
    ) -> Result<PreparedBytes<'a>, BudgetExceeded> {
        debug_assert!(check_shape(shape).is_ok());
        self.scratch.discard(&self.account, shape);
        out.discard(&self.account, shape, large_count, hint);
        let target = Scratch::memory(shape) + DecodeMemory::bytes(shape, large_count, hint).output;
        self.account
            .admit(target - self.scratch.retained() - out.memory())?;
        self.scratch.allocate(shape)?;
        out.allocate(shape, large_count, hint)?;
        Ok(PreparedBytes {
            decoder: self,
            out,
            shape,
            large_count,
            hint,
        })
    }

    fn decode_ints_impl<T: IntValue>(
        &mut self,
        stored: &[u8],
        shape: BlockShape,
        out: &mut IntBlock<T>,
    ) -> Result<(), DecodeError> {
        let Encoding::Int(encoding) = shape.encoding else {
            return WrongEncodingSnafu {
                tag: shape.encoding.tag(),
            }
            .fail();
        };
        check_shape(shape)?;
        let body = body(
            &mut self.decompressor,
            &mut self.scratch.body,
            stored,
            shape,
        )?;
        let mut input = Cursor::new(body);
        let flags = read_header(&mut input, shape, FLAG_NULLS)?;
        read_validity(&mut input, flags, shape.rows, &mut out.validity)?;
        int::decode(T::TYPE, encoding, &mut input, shape.rows, |bits| {
            out.values.push(T::from_bits(bits))
        })?;
        ensure_consumed(&input)
    }

    /// Decodes a bytes block of a chunk with `large_count` large values, for a column of
    /// `hint`. The hint is what a damaged block is checked against:
    ///
    /// - only a text column has large values, as the builder writes them;
    /// - every present value of a fixed column has the column's width. The block itself
    ///   stores only the length its values share, and readers take a fixed column's values
    ///   to be of its width.
    fn decode_bytes_impl(
        &mut self,
        stored: &[u8],
        shape: BlockShape,
        large_count: u32,
        hint: BytesHint,
        out: &mut BytesBlock,
    ) -> Result<(), DecodeError> {
        let Encoding::Bytes(encoding) = shape.encoding else {
            return WrongEncodingSnafu {
                tag: shape.encoding.tag(),
            }
            .fail();
        };
        check_shape(shape)?;
        let body = body(
            &mut self.decompressor,
            &mut self.scratch.body,
            stored,
            shape,
        )?;
        let mut input = Cursor::new(body);
        let flags = read_header(&mut input, shape, FLAG_NULLS | FLAG_LARGE)?;
        ensure!(
            flags & FLAG_LARGE == 0 || hint == BytesHint::Text,
            LargeOutsideTextSnafu { hint }
        );
        read_validity(&mut input, flags, shape.rows, &mut out.validity)?;
        let large_bits = if flags & FLAG_LARGE == 0 {
            None
        } else {
            let bits = input
                .take(shape.rows.div_ceil(8))
                .context(TruncatedLargeRefsSnafu)?;
            let mut next = 0;
            for row in (0..shape.rows).filter(|&row| bit(bits, row)) {
                let index = input.u32().context(TruncatedLargeRefsSnafu)?;
                ensure!(
                    index < large_count,
                    LargeRefSnafu {
                        index,
                        count: large_count
                    }
                );
                ensure!(index >= next, LargeRefOrderSnafu { index });
                next = index + 1;
                out.large.push(LargeRef {
                    row: row as u32,
                    index,
                });
            }
            Some(bits)
        };
        let validity = &out.validity;
        let is_present =
            |row: usize| validity.is_valid(row) && !large_bits.is_some_and(|bits| bit(bits, row));
        let present = (0..shape.rows).filter(|&row| is_present(row)).count();
        bytes::decode(
            encoding,
            &mut input,
            present,
            &mut out.heap,
            &mut self.scratch.ends,
            &mut self.scratch.dict_ends,
        )?;
        ensure_consumed(&input)?;
        if let BytesHint::Fixed(width) = hint {
            let mut start = 0;
            // The ends do not decrease: every encoding's decoder checks them.
            for &end in self.scratch.ends.iter() {
                let len = end - start;
                ensure!(len == u32::from(width), ColumnWidthSnafu { len, width });
                start = end;
            }
        }
        out.offsets.push(0);
        let mut ends = self.scratch.ends.iter().copied();
        let mut end = 0;
        for row in 0..shape.rows {
            if is_present(row) {
                end = ends.next().unwrap_or(end);
            }
            out.offsets.push(end);
        }
        Ok(())
    }
}

/// Integer output and scratch whose allocation has already been admitted for one block.
pub(crate) struct PreparedInts<'a, T> {
    /// The decoder whose scratch was admitted with the output.
    decoder: &'a mut BlockDecoder,
    /// The fixed-capacity destination for this prepared integer block.
    out: &'a mut IntBlock<T>,
    shape: BlockShape,
}

impl<T: IntValue> PreparedInts<'_, T> {
    /// Decodes only the shape whose storage was admitted before the fragment was borrowed.
    pub(crate) fn decode(self, stored: &[u8], shape: BlockShape) -> Result<(), DecodeError> {
        assert_eq!(
            self.shape, shape,
            "prepared storage must match the directory block"
        );
        self.decoder.decode_ints_impl(stored, shape, self.out)
    }
}

/// Bytes output and scratch whose allocation has already been admitted for one block.
pub(crate) struct PreparedBytes<'a> {
    /// The decoder whose scratch was admitted with the output.
    decoder: &'a mut BlockDecoder,
    /// The fixed-capacity destination for this prepared bytes block.
    out: &'a mut BytesBlock,
    shape: BlockShape,
    large_count: u32,
    hint: BytesHint,
}

impl PreparedBytes<'_> {
    /// Decodes only the previously admitted shape and column constraints.
    pub(crate) fn decode(self, stored: &[u8], shape: BlockShape) -> Result<(), DecodeError> {
        assert_eq!(
            self.shape, shape,
            "prepared storage must match the directory block"
        );
        self.decoder
            .decode_bytes_impl(stored, shape, self.large_count, self.hint, self.out)
    }
}

/// A raw integer block read in place: valid while the fragment holding it is borrowed.
#[derive(Debug)]
pub(crate) struct IntView<'a, T: IntValue> {
    values: &'a [T::Le],
    validity: Option<&'a [u8]>,
}

impl<'a, T: IntValue> IntView<'a, T> {
    /// Every row's value, little-endian. A null row holds some other value of the block.
    pub(crate) fn values(&self) -> &'a [T::Le] {
        self.values
    }

    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    /// Panics when `row` is past the block, like indexing.
    pub(crate) fn get(&self, row: usize) -> Option<T> {
        let valid = self.validity.is_none_or(|bits| bit(bits, row));
        valid.then(|| T::from_le(&self.values[row]))
    }
}

/// Views a block in place when it is a raw, uncompressed integer block; `None` when it is
/// stored otherwise and has to be decoded.
pub(crate) fn view_ints<T: IntValue>(
    stored: &[u8],
    shape: BlockShape,
) -> Result<Option<IntView<'_, T>>, DecodeError> {
    if shape.codec != Codec::Raw || shape.encoding != Encoding::Int(IntEncoding::Raw) {
        return Ok(None);
    }
    check_shape(shape)?;
    let body = raw_body(stored, shape)?;
    let mut input = Cursor::new(body);
    let flags = read_header(&mut input, shape, FLAG_NULLS)?;
    let validity = if flags & FLAG_NULLS == 0 {
        None
    } else {
        Some(
            input
                .take(shape.rows.div_ceil(8))
                .context(TruncatedValiditySnafu)?,
        )
    };
    let truncated = TruncatedIntsSnafu {
        encoding: IntEncoding::Raw,
    };
    let bytes = input
        .take(shape.rows * T::TYPE.width())
        .context(truncated)?;
    ensure_consumed(&input)?;
    // The values are unaligned little-endian types, so only the length can fail the cast,
    // and it was just taken as a whole number of values.
    let Ok(values) = <[T::Le]>::ref_from_bytes(bytes) else {
        return truncated.fail();
    };
    Ok(Some(IntView { values, validity }))
}

/// Checks the tags and returns the body, decompressing it into `buffer` when needed.
fn body<'a>(
    decompressor: &mut Option<Decompressor>,
    buffer: &'a mut BudgetVec<u8>,
    stored: &'a [u8],
    shape: BlockShape,
) -> Result<&'a [u8], DecodeError> {
    match shape.codec {
        Codec::Raw => raw_body(stored, shape),
        Codec::Zstd => {
            compress::decompress(
                decompressor.as_mut().context(CompressedSnafu)?,
                payload(stored, shape)?,
                shape.uncompressed_len,
                buffer,
            )?;
            Ok(buffer)
        }
    }
}

fn raw_body(stored: &[u8], shape: BlockShape) -> Result<&[u8], DecodeError> {
    let payload = payload(stored, shape)?;
    ensure!(
        payload.len() == shape.uncompressed_len,
        PayloadLengthSnafu {
            expected: shape.uncompressed_len,
            actual: payload.len()
        }
    );
    Ok(payload)
}

fn payload(stored: &[u8], shape: BlockShape) -> Result<&[u8], DecodeError> {
    let ([codec, encoding], payload) = stored.split_first_chunk::<2>().context(MissingTagsSnafu)?;
    ensure!(
        *codec == shape.codec as u8 && *encoding == shape.encoding.tag(),
        TagMismatchSnafu {
            codec: *codec,
            encoding: *encoding
        }
    );
    Ok(payload)
}

/// Checks the rows and the length a shape declares against the format's limits, before
/// anything is sized by them.
fn check_shape(shape: BlockShape) -> Result<(), DecodeError> {
    ensure!(
        shape.rows <= MAX_BLOCK_ROWS,
        TooManyRowsSnafu { rows: shape.rows }
    );
    ensure!(
        shape.uncompressed_len <= MAX_BLOCK_LEN,
        DeclaredLengthSnafu {
            len: shape.uncompressed_len,
            limit: MAX_BLOCK_LEN
        }
    );
    Ok(())
}

fn read_header(input: &mut Cursor<'_>, shape: BlockShape, allowed: u8) -> Result<u8, DecodeError> {
    let flags = input.u8().context(TruncatedHeaderSnafu)?;
    let rows = input.u32().context(TruncatedHeaderSnafu)? as usize;
    ensure!(flags & !allowed == 0, BlockFlagsSnafu { flags });
    ensure!(
        rows == shape.rows,
        RowCountSnafu {
            expected: shape.rows,
            actual: rows
        }
    );
    Ok(flags)
}

fn read_validity(
    input: &mut Cursor<'_>,
    flags: u8,
    rows: usize,
    validity: &mut Validity,
) -> Result<(), DecodeError> {
    validity.bits.clear();
    validity.nulls = flags & FLAG_NULLS != 0;
    if validity.nulls {
        let bits = input
            .take(rows.div_ceil(8))
            .context(TruncatedValiditySnafu)?;
        validity.bits.extend_from_slice(bits);
    }
    Ok(())
}

fn ensure_consumed(input: &Cursor<'_>) -> Result<(), DecodeError> {
    ensure!(
        input.remaining() == 0,
        TrailingBytesSnafu {
            extra: input.remaining()
        }
    );
    Ok(())
}

#[cfg(test)]
impl BlockDecoder {
    /// Test convenience that checks hostile shapes before using the charged preparation API.
    pub(crate) fn decode_ints<T: IntValue>(
        &mut self,
        stored: &[u8],
        shape: BlockShape,
        out: &mut IntBlock<T>,
    ) -> Result<(), DecodeError> {
        check_shape(shape)?;
        self.prepare_ints(shape, out).unwrap().decode(stored, shape)
    }

    /// Test convenience for charged bytes decoding; the format assertions remain unchanged.
    pub(crate) fn decode_bytes(
        &mut self,
        stored: &[u8],
        shape: BlockShape,
        count: u32,
        hint: BytesHint,
        out: &mut BytesBlock,
    ) -> Result<(), DecodeError> {
        check_shape(shape)?;
        self.prepare_bytes(shape, count, hint, out)
            .unwrap()
            .decode(stored, shape)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use strum::VariantArray;

    use super::*;
    use crate::codec::{MAX_INLINE_VALUE_LEN, bytes::BytesEncoding};

    #[test]
    fn decoder_replacements_drop_all_obsolete_storage_before_any_new_allocation() {
        use std::{
            cell::{Cell, RefCell},
            rc::Rc,
        };
        let account = crate::budget::test_account();
        let actual = Rc::new(Cell::new(0u64));
        let events = Rc::new(RefCell::new(Vec::new()));
        let observed = actual.clone();
        let recorded = events.clone();
        account.observe_allocations(move |allocate, bytes, charge| {
            let next = if allocate {
                observed.get() + bytes
            } else {
                observed.get() - bytes
            };
            assert!(next <= charge);
            observed.set(next);
            if bytes != 0 {
                recorded.borrow_mut().push((allocate, bytes));
            }
        });
        let mut decoder = BlockDecoder::raw(&account);
        let mut out = BytesBlock::default();
        let shape = |encoding, rows, uncompressed_len| BlockShape {
            codec: Codec::Zstd,
            encoding: Encoding::Bytes(encoding),
            rows,
            uncompressed_len,
        };
        let first = shape(BytesEncoding::Dictionary, 16, 32);
        decoder
            .prepare_bytes(first, 0, BytesHint::LowCardinality, &mut out)
            .unwrap();
        let old = out.memory() + decoder.scratch.retained();
        assert_eq!(actual.get(), old);
        events.borrow_mut().clear();
        let second = shape(BytesEncoding::Plain, 8, 64);
        decoder
            .prepare_bytes(second, 0, BytesHint::LowCardinality, &mut out)
            .unwrap();
        let events_before_first_allocation: u64 = events
            .borrow()
            .iter()
            .take_while(|(allocate, _)| !allocate)
            .map(|(_, bytes)| bytes)
            .sum();
        assert_eq!(events_before_first_allocation, old);
        assert_eq!(actual.get(), out.memory() + decoder.scratch.retained());
        let pointer = out.heap.as_ptr();
        events.borrow_mut().clear();
        decoder
            .prepare_bytes(second, 0, BytesHint::LowCardinality, &mut out)
            .unwrap();
        assert!(events.borrow().is_empty());
        assert_eq!(out.heap.as_ptr(), pointer);
        drop(out);
        drop(decoder);
        assert_eq!(actual.get(), 0);
        assert_eq!(account.live_bytes(), 0);
    }

    fn stored(encoded: &Encoded<'_>) -> (Vec<u8>, BlockShape, usize) {
        let mut out = crate::budget::test_account()
            .buffer(encoded.stored_len())
            .unwrap();
        encoded.write(&mut out);
        let shape = BlockShape {
            codec: encoded.codec,
            encoding: encoded.encoding,
            uncompressed_len: encoded.uncompressed_len,
            rows: 0,
        };
        (out.to_vec(), shape, encoded.stored_len())
    }

    fn int_block<T: IntValue + 'static>(
        values: Vec<T>,
    ) -> impl Strategy<Value = (Vec<T>, Option<Vec<bool>>)> {
        let len = values.len();
        (
            Just(values),
            prop::option::of(prop::collection::vec(any::<bool>(), len)),
        )
    }

    fn check_ints<T: IntValue>(
        values: &[T],
        validity: Option<&[bool]>,
        compressed: bool,
    ) -> Result<(), TestCaseError> {
        let mut compressor = compress::new_compressor(&crate::budget::test_account()).unwrap();
        let mut scratch = EncodeScratch::new(
            &crate::budget::test_account(),
            crate::codec::block::ScratchCaps::default(),
        );
        let encoded = encode_ints(
            values,
            validity,
            compressed.then_some(&mut compressor),
            &mut scratch,
        )
        .unwrap();
        let (bytes, mut shape, stored_len) = stored(&encoded);
        shape.rows = values.len();
        let raw = values.len() * int_raw_len(T::TYPE, validity.is_some());
        prop_assert!(stored_len <= raw + MAX_BLOCK_OVERHEAD);
        let mut decoder = BlockDecoder::new(&crate::budget::test_account()).unwrap();
        let mut out = IntBlock::default();
        decoder.decode_ints(&bytes, shape, &mut out).unwrap();
        prop_assert_eq!(out.len(), values.len());
        for (row, &value) in values.iter().enumerate() {
            let valid = validity.is_none_or(|validity| validity[row]);
            prop_assert_eq!(out.get(row), valid.then_some(value));
        }
        if let Some(view) = view_ints::<T>(&bytes, shape).unwrap() {
            prop_assert_eq!(view.len(), values.len());
            for row in 0..values.len() {
                prop_assert_eq!(view.get(row), out.get(row));
            }
        }
        Ok(())
    }

    proptest! {
        #[test]
        fn integer_blocks_round_trip_with_nulls(
            (values, validity) in prop::collection::vec(
                prop_oneof![Just(i64::MIN), Just(i64::MAX), -2i64..2, any::<i64>()],
                0..300,
            ).prop_flat_map(int_block),
            compressed in any::<bool>(),
        ) {
            check_ints(&values, validity.as_deref(), compressed)?;
        }

        #[test]
        fn u64_and_u32_blocks_round_trip_with_nulls(
            (wide, wide_validity) in prop::collection::vec(
                prop_oneof![Just(u64::MAX), 0u64..64, any::<u64>()],
                0..300,
            ).prop_flat_map(int_block),
            (narrow, narrow_validity) in prop::collection::vec(
                prop_oneof![Just(u32::MAX), 0u32..4],
                0..300,
            ).prop_flat_map(int_block),
            compressed in any::<bool>(),
        ) {
            check_ints(&wide, wide_validity.as_deref(), compressed)?;
            check_ints(&narrow, narrow_validity.as_deref(), compressed)?;
        }

        #[test]
        fn bytes_blocks_round_trip_with_nulls_and_large_references(
            rows in prop::collection::vec(
                (
                    prop_oneof![
                        Just(Vec::new()),
                        Just(b"DIRECT".to_vec()),
                        prop::collection::vec(any::<u8>(), 0..24),
                    ],
                    // 0: null, 1: large, otherwise inline.
                    0u8..6,
                ),
                0..200,
            ),
            hint in prop_oneof![Just(BytesHint::LowCardinality), Just(BytesHint::Text)],
            compressed in any::<bool>(),
        ) {
            let nullable = rows.iter().any(|&(_, kind)| kind == 0);
            let large: Vec<(u32, u32)> = if hint == BytesHint::Text {
                rows.iter()
                    .enumerate()
                    .filter(|(_, (_, kind))| *kind == 1)
                    .enumerate()
                    .map(|(index, (row, _))| (row as u32, index as u32))
                    .collect()
            } else {
                Vec::new()
            };
            let values: Vec<&[u8]> = rows.iter().map(|(value, _)| value.as_slice()).collect();
            let validity: Option<Vec<bool>> =
                nullable.then(|| rows.iter().map(|&(_, kind)| kind != 0).collect());
            let mut compressor = compress::new_compressor(&crate::budget::test_account()).unwrap();
            let mut scratch = EncodeScratch::new(&crate::budget::test_account(), crate::codec::block::ScratchCaps::default());
            let encoded = encode_bytes(
                &values[..],
                validity.as_deref(),
                &large,
                hint,
                compressed.then_some(&mut compressor),
                &mut scratch,
            )
            .unwrap();
            let (bytes, mut shape, stored_len) = stored(&encoded);
            shape.rows = rows.len();
            let raw: usize = rows
                .iter()
                .map(|(value, kind)| {
                    let inline = *kind != 0 && !(*kind == 1 && hint == BytesHint::Text);
                    bytes_raw_len(if inline { value.len() } else { 0 }, hint, nullable)
                })
                .sum();
            prop_assert!(stored_len <= raw + MAX_BLOCK_OVERHEAD);
            let mut decoder = BlockDecoder::new(&crate::budget::test_account()).unwrap();
            let mut out = BytesBlock::default();
            decoder
                .decode_bytes(&bytes, shape, large.len() as u32, hint, &mut out)
                .unwrap();
            prop_assert_eq!(out.len(), rows.len());
            let mut next_large = large.iter();
            for (row, (value, kind)) in rows.iter().enumerate() {
                let expected = match kind {
                    0 => BytesValue::Null,
                    1 if hint == BytesHint::Text => BytesValue::Large(next_large.next().unwrap().1),
                    _ => BytesValue::Inline(value),
                };
                prop_assert_eq!(out.get(row), expected);
            }
        }

        #[test]
        fn decoding_hostile_bodies_never_panics(
            body in prop::collection::vec(any::<u8>(), 0..96),
            encoding in prop_oneof![
                prop::sample::select(IntEncoding::VARIANTS).prop_map(Encoding::Int),
                prop::sample::select(BytesEncoding::VARIANTS).prop_map(Encoding::Bytes),
            ],
            rows in 0usize..40,
            large_count in 0u32..4,
        ) {
            let mut stored = vec![Codec::Raw as u8, encoding.tag()];
            stored.extend_from_slice(&body);
            let shape = BlockShape { codec: Codec::Raw, encoding, uncompressed_len: body.len(), rows };
            let mut decoder = BlockDecoder::new(&crate::budget::test_account()).unwrap();
            let _ = decoder.decode_ints(&stored, shape, &mut IntBlock::<i64>::default());
            let _ = decoder.decode_ints(&stored, shape, &mut IntBlock::<u32>::default());
            let _ = decoder.decode_bytes(&stored, shape, large_count, BytesHint::Text, &mut BytesBlock::default());
            let _ = view_ints::<u64>(&stored, shape);
        }
    }

    #[test]
    fn nullable_blocks_without_nulls_carry_no_bitmap() {
        let mut scratch = EncodeScratch::new(
            &crate::budget::test_account(),
            crate::codec::block::ScratchCaps::default(),
        );
        let values = [5u64, 6, 7];
        let without = encode_ints(&values, None, None, &mut scratch)
            .unwrap()
            .stored_len();
        let with_flags = encode_ints(&values, Some(&[true; 3]), None, &mut scratch)
            .unwrap()
            .stored_len();
        assert_eq!(without, with_flags);
    }

    #[test]
    fn a_tag_that_disagrees_with_the_directory_is_rejected() {
        let mut scratch = EncodeScratch::new(
            &crate::budget::test_account(),
            crate::codec::block::ScratchCaps::default(),
        );
        let encoded = encode_ints(&[1u64, 2, 3], None, None, &mut scratch).unwrap();
        let (bytes, mut shape, _) = stored(&encoded);
        shape.rows = 3;
        shape.encoding = Encoding::Int(IntEncoding::Raw);
        if encoded.encoding == shape.encoding {
            shape.encoding = Encoding::Int(IntEncoding::Delta);
        }
        let result = BlockDecoder::new(&crate::budget::test_account())
            .unwrap()
            .decode_ints(&bytes, shape, &mut IntBlock::<u64>::default());
        assert!(matches!(result, Err(DecodeError::TagMismatch { .. })));
    }

    #[test]
    fn a_zstd_block_declaring_too_much_is_rejected_before_decompressing() {
        let mut compressor = compress::new_compressor(&crate::budget::test_account()).unwrap();
        let mut frame = crate::budget::test_account()
            .buffer(0)
            .expect("empty storage is free");
        let bomb = vec![0u8; MAX_BLOCK_LEN * 4];
        assert_eq!(
            compress::compress(
                &mut compressor,
                &bomb,
                &mut frame,
                compress::frame_len(bomb.len()),
            )
            .unwrap(),
            Codec::Zstd
        );
        let mut stored = vec![Codec::Zstd as u8, Encoding::Int(IntEncoding::Raw).tag()];
        stored.extend_from_slice(&frame);
        let shape = BlockShape {
            codec: Codec::Zstd,
            encoding: Encoding::Int(IntEncoding::Raw),
            uncompressed_len: bomb.len(),
            rows: 1,
        };
        let mut decoder = BlockDecoder::new(&crate::budget::test_account()).unwrap();
        let result = decoder.decode_ints(&stored, shape, &mut IntBlock::<u64>::default());
        assert!(matches!(result, Err(DecodeError::DeclaredLength { .. })));
        assert_eq!(decoder.scratch.body.capacity(), 0);
    }

    #[test]
    fn a_large_reference_past_the_directory_is_rejected() {
        let long = vec![b'x'; MAX_INLINE_VALUE_LEN + 1];
        let values: [&[u8]; 2] = [b"short", &long];
        let mut scratch = EncodeScratch::new(
            &crate::budget::test_account(),
            crate::codec::block::ScratchCaps::default(),
        );
        let encoded = encode_bytes(
            &values[..],
            None,
            &[(1, 3)],
            BytesHint::Text,
            None,
            &mut scratch,
        )
        .unwrap();
        let (bytes, mut shape, _) = stored(&encoded);
        shape.rows = 2;
        let mut decoder = BlockDecoder::new(&crate::budget::test_account()).unwrap();
        let mut out = BytesBlock::default();
        decoder
            .decode_bytes(&bytes, shape, 4, BytesHint::Text, &mut out)
            .unwrap();
        assert_eq!(out.get(1), BytesValue::Large(3));
        let result = decoder.decode_bytes(&bytes, shape, 3, BytesHint::Text, &mut out);
        assert!(matches!(
            result,
            Err(DecodeError::LargeRef { index: 3, count: 3 })
        ));
    }
}
