//! Dictionary encoding for low-cardinality values: a dictionary local to the block, plus
//! bit-packed codes (§6.2).
//!
//! Payload for `k` values with `d` distinct ones:
//!
//! - `d`, `u32`;
//! - the end offsets of the entries, as a nested `u32` integer column;
//! - the entries back to back, in code order, which is the entries' byte order;
//! - the code width `w`, `u8`, enough for `d - 1`;
//! - the `k` codes, `w` bits each.
//!
//! Decoding expands codes into values, so the output can be larger than the block. It is
//! bounded by [`MAX_BLOCK_LEN`]: a valid block's values never exceed it before encoding.
//!
//! The encoder finds the distinct values by sorting the present values' positions by value,
//! in one buffer of a `u32` per value that it reuses from block to block, instead of a hash
//! map allocated per block. The decoder does not depend on the entries' order. Sorting
//! compares values that come from outside (hosts, log types) in `O(n log n)` whatever they
//! are, so it needs no seeded hash either (§0.3 rule 3).

use snafu::{OptionExt, ensure};

use super::{
    BitWidthSnafu, DecodeError, DictionaryCodeSnafu, DictionaryLengthSnafu, HeapLimitSnafu,
    MAX_BLOCK_LEN, TruncatedHeapSnafu, bitpack,
    bytes::{ByteValues, BytesScratch, decode_offsets, running_ends},
    cursor::Cursor,
    int::{self, IntSizes, IntType},
};

/// Sorts the present values into `scratch.order` and returns the payload size.
pub(super) fn measure(
    values: &(impl ByteValues + ?Sized),
    present: &[u32],
    scratch: &mut BytesScratch,
) -> usize {
    let order = &mut scratch.order;
    order.clear();
    order.reserve(present.len(), scratch.max_order);
    order.extend(0..present.len() as u32);
    let value = |position: u32| values.value(present[position as usize] as usize);
    // Unstable, so that it sorts in place; equal values have one entry whatever their order.
    order.sort_unstable_by(|&a, &b| value(a).cmp(value(b)));
    let (count, len) = entries(values, present, order)
        .fold((0, 0), |(count, len), entry| (count + 1, len + entry.len()));
    let nested = IntSizes::measure(
        IntType::U32,
        running_ends(entries(values, present, order).map(<[u8]>::len)),
    )
    .smallest()
    .1;
    4 + 1 + nested + len + 1 + bitpack::packed_len(present.len(), code_width(count))
}

/// The dictionary's entries: the first of each run of equal values in `order`, so the
/// distinct present values in byte order.
fn entries<'a, V: ByteValues + ?Sized>(
    values: &'a V,
    present: &'a [u32],
    order: &'a [u32],
) -> impl Iterator<Item = &'a [u8]> + Clone + 'a {
    let value = move |position: u32| values.value(present[position as usize] as usize);
    order
        .iter()
        .enumerate()
        .filter(move |&(index, &position)| index == 0 || value(order[index - 1]) != value(position))
        .map(move |(_, &position)| value(position))
}

/// Appends the payload that [`measure`] prepared in `scratch`. The codes take the place of
/// the row indices in `present`, which the payload needs no more by then.
pub(super) fn encode(
    values: &(impl ByteValues + ?Sized),
    present: &mut [u32],
    scratch: &BytesScratch,
    out: &mut crate::budget::BudgetVec<u8>,
) {
    let order = &scratch.order;
    let count = entries(values, present, order).count();
    out.extend_from_slice(&(count as u32).to_le_bytes());
    let ends = || running_ends(entries(values, present, order).map(<[u8]>::len));
    let (nested, _) = IntSizes::measure(IntType::U32, ends()).smallest();
    out.push(nested as u8);
    int::encode(IntType::U32, nested, ends(), out);
    for entry in entries(values, present, order) {
        out.extend_from_slice(entry);
    }
    let width = code_width(count);
    out.push(width);
    // A value's code is its entry's rank in `order`.
    let mut code = 0u32;
    let mut previous: Option<&[u8]> = None;
    for &position in order.iter() {
        let value = values.value(present[position as usize] as usize);
        if previous.is_some_and(|previous| previous != value) {
            code += 1;
        }
        previous = Some(value);
        present[position as usize] = code;
    }
    bitpack::pack(present.iter().map(|&code| u64::from(code)), width, out);
}

