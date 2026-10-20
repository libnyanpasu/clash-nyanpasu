//! Chunk boundaries (§4.3): which rows go in which chunk, so that every chunk stays within
//! M rows, B bytes, and a fragment 0 that fits one fragment.
//!
//! The caller costs each row with [`int_cost`] and [`bytes_cost`], the same raw sizes the
//! builder splits blocks by. Planning by raw size is safe because encoding expands raw data
//! by a bounded amount only:
//!
//! - a stored block is at most its rows' raw size plus
//!   [`MAX_BLOCK_OVERHEAD`] (8 bytes);
//! - placing it in a data fragment adds at most 7 bytes of alignment and, because every
//!   data fragment holds a block, at most one 4-byte trailer;
//! - a large value of `n` bytes is stored as `n` bytes plus a trailer per fragment;
//! - a column of raw size `r` has at most `1 + r / MIN_BLOCK_FILL` blocks.
//!
//! A chunk of `C` columns whose rows have inline raw size `R`, large-value size `L` and `V`
//! large values therefore takes at most
//! `R + L + 19·(C + R / MIN_BLOCK_FILL) + max_fragment0_len(C, C + R / MIN_BLOCK_FILL, V)`
//! bytes, and the planner keeps that within B. The tests build planned chunks for real and
//! measure them, so the bound is checked by more than itself.

use std::{
    iter::Sum,
    ops::{Add, Range},
};

use snafu::ensure;

use super::{
    error::{PlanColumnsSnafu, PlanError, RowTooLargeSnafu},
    format::{
        MAX_BLOCKS, MAX_CHUNK_LEN, MAX_COLUMNS, MAX_LARGE_VALUES, MAX_ROWS, MIN_BLOCK_FILL,
        large_stored_len, max_fragment0_len,
    },
};
use crate::{
    codec::{
        MAX_INLINE_VALUE_LEN,
        block::{MAX_BLOCK_OVERHEAD, bytes_raw_len, int_raw_len},
        bytes::BytesHint,
        checksum::TRAILER_LEN,
        int::IntType,
    },
    storage::MAX_FRAGMENT_LEN,
};

/// What placing one block in a data fragment can add beyond its rows' raw size: the block
/// overhead, alignment padding, and a share of one trailer.
const BLOCK_PLACEMENT_OVERHEAD: u64 = (MAX_BLOCK_OVERHEAD + 7 + TRAILER_LEN) as u64;

/// What a row adds to a chunk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RowCost {
    /// The raw size of the row's values inside column blocks.
    pub(crate) inline: u64,
    /// The stored size of the row's large values, trailers included.
    pub(crate) large: u64,
    pub(crate) large_values: u32,
}

impl Add for RowCost {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            inline: self.inline + other.inline,
            large: self.large + other.large,
            large_values: self.large_values + other.large_values,
        }
    }
}

impl Sum for RowCost {
    fn sum<I: Iterator<Item = Self>>(costs: I) -> Self {
        costs.fold(Self::default(), Add::add)
    }
}

/// The cost of an integer value; a null value costs the same.
pub(crate) fn int_cost(ty: IntType, nullable: bool) -> RowCost {
    RowCost {
        inline: int_raw_len(ty, nullable) as u64,
        ..RowCost::default()
    }
}

/// The cost of a bytes value of `len` bytes; pass 0 for a null value.
pub(crate) fn bytes_cost(len: usize, hint: BytesHint, nullable: bool) -> RowCost {
    if hint == BytesHint::Text && len > MAX_INLINE_VALUE_LEN {
        RowCost {
            inline: bytes_raw_len(0, hint, nullable) as u64,
            large: large_stored_len(len),
            large_values: 1,
        }
    } else {
        RowCost {
            inline: bytes_raw_len(len, hint, nullable) as u64,
            ..RowCost::default()
        }
    }
}

/// Whether `rows` rows of `columns` columns with total cost `cost` surely fit one chunk.
pub(crate) fn fits(columns: usize, rows: usize, cost: RowCost) -> bool {
    let blocks = columns as u64 + cost.inline / MIN_BLOCK_FILL as u64;
    if rows > MAX_ROWS
        || blocks > MAX_BLOCKS as u64
        || cost.large_values as usize > MAX_LARGE_VALUES
    {
        return false;
    }
    let fragment0 = max_fragment0_len(columns, blocks as usize, cost.large_values as usize) as u64;
    let bytes = cost.inline + cost.large + blocks * BLOCK_PLACEMENT_OVERHEAD + fragment0;
    fragment0 <= MAX_FRAGMENT_LEN as u64 && bytes <= MAX_CHUNK_LEN
}

