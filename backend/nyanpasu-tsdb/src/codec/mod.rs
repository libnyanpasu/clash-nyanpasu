//! Column encodings (§6.2) and compression (§6.3).
//!
//! - Everything is scalar integer arithmetic. A SIMD fast path is allowed only behind
//!   runtime detection (§0.3 rule 3); there is none yet.
//! - Every stored column block starts with its codec and encoding tags, outside any
//!   compression, so old and new codecs can coexist (§6.1). [`block`] has the layout.
//! - Decoders never panic on hostile bytes, and never allocate in proportion to a length
//!   they have not checked against the limits below.
//!
//! The limits are constants of the format version, not profile values: a profile's budgets
//! decide admission, and a smaller profile must never make a valid block look corrupted
//! (§6.5).

// The write and query paths that use the chunk format arrive in later cards; until then
// only the chunk layer and the tests call into this module.
#![allow(
    dead_code,
    reason = "used by the chunk layer, whose own callers come later"
)]

pub(crate) mod bitpack;
pub(crate) mod block;
pub(crate) mod bytes;
pub(crate) mod checksum;
pub(crate) mod compress;
pub(crate) mod cursor;
pub(crate) mod dict;
pub(crate) mod int;
pub(crate) mod varint;

use snafu::Snafu;
use strum::FromRepr;

use self::{
    bytes::{BytesEncoding, BytesHint},
    int::IntEncoding,
};
use crate::ErrorKind;

/// The largest block body before compression, and the largest heap a decoded bytes block
/// may expand to. §6.3 rule 2 caps a column block at 128 KiB uncompressed; the builder aims
/// at a quarter of that, so a valid block stays far below it.
pub(crate) const MAX_BLOCK_LEN: usize = 128 * 1024;

/// Every row costs at least four raw bytes (a `u32`, or a bytes offset), so a block within
/// [`MAX_BLOCK_LEN`] has at most this many rows. Decoders size their output by the row
/// count, which is therefore bounded before they allocate.
pub(crate) const MAX_BLOCK_ROWS: usize = MAX_BLOCK_LEN / 4;

/// The longest value a block holds inline (§6.5). A longer text value becomes a large value
/// of the chunk; other bytes columns cannot hold one.
pub(crate) const MAX_INLINE_VALUE_LEN: usize = 16 * 1024;

/// How a block's payload is stored. The discriminant is the tag stored in the block, outside
/// any compression: a new codec gets a new tag, a variant is never renumbered, and blocks
/// with old tags stay readable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub(crate) enum Codec {
    Raw = 0,
    Zstd = 1,
}

/// How a block's values are encoded. Integer and bytes encodings have disjoint tags, so a
/// tag alone tells which kind of column a block can belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Encoding {
    Int(IntEncoding),
    Bytes(BytesEncoding),
}

impl Encoding {
    pub(crate) const fn tag(self) -> u8 {
        match self {
            Self::Int(encoding) => encoding as u8,
            Self::Bytes(encoding) => encoding as u8,
        }
    }

    pub(crate) fn from_tag(tag: u8) -> Option<Self> {
        IntEncoding::from_repr(tag)
            .map(Self::Int)
            .or_else(|| BytesEncoding::from_repr(tag).map(Self::Bytes))
    }
}