pub(super) fn decode(
    input: &mut Cursor<'_>,
    count: usize,
    heap: &mut crate::budget::BudgetVec<u8>,
    ends: &mut crate::budget::BudgetVec<u32>,
    dict_ends: &mut crate::budget::BudgetVec<u32>,
) -> Result<(), DecodeError> {
    let entries = input.u32().context(TruncatedHeapSnafu)?;
    // Every entry is a distinct value of the block, so there are no more entries than values.
    // That bounds the entries' offsets by the block's rows, before they are allocated.
    ensure!(
        entries as usize <= count,
        DictionaryLengthSnafu {
            len: entries,
            values: count
        }
    );
    decode_offsets(input, entries as usize, dict_ends)?;
    let dictionary = input
        .take(dict_ends.last().copied().unwrap_or(0) as usize)
        .context(TruncatedHeapSnafu)?;
    let width = input.u8().context(TruncatedHeapSnafu)?;
    ensure!(width <= 64, BitWidthSnafu { width });
    let codes = input
        .take(bitpack::packed_len(count, width))
        .context(TruncatedHeapSnafu)?;
    for code in bitpack::unpack(codes, width, count) {
        ensure!(
            code < u64::from(entries),
            DictionaryCodeSnafu { code, len: entries }
        );
        let code = code as usize;
        let start = if code == 0 { 0 } else { dict_ends[code - 1] };
        let value = &dictionary[start as usize..dict_ends[code] as usize];
        ensure!(
            heap.len() + value.len() <= MAX_BLOCK_LEN,
            HeapLimitSnafu {
                limit: MAX_BLOCK_LEN
            }
        );
        heap.extend_from_slice(value);
        ends.push(heap.len() as u32);
    }
    Ok(())
}

/// The bits a code needs: none for one entry or none.
fn code_width(entries: usize) -> u8 {
    bitpack::width(entries.saturating_sub(1) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// However the values interleave, each distinct one has a single entry, the entries are
    /// in byte order, and each code is its entry's rank.
    #[test]
    fn every_distinct_value_has_one_entry_in_byte_order() {
        let values: [&[u8]; 6] = [b"b", b"a", b"c", b"a", b"b", b"a"];
        let present: Vec<u32> = (0..6).collect();
        let mut scratch =
            BytesScratch::new(&crate::budget::test_account(), crate::codec::MAX_BLOCK_ROWS);
        let size = measure(&values[..], &present, &mut scratch);
        let mut out = crate::budget::test_account()
            .buffer(MAX_BLOCK_LEN + 1024)
            .unwrap();
        encode(&values[..], &mut present.clone(), &scratch, &mut out);
        assert_eq!(out.len(), size);
        assert_eq!(out[..4], 3u32.to_le_bytes());
        assert!(
            out.windows(3).any(|entries| entries == b"abc"),
            "{out:02x?}"
        );
        let (mut heap, mut ends, mut dict_ends) = (
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_LEN)
                .unwrap(),
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_ROWS)
                .unwrap(),
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_ROWS)
                .unwrap(),
        );
        let mut input = Cursor::new(&out);
        decode(&mut input, 6, &mut heap, &mut ends, &mut dict_ends).unwrap();
        assert_eq!(input.remaining(), 0);
        assert_eq!(&*heap, b"bacaba");
        assert_eq!(&*dict_ends, [1, 2, 3]);
    }

    /// A one-entry dictionary of a 16 KiB value referenced by many codes would expand to
    /// far more than the block. The decoder stops at the limit instead of allocating it.
    #[test]
    fn expansion_past_the_block_limit_is_rejected() {
        let value = vec![b'x'; 16 * 1024];
        let values: Vec<&[u8]> = vec![&value; 9];
        let present: Vec<u32> = (0..9).collect();
        let mut scratch =
            BytesScratch::new(&crate::budget::test_account(), crate::codec::MAX_BLOCK_ROWS);
        measure(&values[..], &present, &mut scratch);
        let mut out = crate::budget::test_account()
            .buffer(MAX_BLOCK_LEN + 1024)
            .unwrap();
        encode(&values[..], &mut present.clone(), &scratch, &mut out);
        // The payload is one entry plus nine zero-width codes.
        assert!(out.len() < value.len() + 16);
        let (mut heap, mut ends, mut dict_ends) = (
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_LEN)
                .unwrap(),
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_ROWS)
                .unwrap(),
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_ROWS)
                .unwrap(),
        );
        let result = decode(
            &mut Cursor::new(&out),
            9,
            &mut heap,
            &mut ends,
            &mut dict_ends,
        );
        assert!(matches!(result, Err(DecodeError::HeapLimit { .. })));
        assert!(heap.len() <= MAX_BLOCK_LEN);
    }

    #[test]
    fn a_code_past_the_dictionary_is_rejected() {
        // One entry, "a", and two one-bit codes: 0, then 1, which does not exist.
        let mut hostile = 1u32.to_le_bytes().to_vec();
        hostile.push(int::IntEncoding::Raw as u8);
        hostile.extend_from_slice(&1u32.to_le_bytes());
        hostile.push(b'a');
        hostile.push(1);
        hostile.push(0b10);
        let (mut heap, mut ends, mut dict_ends) = (
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_LEN)
                .unwrap(),
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_ROWS)
                .unwrap(),
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_ROWS)
                .unwrap(),
        );
        let result = decode(
            &mut Cursor::new(&hostile),
            2,
            &mut heap,
            &mut ends,
            &mut dict_ends,
        );
        assert!(matches!(
            result,
            Err(DecodeError::DictionaryCode { code: 1, len: 1 })
        ));
    }
}