/// Splits rows, costed one by one, into consecutive chunk row ranges, each as long as the
/// limits allow. Fails when there are too many columns, or when one row cannot fit a chunk
/// on its own. Validates individual rows first, then yields boundaries without collecting them.
pub(crate) fn plan_chunks(
    columns: usize,
    rows: &[RowCost],
) -> Result<impl Iterator<Item = Range<usize>> + '_, PlanError> {
    ensure!(
        (1..=MAX_COLUMNS).contains(&columns),
        PlanColumnsSnafu { columns }
    );
    for (row, &cost) in rows.iter().enumerate() {
        ensure!(fits(columns, 1, cost), RowTooLargeSnafu { row });
    }
    let mut start = 0;
    Ok(std::iter::from_fn(move || {
        if start == rows.len() {
            return None;
        }
        let mut end = start;
        let mut total = RowCost::default();
        while end < rows.len() && fits(columns, end + 1 - start, total + rows[end]) {
            total = total + rows[end];
            end += 1;
        }
        let range = start..end;
        start = end;
        Some(range)
    }))
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn row_cost() -> impl Strategy<Value = RowCost> {
        prop_oneof![
            8 => (1u64..20_000).prop_map(|inline| RowCost { inline, ..RowCost::default() }),
            1 => (16_385usize..1_200_000).prop_map(|len| bytes_cost(len, BytesHint::Text, true)),
        ]
    }

    proptest! {
        #[test]
        fn ranges_cover_the_rows_and_respect_every_limit(
            columns in 1usize..=MAX_COLUMNS,
            rows in prop::collection::vec(row_cost(), 0..3000),
        ) {
            let ranges = plan_chunks(columns, &rows).unwrap().collect::<Vec<_>>();
            let mut next = 0;
            for (index, range) in ranges.iter().enumerate() {
                prop_assert_eq!(range.start, next);
                prop_assert!(!range.is_empty());
                let cost: RowCost = rows[range.clone()].iter().copied().sum();
                prop_assert!(fits(columns, range.len(), cost));
                // Greedy: the next row would not have fitted.
                if index + 1 < ranges.len() {
                    let grown = cost + rows[range.end];
                    prop_assert!(!fits(columns, range.len() + 1, grown));
                }
                next = range.end;
            }
            prop_assert_eq!(next, rows.len());
        }
    }

    #[test]
    fn the_row_limit_ends_a_chunk() {
        let rows = vec![int_cost(IntType::U32, false); MAX_ROWS + 1];
        let ranges = plan_chunks(1, &rows).unwrap().collect::<Vec<_>>();
        assert_eq!(ranges, [0..MAX_ROWS, MAX_ROWS..MAX_ROWS + 1]);
    }

    #[test]
    fn the_byte_limit_ends_a_chunk() {
        // 8 KiB per row: about 500 rows reach 4 MiB.
        let rows = vec![bytes_cost(8 * 1024, BytesHint::Text, false); 1000];
        let ranges = plan_chunks(1, &rows).unwrap().collect::<Vec<_>>();
        assert!(ranges.len() >= 2);
        let first: RowCost = rows[ranges[0].clone()].iter().copied().sum();
        assert!(first.inline <= MAX_CHUNK_LEN);
        assert!(first.inline + rows[0].inline > MAX_CHUNK_LEN - 32 * 1024);
    }

    #[test]
    fn a_row_too_large_for_any_chunk_fails() {
        let rows = [
            bytes_cost(10, BytesHint::Text, false),
            bytes_cost(MAX_CHUNK_LEN as usize, BytesHint::Text, false),
        ];
        assert!(matches!(
            plan_chunks(1, &rows),
            Err(PlanError::RowTooLarge { row: 1 })
        ));
        assert!(matches!(
            plan_chunks(MAX_COLUMNS + 1, &[]),
            Err(PlanError::PlanColumns { .. })
        ));
    }
}