/// Why a stored block failed to decode. Every variant means the stored bytes are damaged.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum DecodeError {
    #[snafu(display("stored block is too short for its tags"))]
    MissingTags,

    #[snafu(display("stored block tags {codec}/{encoding} differ from the directory"))]
    TagMismatch { codec: u8, encoding: u8 },

    #[snafu(display("encoding {tag} does not apply to this kind of column"))]
    WrongEncoding { tag: u8 },

    #[snafu(display("declared uncompressed length {len} exceeds {limit}"))]
    DeclaredLength { len: usize, limit: usize },

    #[snafu(display("raw payload has {actual} bytes, not {expected}"))]
    PayloadLength { expected: usize, actual: usize },

    #[snafu(display("failed to decompress the block: {source}"))]
    Decompress { source: std::io::Error },

    /// A decoder of raw blocks met a compressed one: the block belongs to a chunk that is
    /// never compressed.
    #[snafu(display("the block is compressed, where only raw blocks are stored"))]
    Compressed,

    #[snafu(display("block decompressed to {actual} bytes, not {expected}"))]
    DecompressedLength { expected: usize, actual: usize },

    #[snafu(display("block of {rows} rows exceeds {MAX_BLOCK_ROWS}"))]
    TooManyRows { rows: usize },

    #[snafu(display("block header is truncated"))]
    TruncatedHeader,

    #[snafu(display("unknown block flags {flags:#x}"))]
    BlockFlags { flags: u8 },

    #[snafu(display("block holds {actual} rows, not {expected}"))]
    RowCount { expected: usize, actual: usize },

    #[snafu(display("validity bitmap is truncated"))]
    TruncatedValidity,

    #[snafu(display("large-value bitmap or references are truncated"))]
    TruncatedLargeRefs,

    #[snafu(display("large-value reference {index} is not below {count}"))]
    LargeRef { index: u32, count: u32 },

    #[snafu(display("large-value reference {index} does not follow the one before it"))]
    LargeRefOrder { index: u32 },

    #[snafu(display("a block of a {hint:?} column refers to large values, which only text has"))]
    LargeOutsideText { hint: BytesHint },

    #[snafu(display("{encoding:?} integers are truncated"))]
    TruncatedInts { encoding: IntEncoding },

    #[snafu(display("malformed varint in {encoding:?} integers"))]
    Varint { encoding: IntEncoding },

    #[snafu(display("bit width {width} exceeds 64"))]
    BitWidth { width: u8 },

    #[snafu(display("value {value} does not fit a u32 column"))]
    U32Range { value: u64 },

    #[snafu(display("unknown encoding {tag} of a nested offset column"))]
    NestedEncoding { tag: u8 },

    #[snafu(display("bytes offsets decrease"))]
    Offsets,

    #[snafu(display("bytes heap is truncated"))]
    TruncatedHeap,

    #[snafu(display("fixed width {width} exceeds {MAX_INLINE_VALUE_LEN}"))]
    FixedWidth { width: u32 },

    #[snafu(display("a value of {len} bytes in a column of fixed width {width}"))]
    ColumnWidth { len: u32, width: u16 },

    #[snafu(display("dictionary of {len} entries for {values} values"))]
    DictionaryLength { len: u32, values: usize },

    #[snafu(display("dictionary code {code} is not below {len}"))]
    DictionaryCode { code: u64, len: u32 },

    #[snafu(display("decoded values exceed {limit} bytes"))]
    HeapLimit { limit: usize },

    #[snafu(display("{extra} bytes left after the block"))]
    TrailingBytes { extra: usize },
}

impl DecodeError {
    /// A block that fails to decode is damaged, whatever the step (§6.5). zstd failures are
    /// included: a valid frame never fails to decompress into its declared length.
    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Corrupted
    }
}

#[cfg(test)]
mod tests {
    use strum::VariantArray;

    use super::*;

    // The tags are stored in every block header, outside any compression.
    #[test]
    fn the_codec_tags_are_pinned() {
        assert_eq!(Codec::Raw as u8, 0);
        assert_eq!(Codec::Zstd as u8, 1);
        assert_eq!(Codec::from_repr(0), Some(Codec::Raw));
        assert_eq!(Codec::from_repr(1), Some(Codec::Zstd));
        assert_eq!(Codec::from_repr(2), None);
    }

    /// A tag alone tells which kind of column a block belongs to: the two kinds of
    /// encoding share no tag, and every tag round trips to its own kind.
    #[test]
    fn an_encoding_tag_names_its_kind() {
        for &encoding in IntEncoding::VARIANTS {
            let tag = Encoding::Int(encoding).tag();
            assert_eq!(tag, encoding as u8);
            assert_eq!(Encoding::from_tag(tag), Some(Encoding::Int(encoding)));
        }
        for &encoding in BytesEncoding::VARIANTS {
            let tag = Encoding::Bytes(encoding).tag();
            assert_eq!(tag, encoding as u8);
            assert_eq!(Encoding::from_tag(tag), Some(Encoding::Bytes(encoding)));
        }
        let known = IntEncoding::VARIANTS.len() + BytesEncoding::VARIANTS.len();
        let decoded = (0..=u8::MAX).filter_map(Encoding::from_tag).count();
        assert_eq!(decoded, known);
    }
}
