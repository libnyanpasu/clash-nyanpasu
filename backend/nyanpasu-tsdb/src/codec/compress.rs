//! zstd at level 1, one column block at a time, with contexts owned by the caller (§6.3
//! rules 3–5).
//!
//! - A block under [`MIN_COMPRESSED_INPUT`] bytes stays uncompressed.
//! - The compressed form is kept only when its length, frame header included, is at most
//!   0.9 × the encoded length.
//! - The declared uncompressed length is checked against [`MAX_BLOCK_LEN`] before
//!   decompressing, and the block decompresses into a buffer of exactly that length: zstd
//!   itself says a frame's content size cannot be trusted (§6.5).
//!
//! The writer holds one compressor and each query's scratch one decompressor; there is no
//! thread-local or global context.

use std::io;

use crate::budget::{BudgetVec, MemoryAccount, MemoryPlan, Reserved};
use snafu::{ResultExt, ensure};

/// A compressor whose lazy dependency workspace stays charged until destruction.
pub(crate) type Compressor = Reserved<zstd::bulk::Compressor<'static>>;
/// A decompressor retaining its dependency workspace charge across blocks.
pub(crate) type Decompressor = Reserved<zstd::bulk::Decompressor<'static>>;

/// The lazy level-one compression workspace for supported block sizes. zstd 1.5.7's
/// `ZSTD_sizeof_CCtx` peaks at 218,110 bytes over supported blocks, near 16 KiB; round up.
/// A reused context retains the largest workspace, and a test measures both fresh and reused.
pub(crate) const COMPRESSION_CONTEXT: u64 = 224 * 1024;
/// The decompression context shared by event scans and maintenance: zstd 1.5.7 reports
/// 95,984 bytes, rounded up. Bulk decoding uses caller storage and allocates no window.
pub(crate) const DECOMPRESSION_CONTEXT: u64 = 100 * 1024;
/// The existing series-query context envelope, retained for admission compatibility.
pub(crate) const SERIES_DECOMPRESSION_CONTEXT: u64 = 128 * 1024;

use super::{
    Codec, DeclaredLengthSnafu, DecodeError, DecompressSnafu, DecompressedLengthSnafu,
    MAX_BLOCK_LEN,
};

/// ClickHouse, Parquet and InfluxDB 3 default to level 1 too; level 19 would need about
/// 18 MiB of context per megabyte, which no device budget has (§6.3 rule 4).
pub(crate) const ZSTD_LEVEL: i32 = 1;

/// Below this, zstd's frame overhead outweighs anything it can find (VictoriaMetrics' rule,
/// §6.3 rule 3).
pub(crate) const MIN_COMPRESSED_INPUT: usize = 128;

/// Creates a compressor only after its account covers construction and lazy first use.
pub(crate) fn new_compressor(account: &MemoryAccount) -> io::Result<Compressor> {
    account
        .value("dependency", MemoryPlan::bytes(COMPRESSION_CONTEXT), || {
            zstd::bulk::Compressor::new(ZSTD_LEVEL)
        })
        .expect("the caller admitted the compressor allowance")
        .transpose()
}

/// Creates a decompressor only after its account covers the dependency context.
pub(crate) fn new_decompressor(account: &MemoryAccount) -> io::Result<Decompressor> {
    account
        .value(
            "dependency",
            MemoryPlan::bytes(DECOMPRESSION_CONTEXT),
            zstd::bulk::Decompressor::new,
        )
        .expect("the caller admitted the decompressor allowance")
        .transpose()
}

/// Whether a compressed form of `frame_len` bytes saves enough over `encoded_len`: at least
/// 10 %, or the reader would pay for decompression for nothing (§6.3 rule 3).
pub(crate) fn keeps_compressed(encoded_len: usize, frame_len: usize) -> bool {
    frame_len * 10 <= encoded_len * 9
}

/// Compresses `encoded` into `frame` when §6.3 says it pays, and returns the codec to store
/// it with. With [`Codec::Raw`], `frame` holds nothing useful. `frame` gets all of
/// `max_frame` when first used, which has to hold [`frame_len`] of the longest block compressed
/// into it.
pub(crate) fn compress(
    compressor: &mut Compressor,
    encoded: &[u8],
    frame: &mut BudgetVec<u8>,
    max_frame: usize,
) -> io::Result<Codec> {
    if encoded.len() < MIN_COMPRESSED_INPUT {
        return Ok(Codec::Raw);
    }
    frame.clear();
    // zstd writes into the spare capacity, never past it.
    frame.reserve(frame_len(encoded.len()), max_frame);
    let len = frame.compress_into(compressor, encoded)?;
    frame.truncate(len);
    Ok(if keeps_compressed(encoded.len(), len) {
        Codec::Zstd
    } else {
        Codec::Raw
    })
}

/// The room zstd needs to compress `len` bytes whatever they are: zstd's `ZSTD_compressBound`.
pub(crate) fn frame_len(len: usize) -> usize {
    zstd::compress_bound(len)
}

/// Decompresses `frame` into admitted storage that covers `len`, then checks the exact length.
/// A retained larger buffer still bounds zstd's writes by its charged capacity, at most
/// `MAX_BLOCK_LEN`; a frame that expands past its declared length is rejected afterwards.
pub(crate) fn decompress(
    decompressor: &mut Decompressor,
    frame: &[u8],
    len: usize,
    out: &mut BudgetVec<u8>,
) -> Result<(), DecodeError> {
    ensure!(
        len <= MAX_BLOCK_LEN,
        DeclaredLengthSnafu {
            len,
            limit: MAX_BLOCK_LEN
        }
    );
    out.clear();
    assert!(
        (len..=MAX_BLOCK_LEN).contains(&out.capacity()),
        "decompression uses admitted capacity within the block limit"
    );
    let written = out
        .decompress_into(decompressor, frame)
        .context(DecompressSnafu)?;
    ensure!(
        written == len,
        DecompressedLengthSnafu {
            expected: len,
            actual: written
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bytes zstd cannot shrink, from a fixed xorshift sequence.
    fn noise(len: usize) -> Vec<u8> {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    }

    #[test]
    fn the_ten_percent_rule_is_exact() {
        assert!(keeps_compressed(1000, 900));
        assert!(!keeps_compressed(1000, 901));
        assert!(keeps_compressed(10, 9));
        assert!(!keeps_compressed(10, 10));
    }

    #[test]
    fn small_blocks_stay_uncompressed() {
        let mut compressor = new_compressor(&crate::budget::test_account()).unwrap();
        let mut frame = crate::budget::test_account()
            .buffer(0)
            .expect("empty storage is free");
        let zeros = [0u8; MIN_COMPRESSED_INPUT];
        assert_eq!(
            compress(
                &mut compressor,
                &zeros[..MIN_COMPRESSED_INPUT - 1],
                &mut frame,
                frame_len(MAX_BLOCK_LEN + 1),
            )
            .unwrap(),
            Codec::Raw
        );
        // The same bytes at the threshold compress well, so the threshold is what kept the
        // shorter block raw.
        assert_eq!(
            compress(
                &mut compressor,
                &zeros,
                &mut frame,
                frame_len(MAX_BLOCK_LEN + 1)
            )
            .unwrap(),
            Codec::Zstd
        );
    }

    #[test]
    fn random_bytes_stay_raw_and_redundant_bytes_compress() {
        let mut compressor = new_compressor(&crate::budget::test_account()).unwrap();
        let mut frame = crate::budget::test_account()
            .buffer(0)
            .expect("empty storage is free");
        assert_eq!(
            compress(
                &mut compressor,
                &noise(4096),
                &mut frame,
                frame_len(MAX_BLOCK_LEN + 1)
            )
            .unwrap(),
            Codec::Raw
        );
        let text = b"GET /index.html 200 ".repeat(200);
        assert_eq!(
            compress(
                &mut compressor,
                &text,
                &mut frame,
                frame_len(MAX_BLOCK_LEN + 1)
            )
            .unwrap(),
            Codec::Zstd
        );
        assert!(keeps_compressed(text.len(), frame.len()));
    }

    #[test]
    fn one_context_pair_serves_many_blocks() {
        let mut compressor = new_compressor(&crate::budget::test_account()).unwrap();
        let mut decompressor = new_decompressor(&crate::budget::test_account()).unwrap();
        let (mut frame, mut out) = (
            crate::budget::test_account()
                .buffer(0)
                .expect("empty storage is free"),
            crate::budget::test_account()
                .buffer(0)
                .expect("empty storage is free"),
        );
        for round in 0..64usize {
            // Sizes and contents vary, so leftover state from the previous block would show.
            let block: Vec<u8> = format!("block {round} ")
                .into_bytes()
                .into_iter()
                .cycle()
                .take(200 + round * 997)
                .collect();
            assert_eq!(
                compress(
                    &mut compressor,
                    &block,
                    &mut frame,
                    frame_len(MAX_BLOCK_LEN + 1)
                )
                .unwrap(),
                Codec::Zstd
            );
            out.reset_to(block.len()).unwrap();
            decompress(&mut decompressor, &frame, block.len(), &mut out).unwrap();
            assert_eq!(&*out, block);
        }
    }

    #[test]
    fn a_declared_length_past_the_limit_is_rejected_before_decompressing() {
        let mut compressor = new_compressor(&crate::budget::test_account()).unwrap();
        let mut decompressor = new_decompressor(&crate::budget::test_account()).unwrap();
        let bomb = vec![0u8; MAX_BLOCK_LEN + 1];
        let mut frame = crate::budget::test_account()
            .buffer(0)
            .expect("empty storage is free");
        compress(
            &mut compressor,
            &bomb,
            &mut frame,
            frame_len(MAX_BLOCK_LEN + 1),
        )
        .unwrap();
        let mut out = crate::budget::test_account()
            .buffer(0)
            .expect("empty storage is free");
        let result = decompress(&mut decompressor, &frame, bomb.len(), &mut out);
        assert!(matches!(result, Err(DecodeError::DeclaredLength { .. })));
        // Nothing was decompressed or allocated for it.
        assert_eq!(out.capacity(), 0);
    }

    #[test]
    fn a_block_of_another_length_is_rejected() {
        let mut compressor = new_compressor(&crate::budget::test_account()).unwrap();
        let mut decompressor = new_decompressor(&crate::budget::test_account()).unwrap();
        let block = vec![7u8; 1000];
        let mut frame = crate::budget::test_account()
            .buffer(0)
            .expect("empty storage is free");
        compress(
            &mut compressor,
            &block,
            &mut frame,
            frame_len(MAX_BLOCK_LEN + 1),
        )
        .unwrap();
        let mut out = crate::budget::test_account()
            .buffer(0)
            .expect("empty storage is free");
        out.reset_to(999).unwrap();
        // Longer than declared: zstd refuses to write past the buffer.
        assert!(matches!(
            decompress(&mut decompressor, &frame, 999, &mut out),
            Err(DecodeError::Decompress { .. })
        ));
        out.reset_to(1001).unwrap();
        // Shorter than declared: zstd succeeds, and the length check rejects it.
        assert!(matches!(
            decompress(&mut decompressor, &frame, 1001, &mut out),
            Err(DecodeError::DecompressedLength {
                expected: 1001,
                actual: 1000
            })
        ));
        // A previously larger block left spare capacity. Length validation still rejects
        // a frame larger than declared, even when it fits the retained allocation.
        assert!(matches!(
            decompress(&mut decompressor, &frame, 999, &mut out),
            Err(DecodeError::DecompressedLength {
                expected: 999,
                actual: 1000
            })
        ));
    }
}
