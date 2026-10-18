//! The builder, the planner and the reader together: round trips, row batches, limits, and
//! hostile fragments.

use std::{cell::Cell, collections::BTreeMap, ops::Range};

use proptest::prelude::*;

use super::{
    FragmentSink, FragmentSource,
    builder::{BuiltChunk, ChunkBuilder, Column, ColumnLayout, KEY_COLUMN, Values, working_memory},
    error::{BlockProblem, BuildError, ColumnProblem, HeaderProblem, LargeProblem, ReadError},
    format::{
        BLOCK_TARGET_LEN, BlockEntry, BlockRecord, ChunkKind, ColumnDesc, ColumnRecord, ColumnTag,
        DATA_PAYLOAD_LEN, DirectoryRecord, HeaderRecord, IntRange, IntStats, LargeEntry,
        MAX_BLOCK_ENTRY_LEN, MAX_BLOCKS, MAX_CHUNK_LEN, MAX_COLUMN_LEN, MAX_COLUMNS,
        MAX_DATA_FRAGMENT_LEN, MAX_HEADER_LEN, MAX_LARGE_ENTRY_LEN, MAX_LARGE_VALUE_LEN,
        MAX_LARGE_VALUES, MAX_LIST_PREFIX_LEN, MAX_ROWS, MAX_STATS_LEN, MIN_BLOCK_ENTRY_LEN,
        MIN_COLUMN_LEN, MIN_FRAGMENT0_LEN, MIN_HEADER_LEN, MIN_LARGE_ENTRY_LEN, PhysicalType,
        max_fragment0_len,
    },
    plan::{RowCost, bytes_cost, int_cost, plan_chunks},
    reader::ChunkReader,
};
use crate::{
    ErrorKind,
    budget::MemoryBudget,
    codec::{
        Codec, DecodeError, Encoding, MAX_BLOCK_LEN, MAX_INLINE_VALUE_LEN,
        block::{
            BlockDecoder, BytesBlock, BytesValue, EncodeScratch, Encoded, IntBlock,
            MAX_BLOCK_OVERHEAD, bytes_raw_len, encode_bytes, encode_ints, int_raw_len,
        },
        bytes::{BytesEncoding, BytesHint},
        checksum::{self, TRAILER_LEN},
        compress,
        int::IntType,
    },
    storage::{self, ChunkId, FragmentId, MAX_FRAGMENT_LEN, StorageError},
};

const FINGERPRINT: u64 = 0x5eed_f00d;

/// Fragments in memory, as a backend holds them.
#[derive(Debug, Default)]
struct Fragments {
    map: BTreeMap<u32, Vec<u8>>,
    largest: Cell<usize>,
}

impl Fragments {
    fn total(&self) -> u64 {
        self.map
            .values()
            .map(|fragment| fragment.len() as u64)
            .sum()
    }

    /// Rewrites fragment `index`'s content and seals it again, so that only the structure
    /// is wrong, not the checksum.
    fn reseal(&mut self, index: u32, edit: impl FnOnce(&mut Vec<u8>)) {
        let fragment = self.map.get_mut(&index).unwrap();
        fragment.truncate(fragment.len() - TRAILER_LEN);
        edit(fragment);
        let mut charged = crate::budget::test_account()
            .buffer(fragment.len() + TRAILER_LEN)
            .unwrap();
        charged.extend_from_slice(fragment);
        checksum::seal(&mut charged);
        *fragment = charged.to_vec();
    }

    fn directory(&self) -> DirectoryRecord {
        let content = checksum::verify(&self.map[&0]).unwrap();
        DirectoryRecord::read(content).unwrap()
    }

    /// Lets `edit` change the records of fragment 0, and writes them back sealed.
    fn edit_directory(&mut self, edit: impl FnOnce(&mut DirectoryRecord)) {
        self.reseal(0, |content| {
            let mut record = DirectoryRecord::read(content).unwrap();
            edit(&mut record);
            content.clear();
            record.write(content);
        });
    }
}

impl FragmentSink for Fragments {
    fn put(&mut self, index: u32, bytes: &[u8]) -> Result<(), StorageError> {
        assert!(
            self.map.insert(index, bytes.to_vec()).is_none(),
            "fragment {index} written twice"
        );
        self.largest.set(self.largest.get().max(bytes.len()));
        Ok(())
    }
}

/// Lends a fragment within the limit only, as the storage port does.
impl FragmentSource for Fragments {
    fn get<T>(
        &self,
        index: u32,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T, StorageError> {
        let stored = self.map.get(&index).map(Vec::as_slice);
        if let Some(bytes) = stored {
            let id = FragmentId {
                chunk: ChunkId(0),
                index,
            };
            storage::check_lent(id, bytes.len(), limit)?;
        }
        Ok(f(stored))
    }
}

/// A fixed xorshift sequence, so generated tables are cheap and reproducible from a seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }

    fn noise(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next() as u8).collect()
    }

    fn text(&mut self, len: usize) -> Vec<u8> {
        (0..len)
            .map(|_| b"abcdefgh ,.:/"[self.below(13) as usize])
            .collect()
    }
}

fn int(ty: IntType, nullable: bool) -> ColumnLayout {
    ColumnLayout {
        ty: PhysicalType::Int { ty, summed: false },
        nullable,
    }
}

fn bytes(hint: BytesHint, nullable: bool) -> ColumnLayout {
    ColumnLayout {
        ty: PhysicalType::Bytes(hint),
        nullable,
    }
}

const SUMMED: ColumnLayout = ColumnLayout {
    ty: PhysicalType::Int {
        ty: IntType::U64,
        summed: true,
    },
    nullable: true,
};

/// Builds a chunk of `layout` from what `feed` appends, compressed or not.
fn build_chunk(
    layout: &[ColumnLayout],
    compressed: bool,
    feed: impl FnOnce(&mut ChunkBuilder<'_, Fragments>),
) -> (Fragments, BuiltChunk) {
    let mut fragments = Fragments::default();
    let mut compressor = compress::new_compressor(&crate::budget::test_account()).unwrap();
    let (kind, compressor) = if compressed {
        (ChunkKind::Consolidated, Some(&mut compressor))
    } else {
        (ChunkKind::L0Base, None)
    };
    let mut builder = ChunkBuilder::new(
        &mut fragments,
        kind,
        FINGERPRINT,
        compressor,
        layout,
        &crate::budget::test_account(),
    )
    .unwrap();
    feed(&mut builder);
    let built = builder.finish().unwrap();
    (fragments, built)
}

/// One chunk's worth of every column type a consumer maps onto, with nulls.
#[derive(Clone)]
struct Table {
    keys: Vec<i64>,
    upload: Vec<u64>,
    upload_valid: Vec<bool>,
    levels: Vec<u32>,
    signed: Vec<i64>,
    signed_valid: Vec<bool>,
    labels: Vec<Vec<u8>>,
    labels_valid: Vec<bool>,
    texts: Vec<Vec<u8>>,
    texts_valid: Vec<bool>,
    ids: Vec<Vec<u8>>,
}

const LABELS: [&[u8]; 4] = [b"", b"DIRECT", b"proxy-a", b"(other)"];

fn table_layout() -> [ColumnLayout; 7] {
    [
        KEY_COLUMN,
        SUMMED,
        int(IntType::U32, false),
        int(IntType::I64, true),
        bytes(BytesHint::LowCardinality, true),
        bytes(BytesHint::Text, true),
        bytes(BytesHint::Fixed(17), false),
    ]
}

impl Table {
    /// `large` lists the lengths of the large text values, placed on rows the seed picks.
    fn generate(rows: usize, seed: u64, large: &[usize]) -> Self {
        let mut rng = Rng(seed | 1);
        let mut key = rng.below(1 << 40) as i64 - (1 << 39);
        let mut table = Self {
            keys: Vec::new(),
            upload: Vec::new(),
            upload_valid: Vec::new(),
            levels: Vec::new(),
            signed: Vec::new(),
            signed_valid: Vec::new(),
            labels: Vec::new(),
            labels_valid: Vec::new(),
            texts: Vec::new(),
            texts_valid: Vec::new(),
            ids: Vec::new(),
        };
        for _ in 0..rows {
            key += rng.below(5_000) as i64;
            table.keys.push(key);
            table.upload.push(if rng.chance(50) {
                u64::MAX
            } else {
                rng.below(1 << 20)
            });
            table.upload_valid.push(!rng.chance(7));
            table.levels.push(rng.below(6) as u32);
            table.signed.push(match rng.below(4) {
                0 => i64::MIN,
                1 => i64::MAX,
                _ => rng.next() as i64,
            });
            table.signed_valid.push(!rng.chance(3));
            table.labels.push(LABELS[rng.below(4) as usize].to_vec());
            table.labels_valid.push(!rng.chance(5));
            let len = rng.below(120) as usize;
            table.texts.push(rng.text(len));
            table.texts_valid.push(!rng.chance(9));
            let mut id = vec![1];
            id.extend(rng.noise(16));
            table.ids.push(id);
        }
        for &len in large {
            let row = rng.below(rows as u64) as usize;
            table.texts[row] = rng.noise(len);
            table.texts_valid[row] = true;
        }
        table
    }

    fn rows(&self) -> usize {
        self.keys.len()
    }

    /// The rows `rows`, as a table of their own.
    fn slice(&self, rows: Range<usize>) -> Self {
        Self {
            keys: self.keys[rows.clone()].to_vec(),
            upload: self.upload[rows.clone()].to_vec(),
            upload_valid: self.upload_valid[rows.clone()].to_vec(),
            levels: self.levels[rows.clone()].to_vec(),
            signed: self.signed[rows.clone()].to_vec(),
            signed_valid: self.signed_valid[rows.clone()].to_vec(),
            labels: self.labels[rows.clone()].to_vec(),
            labels_valid: self.labels_valid[rows.clone()].to_vec(),
            texts: self.texts[rows.clone()].to_vec(),
            texts_valid: self.texts_valid[rows.clone()].to_vec(),
            ids: self.ids[rows].to_vec(),
        }
    }

    /// Appends every row of the table as one batch.
    fn append_to(&self, builder: &mut ChunkBuilder<'_, Fragments>) -> Result<(), BuildError> {
        let labels: Vec<&[u8]> = self.labels.iter().map(Vec::as_slice).collect();
        let texts: Vec<&[u8]> = self.texts.iter().map(Vec::as_slice).collect();
        let ids: Vec<&[u8]> = self.ids.iter().map(Vec::as_slice).collect();
        builder.append(&[
            Column {
                values: Values::I64(&self.keys),
                validity: None,
            },
            Column {
                values: Values::U64(&self.upload),
                validity: Some(&self.upload_valid),
            },
            Column {
                values: Values::U32(&self.levels),
                validity: None,
            },
            Column {
                values: Values::I64(&self.signed),
                validity: Some(&self.signed_valid),
            },
            Column {
                values: Values::Bytes(&labels),
                validity: Some(&self.labels_valid),
            },
            Column {
                values: Values::Bytes(&texts),
                validity: Some(&self.texts_valid),
            },
            Column {
                values: Values::Bytes(&ids),
                validity: None,
            },
        ])
    }

    /// Builds the table's rows as one batch.
    fn build(&self, compressed: bool) -> (Fragments, BuiltChunk) {
        build_chunk(&table_layout(), compressed, |builder| {
            self.append_to(builder).unwrap()
        })
    }

    /// Builds the table's rows from a source that hands them out once, in batches of the
    /// sizes in `sizes`, taken in turn.
    fn build_streamed(&self, compressed: bool, sizes: &[usize]) -> (Fragments, BuiltChunk) {
        build_chunk(&table_layout(), compressed, |builder| {
            for batch in self.stream(sizes) {
                batch.append_to(builder).unwrap();
            }
        })
    }

    /// The rows as owned batches, produced one at a time: the builder can see a batch only
    /// while it is being appended.
    fn stream<'a>(&'a self, sizes: &'a [usize]) -> impl Iterator<Item = Table> + 'a {
        let mut start = 0;
        let mut turn = 0;
        std::iter::from_fn(move || {
            if start == self.rows() {
                return None;
            }
            let end = (start + sizes[turn % sizes.len()]).min(self.rows());
            turn += 1;
            let batch = self.slice(start..end);
            start = end;
            Some(batch)
        })
    }

    /// The raw size of one row of column `column`, as the builder counts it.
    fn raw_len(&self, column: usize, row: usize) -> usize {
        let text = |values: &[Vec<u8>], valid: &[bool], hint| {
            let len = if valid[row] { values[row].len() } else { 0 };
            let large = hint == BytesHint::Text && len > MAX_INLINE_VALUE_LEN;
            bytes_raw_len(if large { 0 } else { len }, hint, true)
        };
        match column {
            0 => int_raw_len(IntType::I64, false),
            1 => int_raw_len(IntType::U64, true),
            2 => int_raw_len(IntType::U32, false),
            3 => int_raw_len(IntType::I64, true),
            4 => text(&self.labels, &self.labels_valid, BytesHint::LowCardinality),
            5 => text(&self.texts, &self.texts_valid, BytesHint::Text),
            _ => bytes_raw_len(17, BytesHint::Fixed(17), false),
        }
    }
}

/// Fragment 0's length, as the chunk's manifest entry would record it; 0 when it is missing.
fn recorded(fragments: &Fragments) -> u32 {
    fragments
        .map
        .get(&0)
        .map_or(0, |fragment| fragment.len() as u32)
}

/// Opens a chunk with a budget of its own, which holds exactly the reader's bytes once it is
/// open, and nothing when opening fails.
fn open_chunk(fragments: &Fragments) -> Result<ChunkReader, ReadError> {
    let budget = MemoryBudget::new(u64::MAX);
    let account = crate::budget::MemoryAccount::new(
        &budget,
        "test reader",
        crate::budget::MemoryPlan::default(),
    )
    .unwrap();
    account
        .reserve_lent(crate::chunk::reader::lent_memory(recorded(fragments)))
        .unwrap();
    let opened = ChunkReader::open(fragments, &account, recorded(fragments));
    drop(account);
    let held = opened.as_ref().map_or(0, ChunkReader::memory);
    assert_eq!(
        budget.reserved(),
        held + opened
            .as_ref()
            .map_or(0, |_| crate::chunk::reader::lent_memory(recorded(
                fragments
            )))
    );
    opened
}

/// Reads every block, view and large value of a chunk, as a query could.
fn read_everything(fragments: &Fragments) -> Result<(), ReadError> {
    let reader = open_chunk(fragments)?;
    let mut decoder = BlockDecoder::new(&crate::budget::test_account()).unwrap();
    let mut signed = IntBlock::<i64>::default();
    let mut unsigned = IntBlock::<u64>::default();
    let mut narrow = IntBlock::<u32>::default();
    let mut bytes = BytesBlock::default();
    let mut large = reader.account().buffer(0).expect("empty storage is free");
    for (column, desc) in reader.columns().iter().enumerate() {
        let column = column as u16;
        for block in 0..desc.blocks {
            match desc.ty {
                PhysicalType::Int {
                    ty: IntType::I64, ..
                } => {
                    reader.read_ints(fragments, column, block, &mut decoder, &mut signed)?;
                    reader.view_ints::<i64, _>(fragments, column, block, |view| view.len())?;
                }
                PhysicalType::Int {
                    ty: IntType::U64, ..
                } => {
                    reader.read_ints(fragments, column, block, &mut decoder, &mut unsigned)?;
                }
                PhysicalType::Int {
                    ty: IntType::U32, ..
                } => {
                    reader.read_ints(fragments, column, block, &mut decoder, &mut narrow)?;
                }
                PhysicalType::Bytes(_) => {
                    reader.read_bytes(fragments, column, block, &mut decoder, &mut bytes)?;
                    for value in bytes.large_values() {
                        reader.read_large(fragments, value.index, &mut large)?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// A value as a chunk reads back, large values read in.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReadBack {
    Null,
    Int(i128),
    Bytes(Vec<u8>),
}

/// What a chunk reads back as, independent of where its blocks were placed: the header,
/// the columns, each block's rows, encoding and stats, and every value.
#[derive(Debug, PartialEq, Eq)]
struct Content {
    rows: u32,
    key_range: (i64, i64),
    columns: Vec<ColumnDesc>,
    blocks: Vec<(u32, u32, Codec, u32, u32, IntStats)>,
    cells: Vec<Vec<ReadBack>>,
}

fn content(fragments: &Fragments) -> Content {
    let reader = open_chunk(fragments).unwrap();
    let mut decoder = BlockDecoder::new(&crate::budget::test_account()).unwrap();
    let mut signed = IntBlock::<i64>::default();
    let mut unsigned = IntBlock::<u64>::default();
    let mut narrow = IntBlock::<u32>::default();
    let mut bytes = BytesBlock::default();
    let mut blocks = Vec::new();
    let mut cells = Vec::new();
    for (column, desc) in reader.columns().iter().enumerate() {
        let column = column as u16;
        let mut values = Vec::new();
        for (index, entry) in reader.blocks(column).unwrap().iter().enumerate() {
            blocks.push((
                entry.first_row,
                entry.rows,
                entry.codec,
                entry.stored_len,
                entry.uncompressed_len,
                entry.stats,
            ));
            let block = index as u32;
            let rows = 0..entry.rows as usize;
            let int = |value: Option<i128>| value.map_or(ReadBack::Null, ReadBack::Int);
            match desc.ty {
                PhysicalType::Int {
                    ty: IntType::I64, ..
                } => {
                    reader
                        .read_ints(fragments, column, block, &mut decoder, &mut signed)
                        .unwrap();
                    values.extend(rows.map(|row| int(signed.get(row).map(i128::from))));
                }
                PhysicalType::Int {
                    ty: IntType::U64, ..
                } => {
                    reader
                        .read_ints(fragments, column, block, &mut decoder, &mut unsigned)
                        .unwrap();
                    values.extend(rows.map(|row| int(unsigned.get(row).map(i128::from))));
                }
                PhysicalType::Int {
                    ty: IntType::U32, ..
                } => {
                    reader
                        .read_ints(fragments, column, block, &mut decoder, &mut narrow)
                        .unwrap();
                    values.extend(rows.map(|row| int(narrow.get(row).map(i128::from))));
                }
                PhysicalType::Bytes(_) => {
                    reader
                        .read_bytes(fragments, column, block, &mut decoder, &mut bytes)
                        .unwrap();
                    for row in rows {
                        values.push(match bytes.get(row) {
                            BytesValue::Null => ReadBack::Null,
                            BytesValue::Inline(value) => ReadBack::Bytes(value.to_vec()),
                            BytesValue::Large(index) => {
                                let mut large =
                                    reader.account().buffer(0).expect("empty storage is free");
                                reader.read_large(fragments, index, &mut large).unwrap();
                                ReadBack::Bytes(large.to_vec())
                            }
                        });
                    }
                }
            }
        }
        cells.push(values);
    }
    Content {
        rows: reader.rows(),
        key_range: reader.key_range(),
        columns: reader.columns().to_vec(),
        blocks,
        cells,
    }
}

fn check_round_trip(
    table: &Table,
    (fragments, built): &(Fragments, BuiltChunk),
    compressed: bool,
) -> Result<(), TestCaseError> {
    let reader = open_chunk(fragments).unwrap();

    // The header and the accounting.
    let keys = &table.keys;
    let key_range = (*keys.iter().min().unwrap(), *keys.iter().max().unwrap());
    prop_assert_eq!(reader.rows() as usize, table.rows());
    prop_assert_eq!(reader.key_range(), key_range);
    prop_assert_eq!(built.key_range, key_range);
    prop_assert_eq!(reader.schema_fingerprint(), FINGERPRINT);
    prop_assert_eq!(
        reader.kind(),
        if compressed {
            ChunkKind::Consolidated
        } else {
            ChunkKind::L0Base
        }
    );
    prop_assert_eq!(reader.fragments() as usize, fragments.map.len());
    prop_assert_eq!(built.fragments as usize, fragments.map.len());
    prop_assert_eq!(built.bytes, fragments.total());
    prop_assert!(built.bytes <= MAX_CHUNK_LEN);
    prop_assert!(fragments.largest.get() <= MAX_DATA_FRAGMENT_LEN);
    let blocks: usize = reader
        .columns()
        .iter()
        .map(|desc| desc.blocks as usize)
        .sum();
    let bound = max_fragment0_len(7, blocks, reader.large_values().len());
    prop_assert!(fragments.map[&0].len() <= bound);

    // Every block is within its limits, and within the raw-size bound planning relies on.
    let mut zstd_blocks = 0;
    for column in 0..reader.columns().len() {
        for block in reader.blocks(column as u16).unwrap() {
            let rows = block.first_row as usize..(block.first_row + block.rows) as usize;
            let raw: usize = rows.map(|row| table.raw_len(column, row)).sum();
            prop_assert!(raw <= BLOCK_TARGET_LEN);
            prop_assert!(block.uncompressed_len as usize <= MAX_BLOCK_LEN);
            prop_assert!(block.stored_len as usize <= raw + MAX_BLOCK_OVERHEAD);
            zstd_blocks += usize::from(block.codec == Codec::Zstd);
        }
    }
    if !compressed {
        prop_assert_eq!(zstd_blocks, 0);
    } else if table.rows() >= 1000 {
        // The text column is redundant enough for zstd.
        prop_assert!(zstd_blocks > 0);
    }

    // The values.
    let expect = |valid: &[bool], row: usize| valid.get(row).copied().unwrap_or(true);
    let ints = |values: &[i128], valid: &[bool]| -> Vec<ReadBack> {
        (0..values.len())
            .map(|row| {
                if expect(valid, row) {
                    ReadBack::Int(values[row])
                } else {
                    ReadBack::Null
                }
            })
            .collect()
    };
    let byte_cells = |values: &[Vec<u8>], valid: &[bool]| -> Vec<ReadBack> {
        (0..values.len())
            .map(|row| {
                if expect(valid, row) {
                    ReadBack::Bytes(values[row].clone())
                } else {
                    ReadBack::Null
                }
            })
            .collect()
    };
    let widen = |values: &[i64]| {
        values
            .iter()
            .map(|&value| i128::from(value))
            .collect::<Vec<_>>()
    };
    let expected = [
        ints(&widen(&table.keys), &[]),
        ints(
            &table
                .upload
                .iter()
                .map(|&value| i128::from(value))
                .collect::<Vec<_>>(),
            &table.upload_valid,
        ),
        ints(
            &table
                .levels
                .iter()
                .map(|&value| i128::from(value))
                .collect::<Vec<_>>(),
            &[],
        ),
        ints(&widen(&table.signed), &table.signed_valid),
        byte_cells(&table.labels, &table.labels_valid),
        byte_cells(&table.texts, &table.texts_valid),
        byte_cells(&table.ids, &[]),
    ];
    let read = content(fragments);
    prop_assert!(read.cells == expected, "the values read back differ");

    // Raw integer blocks read in place give the decoded values.
    let mut decoder = BlockDecoder::new(&crate::budget::test_account()).unwrap();
    let mut signed = IntBlock::<i64>::default();
    for column in [0u16, 3] {
        for block in 0..reader.columns()[usize::from(column)].blocks {
            reader
                .read_ints(fragments, column, block, &mut decoder, &mut signed)
                .unwrap();
            let viewed = reader
                .view_ints::<i64, _>(fragments, column, block, |view| {
                    (0..view.len()).map(|row| view.get(row)).collect::<Vec<_>>()
                })
                .unwrap();
            if let Some(viewed) = viewed {
                let decoded: Vec<_> = (0..signed.len()).map(|row| signed.get(row)).collect();
                prop_assert_eq!(viewed, decoded);
            }
        }
    }

    // The chunk stats a query skips chunks by.
    let columns = reader.columns();
    let valid_upload = table
        .upload
        .iter()
        .zip(&table.upload_valid)
        .filter_map(|(&value, &valid)| valid.then_some(value));
    let sum = valid_upload
        .clone()
        .fold(0u64, |sum, value| sum.saturating_add(value));
    prop_assert_eq!(columns[1].stats.sum, Some(sum));
    prop_assert_eq!(
        columns[1].stats.range,
        valid_upload
            .clone()
            .min()
            .zip(valid_upload.clone().max())
            .map(|(min, max)| IntRange::Unsigned { min, max })
    );
    prop_assert_eq!(
        columns[1].stats.presence.is_some(),
        valid_upload.clone().all(|value| value < 64)
    );
    let levels = table
        .levels
        .iter()
        .fold(0u64, |set, &level| set | 1 << level);
    prop_assert_eq!(columns[2].stats.presence, Some(levels));
    prop_assert_eq!(columns[2].stats.sum, None);
    prop_assert_eq!(
        columns[0].stats.range,
        Some(IntRange::Signed {
            min: key_range.0,
            max: key_range.1
        })
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Every chunk round trips, and it reads back the same whether its rows came in one
    /// batch or streamed in batches of any sizes.
    #[test]
    fn chunks_round_trip(
        rows in 1usize..6000,
        seed in any::<u64>(),
        large in prop::collection::vec(
            prop_oneof![
                Just(MAX_INLINE_VALUE_LEN + 1),
                Just(DATA_PAYLOAD_LEN),
                Just(DATA_PAYLOAD_LEN + 1),
                (MAX_INLINE_VALUE_LEN + 1)..600_000,
            ],
            0..3,
        ),
        compressed in any::<bool>(),
        sizes in prop::collection::vec(1usize..2500, 1..6),
    ) {
        let table = Table::generate(rows, seed, &large);
        let whole = table.build(compressed);
        check_round_trip(&table, &whole, compressed)?;
        let streamed = table.build_streamed(compressed, &sizes);
        check_round_trip(&table, &streamed, compressed)?;
        prop_assert!(content(&whole.0) == content(&streamed.0));
    }
}

#[test]
fn columns_split_into_blocks_and_large_values_span_fragments() {
    let large = [DATA_PAYLOAD_LEN + 1, MAX_INLINE_VALUE_LEN + 1];
    let table = Table::generate(9000, 42, &large);
    let (fragments, _) = table.build(false);
    let reader = open_chunk(&fragments).unwrap();
    // 9000 keys of 8 bytes take three blocks of 4092 rows.
    assert_eq!(reader.blocks(0).unwrap().len(), 3);
    assert!(reader.blocks(5).unwrap().len() > 1);
    let mut spans: Vec<u32> = reader
        .large_values()
        .iter()
        .map(|entry| entry.fragments)
        .collect();
    spans.sort_unstable();
    assert_eq!(spans, [1, 2]);
    read_everything(&fragments).unwrap();
}

#[test]
fn compressed_round_trip_of_a_full_table() {
    let table = Table::generate(5000, 7, &[300_000]);
    check_round_trip(&table, &table.build(true), true).unwrap();
    check_round_trip(&table, &table.build(false), false).unwrap();
}

/// A merge hands rows over as it produces them, a few at a time. The builder takes them
/// from a source that cannot go back, interleaves the columns' blocks in its fragments,
/// and the chunk reads back as the same rows built in one batch.
#[test]
fn rows_streamed_in_small_batches_read_back_as_one_batch_does() {
    let table = Table::generate(20_000, 3, &[40_000, DATA_PAYLOAD_LEN + 1]);
    let whole = table.build(true);
    let streamed = table.build_streamed(true, &[1, 7, 300, 2]);
    check_round_trip(&table, &streamed, true).unwrap();
    assert_eq!(content(&whole.0), content(&streamed.0));

    // Small batches fill every column's open block at once, so data fragments mix
    // columns.
    let reader = open_chunk(&streamed.0).unwrap();
    let mut columns_by_fragment: BTreeMap<u32, Vec<u16>> = BTreeMap::new();
    for column in 0..7 {
        for block in reader.blocks(column).unwrap() {
            columns_by_fragment
                .entry(block.fragment)
                .or_default()
                .push(column);
        }
    }
    assert!(
        columns_by_fragment
            .values()
            .any(|columns| columns.iter().any(|&column| column != columns[0])),
        "no data fragment holds blocks of two columns"
    );
}

/// The builder writes a column's blocks as they fill, not when the chunk ends: what it
/// holds back is at most one open block per column and one open fragment.
#[test]
fn fragments_are_written_while_rows_arrive() {
    /// Counts what is written, and keeps nothing.
    struct Counting<'a>(&'a Cell<u64>);

    impl FragmentSink for Counting<'_> {
        fn put(&mut self, _: u32, bytes: &[u8]) -> Result<(), StorageError> {
            self.0.set(self.0.get() + bytes.len() as u64);
            Ok(())
        }
    }

    let mut rng = Rng(23);
    let texts: Vec<Vec<u8>> = (0..3000).map(|_| rng.noise(1000)).collect();
    let written = Cell::new(0);
    let mut sink = Counting(&written);
    let layout = [KEY_COLUMN, bytes(BytesHint::Text, false)];
    let mut builder = ChunkBuilder::new(
        &mut sink,
        ChunkKind::L0Base,
        0,
        None,
        &layout,
        &crate::budget::test_account(),
    )
    .unwrap();
    let mut appended = 0;
    for (batch_index, batch) in texts.chunks(10).enumerate() {
        let first = batch_index as i64 * 10;
        let keys: Vec<i64> = (first..first + batch.len() as i64).collect();
        let slices: Vec<&[u8]> = batch.iter().map(Vec::as_slice).collect();
        builder
            .append(&[
                Column {
                    values: Values::I64(&keys),
                    validity: None,
                },
                Column {
                    values: Values::Bytes(&slices),
                    validity: None,
                },
            ])
            .unwrap();
        appended += batch.len() * 1000;
    }
    // The values alone are about 2.9 MiB. Before `finish`, all of them but those in an open
    // block of each column and in the open fragment are written; a text block stores its
    // values as they are.
    let before_finish = written.get();
    let held_back = (2 * BLOCK_TARGET_LEN + DATA_PAYLOAD_LEN) as u64;
    assert!(appended > 2 * 1024 * 1024);
    assert!(
        before_finish + held_back >= appended as u64,
        "{before_finish} bytes written before the chunk ended, of {appended}"
    );
    let built = builder.finish().unwrap();
    assert_eq!(built.bytes, written.get());
}

/// Each buffer of an open block gets its own cap, the most one block needs of it, the first
/// time a row needs it, and keeps it from block to block: it never grows from one allocation
/// to another, which would hold both for a moment. A nullable `i64` row is 9 bytes, so that
/// column's cap is a block's 3637 rows; a one-byte low-cardinality value is 5, so 6547.
#[test]
fn an_open_block_takes_its_caps_at_once_and_keeps_them() {
    let layout = [
        KEY_COLUMN,
        int(IntType::I64, true),
        bytes(BytesHint::LowCardinality, false),
    ];
    let i64_rows = BLOCK_TARGET_LEN / int_raw_len(IntType::I64, true);
    let byte_rows = BLOCK_TARGET_LEN / bytes_raw_len(1, BytesHint::LowCardinality, false);
    assert_eq!((i64_rows, byte_rows), (3637, 6547));
    let mut fragments = Fragments::default();
    let mut builder = ChunkBuilder::new(
        &mut fragments,
        ChunkKind::L0Base,
        0,
        None,
        &layout,
        &crate::budget::test_account(),
    )
    .unwrap();
    let mut rng = Rng(29);
    // How often each buffer's capacity changed, over two blocks of the bytes column.
    let mut changes = BTreeMap::<(usize, &str), usize>::new();
    let mut before: Vec<_> = (0..3).map(|column| builder.open_buffers(column)).collect();
    for row in 0..2 * byte_rows {
        let value = [b"abcd"[rng.below(4) as usize]];
        builder
            .append(&[
                Column {
                    values: Values::I64(&[row as i64]),
                    validity: None,
                },
                Column {
                    values: Values::I64(&[rng.next() as i64]),
                    validity: Some(&[!rng.chance(3)]),
                },
                Column {
                    values: Values::Bytes(&[&value[..]]),
                    validity: None,
                },
            ])
            .unwrap();
        for (column, before) in before.iter_mut().enumerate() {
            let after = builder.open_buffers(column);
            for ((name, old, _), &(_, new, cap)) in before.iter().zip(&after) {
                assert!(
                    new == 0 || new == cap,
                    "{name} of column {column} after row {row}: capacity {new}, cap {cap}"
                );
                if *old != new {
                    *changes.entry((column, *name)).or_default() += 1;
                }
            }
            *before = after;
        }
    }
    // Each buffer the rows use was allocated once, whole, and never again.
    let used = [
        (0, "values"),
        (1, "validity"),
        (1, "values"),
        (2, "heap"),
        (2, "ends"),
    ];
    assert_eq!(changes, used.map(|buffer| (buffer, 1)).into());
    builder.finish().unwrap();
    let reader = open_chunk(&fragments).unwrap();
    assert_eq!(reader.blocks(1).unwrap()[0].rows as usize, i64_rows);
    assert_eq!(reader.blocks(2).unwrap()[0].rows as usize, byte_rows);
    read_everything(&fragments).unwrap();
}

#[test]
fn raw_blocks_are_viewed_in_place_and_others_are_not() {
    let mut rng = Rng(3);
    // Random 64-bit values only fit raw; a counter compresses to frame of reference.
    let random: Vec<i64> = (0..100).map(|_| rng.next() as i64).collect();
    let counter: Vec<i64> = (0..100).collect();
    let (fragments, _) = build_chunk(&[KEY_COLUMN, int(IntType::I64, false)], false, |builder| {
        builder
            .append(&[
                Column {
                    values: Values::I64(&random),
                    validity: None,
                },
                Column {
                    values: Values::I64(&counter),
                    validity: None,
                },
            ])
            .unwrap();
    });
    let reader = open_chunk(&fragments).unwrap();
    let viewed = reader
        .view_ints::<i64, _>(&fragments, 0, 0, |view| {
            view.values()
                .iter()
                .map(|value| value.get())
                .collect::<Vec<_>>()
        })
        .unwrap();
    assert_eq!(viewed, Some(random));
    assert_eq!(
        reader
            .view_ints::<i64, _>(&fragments, 1, 0, |view| view.len())
            .unwrap(),
        None
    );
    // Asking for another type than the column's is a mismatch with the schema.
    let error = reader
        .view_ints::<u64, _>(&fragments, 0, 0, |view| view.len())
        .unwrap_err();
    assert!(matches!(error, ReadError::ColumnType { column: 0, .. }));
    assert_eq!(error.kind(), ErrorKind::Corrupted);
}

/// The postcard encoding of fragment 0 of [`tiny_chunk`], written out by hand from the wire
/// specification (<https://postcard.jamesmunns.com/wire-format>), not produced by running
/// postcard. In it, a `u8` and a `bool` are one byte, every wider integer is a varint (7
/// bits per byte, least significant first, the high bit set while more follow) and a
/// signed one is zigzagged first (`2n` for `n >= 0`), a sequence is its length and then its
/// records, an `Option` is 0 or 1 and then the value, a struct is its fields in declaration
/// order, and an enum variant is its position in the declaration and then its fields.
fn tiny_fragment0() -> Vec<u8> {
    [
        // "NTSC", then the format version 1 as a little-endian u16
        &[b'N', b'T', b'S', b'C', 0x01, 0x00][..],
        // header: kind L0Base = 1
        &[0x01],
        // fingerprint 2^63: nine groups of zero bits, then the 64th bit
        &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01],
        // 1 row, 2 fragments
        &[0x01, 0x02],
        // key range 300..=300: zigzag 600 = 0b100_1011000
        &[0xd8, 0x04, 0xd8, 0x04],
        // 3 columns
        &[0x03],
        // column 0: type I64 = 1, no hint, width 0, not nullable, not summed, 1 block;
        // stats: range Some(Signed (variant 0) 300..=300), no sum, no presence (300 >= 64)
        &[0x01, 0x00, 0x00, 0x00, 0x00, 0x01],
        &[0x01, 0x00, 0xd8, 0x04, 0xd8, 0x04, 0x00, 0x00],
        // column 1: type U64 = 2, no hint, width 0, nullable, summed, 1 block; stats: no
        // range (the row is null), sum Some(0), presence Some(0)
        &[0x02, 0x00, 0x00, 0x01, 0x01, 0x01],
        &[0x00, 0x01, 0x00, 0x01, 0x00],
        // column 2: type Bytes = 4, hint Fixed = 3, width 2, not nullable, not summed, 1
        // block; no stats
        &[0x04, 0x03, 0x02, 0x00, 0x00, 0x01],
        &[0x00, 0x00, 0x00],
        // 3 blocks, all in fragment 1, raw (codec 0)
        &[0x03],
        // block 0 of column 0: encoding Raw = 5, row 0, 1 row, fragment 1, offset 0; stored
        // 15 bytes = 2 tags + flags + 4-byte row count + 8-byte value, of which 13 are the
        // body; the column's stats
        &[0x00, 0x00, 0x05, 0x00, 0x01, 0x01, 0x00, 0x0f, 0x0d],
        &[0x01, 0x00, 0xd8, 0x04, 0xd8, 0x04, 0x00, 0x00],
        // block 0 of column 1: Raw, at offset 16 (15 rounded up to 8), stored 16 bytes: the
        // body adds a validity byte
        &[0x01, 0x00, 0x05, 0x00, 0x01, 0x01, 0x10, 0x10, 0x0e],
        &[0x00, 0x01, 0x00, 0x01, 0x00],
        // block 0 of column 2: encoding Fixed = 18, at offset 32, stored 13 bytes: 2 tags,
        // flags, row count, the width as a u32, and "ab"
        &[0x02, 0x00, 0x12, 0x00, 0x01, 0x01, 0x20, 0x0d, 0x0b],
        &[0x00, 0x00, 0x00],
        // no large values
        &[0x00],
    ]
    .concat()
}

/// One row: the key 300, a null summed `u64`, and a two-byte fixed value.
fn tiny_chunk() -> Fragments {
    let layout = [KEY_COLUMN, SUMMED, bytes(BytesHint::Fixed(2), false)];
    let mut fragments = Fragments::default();
    let mut builder = ChunkBuilder::new(
        &mut fragments,
        ChunkKind::L0Base,
        1 << 63,
        None,
        &layout,
        &crate::budget::test_account(),
    )
    .unwrap();
    builder
        .append(&[
            Column {
                values: Values::I64(&[300]),
                validity: None,
            },
            Column {
                values: Values::U64(&[7]),
                validity: Some(&[false]),
            },
            Column {
                values: Values::Bytes(&[&b"ab"[..]]),
                validity: None,
            },
        ])
        .unwrap();
    builder.finish().unwrap();
    fragments
}

// The bytes of fragment 0 are the stored format: if this test fails, the layout changed,
// and FORMAT_VERSION has to change with it.
#[test]
fn fragment_0_is_pinned_to_the_postcard_wire_format() {
    let fragments = tiny_chunk();
    let stored = &fragments.map[&0];
    assert_eq!(checksum::verify(stored), Some(&tiny_fragment0()[..]));
    read_everything(&fragments).unwrap();
}

/// The bound fragment 0 is planned by holds: each record encodes to at most its constant,
/// which every field at its longest reaches.
#[test]
fn the_longest_records_take_their_bound() {
    fn len(value: &impl serde::Serialize) -> usize {
        postcard::to_stdvec(value).unwrap().len()
    }
    let stats = IntStats {
        range: Some(IntRange::Signed {
            min: i64::MIN,
            max: i64::MAX,
        }),
        sum: Some(u64::MAX),
        presence: Some(u64::MAX),
    };
    let unsigned = IntStats {
        range: Some(IntRange::Unsigned {
            min: u64::MAX,
            max: u64::MAX,
        }),
        ..stats
    };
    assert_eq!(len(&stats), MAX_STATS_LEN);
    assert_eq!(len(&unsigned), MAX_STATS_LEN);
    let header = HeaderRecord {
        kind: u8::MAX,
        fingerprint: u64::MAX,
        rows: u32::MAX,
        fragments: u32::MAX,
        key_min: i64::MIN,
        key_max: i64::MAX,
    };
    assert_eq!(len(&header), MAX_HEADER_LEN);
    let column = ColumnRecord {
        ty: u8::MAX,
        hint: u8::MAX,
        width: u16::MAX,
        nullable: true,
        summed: true,
        blocks: u32::MAX,
        stats,
    };
    assert_eq!(len(&column), MAX_COLUMN_LEN);
    let block = BlockRecord {
        column: u16::MAX,
        codec: u8::MAX,
        encoding: u8::MAX,
        first_row: u32::MAX,
        rows: u32::MAX,
        fragment: u32::MAX,
        offset: u32::MAX,
        stored_len: u32::MAX,
        uncompressed_len: u32::MAX,
        stats,
    };
    assert_eq!(len(&block), MAX_BLOCK_ENTRY_LEN);
    let large = LargeEntry {
        first_fragment: u32::MAX,
        fragments: u32::MAX,
        len: u32::MAX,
    };
    assert_eq!(len(&large), MAX_LARGE_ENTRY_LEN);
    let longest_list = vec![(); MAX_BLOCKS.max(MAX_COLUMNS).max(MAX_LARGE_VALUES)];
    assert_eq!(len(&longest_list), MAX_LIST_PREFIX_LEN);
}

/// How many records a fragment 0 can hold, and so what parsing it can allocate, rests on these.
#[test]
fn the_shortest_records_take_their_bound() {
    fn len(value: &impl serde::Serialize) -> usize {
        postcard::to_stdvec(value).unwrap().len()
    }
    let column = ColumnRecord {
        ty: 0,
        hint: 0,
        width: 0,
        nullable: false,
        summed: false,
        blocks: 0,
        stats: IntStats::default(),
    };
    assert_eq!(len(&column), MIN_COLUMN_LEN);
    let block = BlockRecord {
        column: 0,
        codec: 0,
        encoding: 0,
        first_row: 0,
        rows: 0,
        fragment: 0,
        offset: 0,
        stored_len: 0,
        uncompressed_len: 0,
        stats: IntStats::default(),
    };
    assert_eq!(len(&block), MIN_BLOCK_ENTRY_LEN);
    let header = HeaderRecord {
        kind: 0,
        fingerprint: 0,
        rows: 0,
        fragments: 0,
        key_min: 0,
        key_max: 0,
    };
    assert_eq!(len(&header), MIN_HEADER_LEN);
    let large = LargeEntry {
        first_fragment: 0,
        fragments: 0,
        len: 0,
    };
    assert_eq!(len(&large), MIN_LARGE_ENTRY_LEN);
    // A chunk has a column of a block at least, so no fragment 0 is shorter than this one.
    let mut shortest = crate::budget::test_account()
        .buffer(MIN_FRAGMENT0_LEN)
        .unwrap();
    DirectoryRecord {
        header,
        columns: vec![column],
        blocks: vec![block],
        large: Vec::new(),
    }
    .write(&mut shortest);
    checksum::seal(&mut shortest);
    assert_eq!(shortest.len(), MIN_FRAGMENT0_LEN);
}

/// A format-valid chunk may exceed the planner's directory cap when its blocks encode small.
#[test]
fn an_unplanned_chunk_grows_fragment_zero_before_serializing_its_directory() {
    let mut layout = vec![KEY_COLUMN];
    layout.extend([SUMMED; MAX_COLUMNS - 2]);
    layout.push(bytes(BytesHint::Fixed(384), false));
    let keys: Vec<i64> = (0..MAX_ROWS as i64).collect();
    let values = vec![u64::MAX; MAX_ROWS];
    let flags = vec![true; MAX_ROWS];
    let fixed = [0u8; 384];
    let byte_values = vec![&fixed[..]; MAX_ROWS];
    let (fragments, built) = build_chunk(&layout, true, |builder| {
        let mut columns = vec![Column {
            values: Values::I64(&keys),
            validity: None,
        }];
        columns.extend((1..MAX_COLUMNS - 1).map(|_| Column {
            values: Values::U64(&values),
            validity: Some(&flags),
        }));
        columns.push(Column {
            values: Values::Bytes(&byte_values),
            validity: None,
        });
        builder.append(&columns).unwrap();
    });
    assert!(fragments.directory().blocks.len() > 730);
    assert!(
        built.fragment0 as usize > MAX_DATA_FRAGMENT_LEN,
        "fragment zero: {} bytes",
        built.fragment0
    );
    assert!(built.fragment0 as usize <= MAX_FRAGMENT_LEN);
    assert_eq!(open_chunk(&fragments).unwrap().rows() as usize, MAX_ROWS);
}

mod limits {
    use super::*;

    fn keys(rows: usize) -> Vec<i64> {
        (0..rows as i64).collect()
    }

    fn key_batch(keys: &[i64]) -> [Column<'_>; 1] {
        [Column {
            values: Values::I64(keys),
            validity: None,
        }]
    }

    #[test]
    fn rows_past_m_are_rejected_before_anything_is_written() {
        let keys = keys(MAX_ROWS + 1);
        let mut fragments = Fragments::default();
        let mut builder = ChunkBuilder::new(
            &mut fragments,
            ChunkKind::L0Base,
            0,
            None,
            &[KEY_COLUMN],
            &crate::budget::test_account(),
        )
        .unwrap();
        let error = builder.append(&key_batch(&keys)).unwrap_err();
        assert!(matches!(error, BuildError::RowCount { rows } if rows == MAX_ROWS + 1));
        assert_eq!(error.kind(), ErrorKind::Local);
        // Across batches too: the second one would take the chunk past M.
        builder.append(&key_batch(&keys[..MAX_ROWS - 1])).unwrap();
        let error = builder.append(&key_batch(&keys[..2])).unwrap_err();
        assert!(matches!(error, BuildError::RowCount { rows } if rows == MAX_ROWS + 1));
        builder.append(&key_batch(&keys[..1])).unwrap();
        assert_eq!(builder.finish().unwrap().rows as usize, MAX_ROWS);
    }

    #[test]
    fn columns_past_the_limit_are_rejected() {
        let mut layout = vec![KEY_COLUMN];
        layout.resize(MAX_COLUMNS + 1, int(IntType::U32, false));
        let mut fragments = Fragments::default();
        let error = ChunkBuilder::new(
            &mut fragments,
            ChunkKind::L0Base,
            0,
            None,
            &layout,
            &crate::budget::test_account(),
        )
        .err()
        .unwrap();
        assert!(matches!(error, BuildError::TooManyColumns));

        let keys = keys(1);
        let values = [1u32];
        let mut batch = vec![Column {
            values: Values::I64(&keys),
            validity: None,
        }];
        batch.resize(
            MAX_COLUMNS,
            Column {
                values: Values::U32(&values),
                validity: None,
            },
        );
        let (fragments, built) = build_chunk(&layout[..MAX_COLUMNS], false, |builder| {
            builder.append(&batch).unwrap()
        });
        assert_eq!(built.rows, 1);
        let reader = open_chunk(&fragments).unwrap();
        assert_eq!(reader.columns().len(), MAX_COLUMNS);
    }

    #[test]
    fn a_chunk_past_b_is_an_error_not_an_oversized_write() {
        // 300 incompressible inline values of 16 KiB: about 4.7 MiB.
        let mut rng = Rng(11);
        let texts: Vec<Vec<u8>> = (0..300).map(|_| rng.noise(MAX_INLINE_VALUE_LEN)).collect();
        let slices: Vec<&[u8]> = texts.iter().map(Vec::as_slice).collect();
        let keys = keys(texts.len());
        let mut fragments = Fragments::default();
        let mut compressor = compress::new_compressor(&crate::budget::test_account()).unwrap();
        let mut builder = ChunkBuilder::new(
            &mut fragments,
            ChunkKind::Consolidated,
            0,
            Some(&mut compressor),
            &[KEY_COLUMN, bytes(BytesHint::Text, false)],
            &crate::budget::test_account(),
        )
        .unwrap();
        let error = builder
            .append(&[
                Column {
                    values: Values::I64(&keys),
                    validity: None,
                },
                Column {
                    values: Values::Bytes(&slices),
                    validity: None,
                },
            ])
            .unwrap_err();
        assert!(matches!(error, BuildError::ChunkTooLarge { .. }));
        assert_eq!(error.kind(), ErrorKind::Local);
        drop(builder);
        assert!(fragments.total() <= MAX_CHUNK_LEN);
        assert!(fragments.largest.get() <= MAX_DATA_FRAGMENT_LEN);
    }

    #[test]
    fn a_large_value_that_does_not_fit_is_rejected_before_its_fragments() {
        let mut rng = Rng(5);
        let texts = [rng.noise(3 << 20), rng.noise(2 << 20)];
        let slices: Vec<&[u8]> = texts.iter().map(Vec::as_slice).collect();
        let keys = keys(2);
        let mut fragments = Fragments::default();
        let mut builder = ChunkBuilder::new(
            &mut fragments,
            ChunkKind::L0Base,
            0,
            None,
            &[KEY_COLUMN, bytes(BytesHint::Text, false)],
            &crate::budget::test_account(),
        )
        .unwrap();
        let error = builder
            .append(&[
                Column {
                    values: Values::I64(&keys),
                    validity: None,
                },
                Column {
                    values: Values::Bytes(&slices),
                    validity: None,
                },
            ])
            .unwrap_err();
        assert!(matches!(error, BuildError::ChunkTooLarge { .. }));
        drop(builder);
        // The key block is still open; the first value's 49 fragments are written, none of
        // the second's.
        assert_eq!(fragments.map.len(), 49);
        assert_eq!(fragments.total(), (3 << 20) + 49 * TRAILER_LEN as u64);
    }

    #[test]
    fn values_past_their_column_limit_are_rejected() {
        let keys = keys(1);
        let too_long = vec![0u8; MAX_LARGE_VALUE_LEN + 1];
        let not_inline = vec![0u8; MAX_INLINE_VALUE_LEN + 1];
        for (value, hint) in [
            (&too_long, BytesHint::Text),
            (&not_inline, BytesHint::LowCardinality),
        ] {
            let values = [value.as_slice()];
            let mut fragments = Fragments::default();
            let mut builder = ChunkBuilder::new(
                &mut fragments,
                ChunkKind::L0Base,
                0,
                None,
                &[KEY_COLUMN, bytes(hint, false)],
                &crate::budget::test_account(),
            )
            .unwrap();
            let error = builder
                .append(&[
                    Column {
                        values: Values::I64(&keys),
                        validity: None,
                    },
                    Column {
                        values: Values::Bytes(&values),
                        validity: None,
                    },
                ])
                .unwrap_err();
            assert!(matches!(
                error,
                BuildError::ValueTooLong {
                    column: 1,
                    row: 0,
                    ..
                }
            ));
        }
    }

    #[test]
    fn layout_mistakes_are_local_errors() {
        let mut fragments = Fragments::default();
        let mut layout_error = |layout: &[ColumnLayout]| {
            ChunkBuilder::new(
                &mut fragments,
                ChunkKind::L0Base,
                0,
                None,
                layout,
                &crate::budget::test_account(),
            )
            .err()
            .unwrap()
        };
        let errors = [
            layout_error(&[]),
            layout_error(&[int(IntType::I64, true)]),
            layout_error(&[SUMMED]),
            layout_error(&[
                KEY_COLUMN,
                ColumnLayout {
                    ty: PhysicalType::Int {
                        ty: IntType::U32,
                        summed: true,
                    },
                    nullable: false,
                },
            ]),
            layout_error(&[
                KEY_COLUMN,
                bytes(BytesHint::Fixed(MAX_INLINE_VALUE_LEN as u16 + 1), false),
            ]),
            layout_error(&[KEY_COLUMN, bytes(BytesHint::Fixed(0), false)]),
        ];
        assert!(matches!(errors[0], BuildError::NoColumns));
        assert!(matches!(errors[1], BuildError::KeyColumn));
        assert!(matches!(errors[2], BuildError::KeyColumn));
        assert!(matches!(
            errors[3],
            BuildError::UnstorableColumn { column: 1, .. }
        ));
        assert!(matches!(
            errors[4],
            BuildError::UnstorableColumn { column: 1, .. }
        ));
        assert!(matches!(
            errors[5],
            BuildError::UnstorableColumn { column: 1, .. }
        ));
        for error in errors {
            assert_eq!(error.kind(), ErrorKind::Local);
        }
    }

    #[test]
    fn batch_mistakes_are_local_errors_that_leave_the_builder_as_it_was() {
        let keys = keys(2);
        let short = [1u64];
        let two = [1u64, 2];
        let wrong_width: [&[u8]; 2] = [b"abc", b"ab"];
        let right_width: [&[u8]; 2] = [b"abc", b"abd"];
        let layout = [KEY_COLUMN, SUMMED, bytes(BytesHint::Fixed(3), false)];
        let mut fragments = Fragments::default();
        let mut builder = ChunkBuilder::new(
            &mut fragments,
            ChunkKind::L0Base,
            0,
            None,
            &layout,
            &crate::budget::test_account(),
        )
        .unwrap();
        let key = Column {
            values: Values::I64(&keys),
            validity: None,
        };
        let summed = Column {
            values: Values::U64(&two),
            validity: None,
        };
        let fixed = Column {
            values: Values::Bytes(&right_width),
            validity: None,
        };
        let mistakes = [
            builder.append(&[key, summed]).unwrap_err(),
            builder
                .append(&[
                    key,
                    Column {
                        values: Values::U64(&short),
                        validity: None,
                    },
                    fixed,
                ])
                .unwrap_err(),
            builder
                .append(&[
                    key,
                    Column {
                        values: Values::U64(&two),
                        validity: Some(&[true]),
                    },
                    fixed,
                ])
                .unwrap_err(),
            builder
                .append(&[
                    Column {
                        values: Values::I64(&keys),
                        validity: Some(&[true, true]),
                    },
                    summed,
                    fixed,
                ])
                .unwrap_err(),
            builder
                .append(&[
                    key,
                    Column {
                        values: Values::I64(&keys),
                        validity: None,
                    },
                    fixed,
                ])
                .unwrap_err(),
            builder
                .append(&[
                    key,
                    summed,
                    Column {
                        values: Values::Bytes(&wrong_width),
                        validity: None,
                    },
                ])
                .unwrap_err(),
        ];
        assert!(matches!(
            mistakes[0],
            BuildError::ColumnCount {
                expected: 3,
                actual: 2
            }
        ));
        assert!(matches!(
            mistakes[1],
            BuildError::ColumnLength { column: 1, .. }
        ));
        assert!(matches!(
            mistakes[2],
            BuildError::ValidityLength { column: 1, .. }
        ));
        assert!(matches!(mistakes[3], BuildError::Nulls { column: 0 }));
        assert!(matches!(mistakes[4], BuildError::ValuesType { column: 1 }));
        assert!(matches!(
            mistakes[5],
            BuildError::FixedWidth { row: 1, len: 2, .. }
        ));
        for error in mistakes {
            assert_eq!(error.kind(), ErrorKind::Local);
        }
        // Nothing of the refused batches was taken.
        builder.append(&[key, summed, fixed]).unwrap();
        let built = builder.finish().unwrap();
        assert_eq!(built.rows, 2);
    }

    #[test]
    fn a_chunk_without_rows_is_an_error() {
        let mut fragments = Fragments::default();
        let mut builder = ChunkBuilder::new(
            &mut fragments,
            ChunkKind::L0Base,
            0,
            None,
            &[KEY_COLUMN],
            &crate::budget::test_account(),
        )
        .unwrap();
        builder.append(&key_batch(&[])).unwrap();
        let error = builder.finish().unwrap_err();
        assert!(matches!(error, BuildError::RowCount { rows: 0 }));
        assert_eq!(error.kind(), ErrorKind::Local);
    }

    #[test]
    fn a_storage_failure_keeps_its_kind() {
        struct Failing;
        impl FragmentSink for Failing {
            fn put(&mut self, _: u32, _: &[u8]) -> Result<(), StorageError> {
                Err(StorageError::Failed {
                    source: "disk full".into(),
                })
            }
        }
        let keys = keys(3);
        let mut sink = Failing;
        let mut builder = ChunkBuilder::new(
            &mut sink,
            ChunkKind::L0Base,
            0,
            None,
            &[KEY_COLUMN],
            &crate::budget::test_account(),
        )
        .unwrap();
        builder.append(&key_batch(&keys)).unwrap();
        let error = builder.finish().unwrap_err();
        assert!(matches!(error, BuildError::WriteFragment { index: 1, .. }));
        assert_eq!(error.kind(), ErrorKind::BackendFailed);
    }

    #[test]
    #[should_panic(expected = "only consolidated chunks are compressed")]
    fn compressing_an_l0_chunk_is_a_programming_error() {
        let mut fragments = Fragments::default();
        let mut compressor = compress::new_compressor(&crate::budget::test_account()).unwrap();
        let _ = ChunkBuilder::new(
            &mut fragments,
            ChunkKind::L0Delta,
            0,
            Some(&mut compressor),
            &[KEY_COLUMN],
            &crate::budget::test_account(),
        );
    }
}

/// The planner's bound checked against chunks built for real: every planned range is built
/// and its stored size measured, independently of the `fits` that chose it.
mod planning {
    use super::*;

    fn build_text_chunk(keys: &[i64], texts: &[Vec<u8>]) -> BuiltChunk {
        let slices: Vec<&[u8]> = texts.iter().map(Vec::as_slice).collect();
        let layout = [KEY_COLUMN, bytes(BytesHint::Text, false)];
        let (fragments, built) = build_chunk(&layout, false, |builder| {
            builder
                .append(&[
                    Column {
                        values: Values::I64(keys),
                        validity: None,
                    },
                    Column {
                        values: Values::Bytes(&slices),
                        validity: None,
                    },
                ])
                .unwrap();
        });
        assert_eq!(built.bytes, fragments.total());
        built
    }

    /// A chunk planned to within one row of B, from values encoding cannot shrink: random
    /// keys stay raw, and every 16 KiB text value fills a block of its own, so there are as
    /// many blocks as the bound allows. The bound's per-block overhead is all needed here;
    /// what is left is mostly fragment 0, which the bound counts at its longest encoding
    /// (79 bytes a block entry) and which takes about 20 here.
    #[test]
    fn a_chunk_planned_up_to_b_still_builds() {
        let mut rng = Rng(17);
        let mut texts: Vec<Vec<u8>> = (0..250).map(|_| rng.noise(MAX_INLINE_VALUE_LEN)).collect();
        texts.extend((0..200).map(|_| rng.noise(1000)));
        let keys: Vec<i64> = texts.iter().map(|_| rng.next() as i64).collect();
        let costs: Vec<RowCost> = texts
            .iter()
            .map(|text| {
                int_cost(IntType::I64, false) + bytes_cost(text.len(), BytesHint::Text, false)
            })
            .collect();
        let ranges = plan_chunks(2, &costs).unwrap().collect::<Vec<_>>();
        // The first chunk ends among the 1000-byte rows, one row short of the bound.
        let first = ranges[0].clone();
        assert!(first.end > 250 && first.end < texts.len(), "{ranges:?}");
        let built = build_text_chunk(&keys[first.clone()], &texts[first]);
        assert!(built.bytes <= MAX_CHUNK_LEN);
        assert!(
            built.bytes > MAX_CHUNK_LEN - 24 * 1024,
            "{} bytes leave too much room for the test to be tight",
            built.bytes
        );
        for range in &ranges[1..] {
            let built = build_text_chunk(&keys[range.clone()], &texts[range.clone()]);
            assert!(built.bytes <= MAX_CHUNK_LEN);
        }
    }

    /// How a generated column's values are drawn.
    #[derive(Clone, Copy, Debug)]
    enum Fill {
        /// Incompressible values: random integers, random bytes of random lengths up to
        /// `max_len` (a fixed column's width), and nulls in one row of four of a nullable
        /// column.
        Noise { max_len: usize },
        /// Every row null.
        Null,
        /// Text: short random values, and one row in 40 a large value of up to 600 KB.
        MixedLarge,
    }

    #[derive(Clone, Copy, Debug)]
    struct GenColumn {
        layout: ColumnLayout,
        fill: Fill,
    }

    /// A generated column's values for every row.
    enum GenValues {
        I64(Vec<i64>),
        U64(Vec<u64>),
        U32(Vec<u32>),
        Bytes(Vec<Vec<u8>>),
    }

    struct Generated {
        columns: Vec<GenColumn>,
        values: Vec<GenValues>,
        validity: Vec<Option<Vec<bool>>>,
        costs: Vec<RowCost>,
    }

    /// Generates rows of `columns` (after the key) until they cost about two and a half
    /// chunks, or `max_rows`.
    fn generate(columns: &[GenColumn], seed: u64, max_rows: usize) -> Generated {
        let mut rng = Rng(seed | 1);
        let columns: Vec<GenColumn> = std::iter::once(GenColumn {
            layout: KEY_COLUMN,
            fill: Fill::Noise { max_len: 0 },
        })
        .chain(columns.iter().copied())
        .collect();
        let mut values: Vec<GenValues> = columns
            .iter()
            .map(|column| match column.layout.ty {
                PhysicalType::Int {
                    ty: IntType::I64, ..
                } => GenValues::I64(Vec::new()),
                PhysicalType::Int {
                    ty: IntType::U64, ..
                } => GenValues::U64(Vec::new()),
                PhysicalType::Int {
                    ty: IntType::U32, ..
                } => GenValues::U32(Vec::new()),
                PhysicalType::Bytes(_) => GenValues::Bytes(Vec::new()),
            })
            .collect();
        let mut validity: Vec<Option<Vec<bool>>> = columns
            .iter()
            .map(|column| column.layout.nullable.then(Vec::new))
            .collect();
        let mut costs = Vec::new();
        let mut total = 0u64;
        while costs.len() < max_rows && total < 5 * MAX_CHUNK_LEN / 2 {
            let mut cost = RowCost::default();
            for (index, column) in columns.iter().enumerate() {
                let nullable = column.layout.nullable;
                let valid = match column.fill {
                    Fill::Null => false,
                    _ => !nullable || !rng.chance(4),
                };
                if let Some(validity) = &mut validity[index] {
                    validity.push(valid);
                }
                match &mut values[index] {
                    GenValues::I64(values) => {
                        values.push(if index == 0 {
                            costs.len() as i64
                        } else {
                            rng.next() as i64
                        });
                        cost = cost + int_cost(IntType::I64, nullable);
                    }
                    GenValues::U64(values) => {
                        values.push(rng.next());
                        cost = cost + int_cost(IntType::U64, nullable);
                    }
                    GenValues::U32(values) => {
                        values.push(rng.next() as u32);
                        cost = cost + int_cost(IntType::U32, nullable);
                    }
                    GenValues::Bytes(values) => {
                        let PhysicalType::Bytes(hint) = column.layout.ty else {
                            unreachable!();
                        };
                        let len = match (hint, column.fill) {
                            (BytesHint::Fixed(width), _) => usize::from(width),
                            (_, Fill::MixedLarge) if rng.chance(40) => {
                                MAX_INLINE_VALUE_LEN + 1 + rng.below(600_000) as usize
                            }
                            (_, Fill::MixedLarge) => rng.below(200) as usize,
                            (_, Fill::Noise { max_len }) => rng.below(max_len as u64 + 1) as usize,
                            (_, Fill::Null) => 0,
                        };
                        values.push(rng.noise(len));
                        cost = cost + bytes_cost(if valid { len } else { 0 }, hint, nullable);
                    }
                }
            }
            total += cost.inline + cost.large;
            costs.push(cost);
        }
        Generated {
            columns,
            values,
            validity,
            costs,
        }
    }

    impl Generated {
        /// Builds the rows `rows` as one chunk, in batches of up to 97 rows.
        fn build(&self, rows: Range<usize>) -> (Fragments, BuiltChunk) {
            let layout: Vec<ColumnLayout> =
                self.columns.iter().map(|column| column.layout).collect();
            let byte_slices: Vec<Option<Vec<&[u8]>>> = self
                .values
                .iter()
                .map(|values| match values {
                    GenValues::Bytes(values) => {
                        Some(values[rows.clone()].iter().map(Vec::as_slice).collect())
                    }
                    _ => None,
                })
                .collect();
            build_chunk(&layout, false, |builder| {
                for start in (0..rows.len()).step_by(97) {
                    let batch = start..rows.len().min(start + 97);
                    let at = rows.start + batch.start..rows.start + batch.end;
                    let columns: Vec<Column<'_>> = self
                        .values
                        .iter()
                        .zip(&self.validity)
                        .zip(&byte_slices)
                        .map(|((values, validity), slices)| Column {
                            values: match values {
                                GenValues::I64(values) => Values::I64(&values[at.clone()]),
                                GenValues::U64(values) => Values::U64(&values[at.clone()]),
                                GenValues::U32(values) => Values::U32(&values[at.clone()]),
                                GenValues::Bytes(_) => {
                                    Values::Bytes(&slices.as_ref().unwrap()[batch.clone()])
                                }
                            },
                            validity: validity.as_ref().map(|validity| &validity[at.clone()]),
                        })
                        .collect();
                    builder.append(&columns).unwrap();
                }
            })
        }
    }

    /// Plans the generated rows, builds every planned chunk, and measures it.
    fn check_planned(columns: &[GenColumn], seed: u64, max_rows: usize) {
        let generated = generate(columns, seed, max_rows);
        let ranges = plan_chunks(generated.columns.len(), &generated.costs)
            .unwrap()
            .collect::<Vec<_>>();
        for range in ranges {
            let (fragments, built) = generated.build(range.clone());
            assert!(
                built.bytes <= MAX_CHUNK_LEN,
                "rows {range:?} of {columns:?} take {} bytes",
                built.bytes
            );
            assert_eq!(built.bytes, fragments.total());
            assert!(fragments.largest.get() <= MAX_DATA_FRAGMENT_LEN);
            read_everything(&fragments).unwrap();
        }
    }

    fn column(layout: ColumnLayout, fill: Fill) -> GenColumn {
        GenColumn { layout, fill }
    }

    fn noise(max_len: usize) -> Fill {
        Fill::Noise { max_len }
    }

    #[test]
    fn fixed_width_columns() {
        let columns = [
            column(
                bytes(BytesHint::Fixed(MAX_INLINE_VALUE_LEN as u16), false),
                noise(0),
            ),
            column(bytes(BytesHint::Fixed(1000), true), noise(0)),
            column(bytes(BytesHint::Fixed(1), false), noise(0)),
        ];
        check_planned(&columns, 1, usize::MAX);
    }

    #[test]
    fn all_null_columns() {
        let types = [
            int(IntType::I64, true),
            SUMMED,
            int(IntType::U32, true),
            bytes(BytesHint::LowCardinality, true),
            bytes(BytesHint::Text, true),
            bytes(BytesHint::Fixed(64), true),
        ];
        let columns: Vec<GenColumn> = types
            .iter()
            .cycle()
            .take(MAX_COLUMNS - 1)
            .map(|&layout| column(layout, Fill::Null))
            .collect();
        check_planned(&columns, 2, 3 * MAX_ROWS / 2);
    }

    #[test]
    fn mixed_large_values() {
        let columns = [
            column(bytes(BytesHint::Text, true), Fill::MixedLarge),
            column(bytes(BytesHint::Text, false), Fill::MixedLarge),
            column(bytes(BytesHint::LowCardinality, false), noise(300)),
            column(SUMMED, noise(0)),
        ];
        check_planned(&columns, 3, usize::MAX);
    }

    #[test]
    fn many_columns_of_every_type_and_hint() {
        let types = [
            (int(IntType::I64, false), noise(0)),
            (SUMMED, noise(0)),
            (int(IntType::U32, true), noise(0)),
            (
                bytes(BytesHint::LowCardinality, true),
                noise(MAX_INLINE_VALUE_LEN),
            ),
            (bytes(BytesHint::Text, false), noise(MAX_INLINE_VALUE_LEN)),
            (bytes(BytesHint::Fixed(17), true), noise(0)),
            (bytes(BytesHint::Text, true), Fill::MixedLarge),
        ];
        let columns: Vec<GenColumn> = types
            .iter()
            .cycle()
            .take(MAX_COLUMNS - 1)
            .map(|&(layout, fill)| column(layout, fill))
            .collect();
        check_planned(&columns, 4, usize::MAX);
    }

    fn gen_column() -> impl Strategy<Value = GenColumn> {
        let hint = prop_oneof![
            Just(BytesHint::LowCardinality),
            Just(BytesHint::Text),
            prop_oneof![
                Just(1u16),
                Just(17),
                Just(4096),
                Just(MAX_INLINE_VALUE_LEN as u16)
            ]
            .prop_map(BytesHint::Fixed),
        ];
        let ty = prop_oneof![
            Just(PhysicalType::Int {
                ty: IntType::I64,
                summed: false
            }),
            any::<bool>().prop_map(|summed| PhysicalType::Int {
                ty: IntType::U64,
                summed
            }),
            Just(PhysicalType::Int {
                ty: IntType::U32,
                summed: false
            }),
            hint.prop_map(PhysicalType::Bytes),
        ];
        let fill = prop_oneof![
            4 => prop_oneof![Just(0usize), Just(100), Just(MAX_INLINE_VALUE_LEN)]
                .prop_map(|max_len| Fill::Noise { max_len }),
            1 => Just(Fill::Null),
            1 => Just(Fill::MixedLarge),
        ];
        (ty, any::<bool>(), fill).prop_map(|(ty, nullable, fill)| {
            let fill = match (ty, fill) {
                (_, Fill::Null) if !nullable => Fill::Noise { max_len: 0 },
                (PhysicalType::Bytes(BytesHint::Text), fill) => fill,
                (_, Fill::MixedLarge) => Fill::Noise { max_len: 100 },
                (_, fill) => fill,
            };
            GenColumn {
                layout: ColumnLayout { ty, nullable },
                fill,
            }
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(12))]

        /// Random layouts of up to 64 columns: every planned range builds within B.
        #[test]
        fn planned_chunks_build_within_b(
            columns in prop::collection::vec(gen_column(), 0..MAX_COLUMNS),
            seed in any::<u64>(),
        ) {
            check_planned(&columns, seed, 2 * MAX_ROWS);
        }
    }
}

mod hostile {
    use super::*;

    /// A chunk with every column type, several blocks, compressed blocks and a large value
    /// over two fragments.
    fn chunk() -> Fragments {
        let table = Table::generate(5000, 99, &[DATA_PAYLOAD_LEN + 10]);
        table.build(true).0
    }

    fn corrupted(fragments: &Fragments) -> ReadError {
        let error = read_everything(fragments).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Corrupted, "{error}");
        error
    }

    /// Why opening the chunk fails once `edit` changed its directory.
    fn opening(edit: impl FnOnce(&mut DirectoryRecord)) -> ReadError {
        let mut fragments = chunk();
        fragments.edit_directory(edit);
        let error = open_chunk(&fragments).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Corrupted, "{error}");
        error
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn flipped_or_truncated_fragments_are_corrupted(
            pick in any::<prop::sample::Index>(),
            at in any::<prop::sample::Index>(),
            flip in 1u8..=255,
            truncate in any::<bool>(),
        ) {
            let mut fragments = chunk();
            let index = *pick.get(&fragments.map.keys().copied().collect::<Vec<_>>());
            let fragment = fragments.map.get_mut(&index).unwrap();
            if truncate {
                fragment.truncate(at.index(fragment.len()));
            } else {
                let at = at.index(fragment.len());
                fragment[at] ^= flip;
            }
            corrupted(&fragments);
        }

        /// Damage behind a valid checksum reaches the parsers and decoders: they may accept
        /// a changed value, but never panic, and anything they reject is corruption.
        #[test]
        fn damage_behind_a_valid_checksum_never_panics(
            pick in any::<prop::sample::Index>(),
            flips in prop::collection::vec((any::<prop::sample::Index>(), 1u8..=255), 1..4),
        ) {
            let mut fragments = chunk();
            let index = *pick.get(&fragments.map.keys().copied().collect::<Vec<_>>());
            fragments.reseal(index, |content| {
                for (at, flip) in &flips {
                    let at = at.index(content.len());
                    content[at] ^= flip;
                }
            });
            if let Err(error) = read_everything(&fragments) {
                prop_assert_eq!(error.kind(), ErrorKind::Corrupted, "{}", error);
            }
        }
    }

    #[test]
    fn huge_counts_are_rejected() {
        let rows = opening(|record| record.header.rows = u32::MAX);
        assert!(
            matches!(
                rows,
                ReadError::InvalidHeader {
                    problem: HeaderProblem::Rows(u32::MAX)
                }
            ),
            "{rows}"
        );
        let fragments = opening(|record| record.header.fragments = u32::MAX);
        assert!(
            matches!(
                fragments,
                ReadError::InvalidHeader {
                    problem: HeaderProblem::Fragments(u32::MAX)
                }
            ),
            "{fragments}"
        );
    }

    /// A directory of `columns`, `blocks` and `large` records, not otherwise valid.
    fn directory_of(columns: usize, blocks: usize, large: usize) -> DirectoryRecord {
        let mut record = chunk().directory();
        record.columns.resize(columns, record.columns[0]);
        record.blocks.resize(blocks, record.blocks[0]);
        record.large.resize(large, record.large[0]);
        record
    }

    fn decodes(record: &DirectoryRecord) -> bool {
        let mut content = Vec::new();
        record.write(&mut content);
        match DirectoryRecord::read(&content) {
            Ok(decoded) => {
                assert_eq!(&decoded, record);
                true
            }
            Err(error) => {
                assert!(
                    matches!(error, ReadError::DecodeDirectory { .. }),
                    "{error}"
                );
                assert_eq!(error.kind(), ErrorKind::Corrupted);
                false
            }
        }
    }

    /// Each list of fragment 0 stops decoding past its limit, before it holds more records:
    /// a hostile length is never what the reader allocates by.
    #[test]
    fn lists_past_their_limits_fail_to_decode() {
        assert!(decodes(&directory_of(
            MAX_COLUMNS,
            MAX_BLOCKS,
            MAX_LARGE_VALUES
        )));
        assert!(!decodes(&directory_of(
            MAX_COLUMNS + 1,
            MAX_BLOCKS,
            MAX_LARGE_VALUES
        )));
        assert!(!decodes(&directory_of(
            MAX_COLUMNS,
            MAX_BLOCKS + 1,
            MAX_LARGE_VALUES
        )));
        assert!(!decodes(&directory_of(
            MAX_COLUMNS,
            MAX_BLOCKS,
            MAX_LARGE_VALUES + 1
        )));

        // A length the input cannot hold gives no length hint (postcard's `size_hint` is
        // `None` past the remaining bytes), so the list fails before it decodes a record,
        // whether its records are fewer than the limit or more: postcard reports the
        // visitor's error as `SerdeDeCustom`.
        let unhinted = |records: usize| {
            let record = chunk().directory();
            let block = record.blocks[0];
            let mut content = Vec::new();
            DirectoryRecord {
                blocks: Vec::new(),
                large: Vec::new(),
                ..record
            }
            .write(&mut content);
            // The empty block list's length, then the empty large-value list's.
            content.truncate(content.len() - 2);
            content.extend(postcard::to_stdvec(&u64::from(u32::MAX)).unwrap());
            for _ in 0..records {
                content.extend(postcard::to_stdvec(&block).unwrap());
            }
            match DirectoryRecord::read(&content).unwrap_err() {
                ReadError::DecodeDirectory { source } => source,
                error => panic!("{error}"),
            }
        };
        for records in [3, MAX_BLOCKS, MAX_BLOCKS + 1] {
            assert_eq!(unhinted(records), postcard::Error::SerdeDeCustom);
        }
    }

    #[test]
    fn the_prefix_and_what_follows_the_directory_are_checked() {
        let mut fragments = chunk();
        fragments.reseal(0, |content| content[4] = 2);
        assert!(matches!(
            corrupted(&fragments),
            ReadError::InvalidHeader {
                problem: HeaderProblem::Version(2)
            }
        ));
        let mut fragments = chunk();
        fragments.reseal(0, |content| content[0] = b'X');
        assert!(matches!(
            corrupted(&fragments),
            ReadError::InvalidHeader {
                problem: HeaderProblem::Magic
            }
        ));
        // Too short for the prefix: no manifest entry records such a fragment 0, and parsing
        // one fails.
        let mut fragments = chunk();
        fragments.reseal(0, |content| content.truncate(3));
        assert!(matches!(
            corrupted(&fragments),
            ReadError::RecordedFragment0 { recorded: 7 }
        ));
        assert!(matches!(
            DirectoryRecord::read(&fragments.map[&0][..3]),
            Err(ReadError::InvalidHeader {
                problem: HeaderProblem::Magic
            })
        ));
        let mut fragments = chunk();
        fragments.reseal(0, |content| content.extend_from_slice(&[0; 3]));
        assert!(matches!(
            corrupted(&fragments),
            ReadError::InvalidHeader {
                problem: HeaderProblem::TrailingBytes(3)
            }
        ));
    }

    /// Changes block entry `entry` and returns why opening fails.
    fn entry_problem(entry: usize, edit: impl FnOnce(&mut BlockRecord)) -> BlockProblem {
        match opening(|record| edit(&mut record.blocks[entry])) {
            ReadError::InvalidBlockEntry {
                entry: found,
                problem,
            } if found as usize == entry => problem,
            error => panic!("{error}"),
        }
    }

    #[test]
    fn invalid_directory_entries_are_rejected_at_open() {
        // An offset past any fragment, and an unaligned one.
        let past = entry_problem(0, |block| block.offset = DATA_PAYLOAD_LEN as u32 + 4);
        assert!(matches!(past, BlockProblem::Extent { .. }), "{past:?}");
        let unaligned = entry_problem(0, |block| block.offset = 3);
        assert!(
            matches!(unaligned, BlockProblem::Extent { offset: 3, .. }),
            "{unaligned:?}"
        );
        // Overlapping rows: the second block of column 0 starts at row 0 again.
        let overlap = entry_problem(1, |block| block.first_row = 0);
        assert!(
            matches!(overlap, BlockProblem::RowRange { first: 0, .. }),
            "{overlap:?}"
        );
        // A fragment the chunk does not have.
        assert_eq!(
            entry_problem(0, |block| block.fragment = 1000),
            BlockProblem::Fragment(1000)
        );
        // A huge uncompressed length.
        assert_eq!(
            entry_problem(0, |block| block.uncompressed_len = u32::MAX),
            BlockProblem::UncompressedLength(u32::MAX)
        );
        // Another column's entry.
        assert_eq!(
            entry_problem(0, |block| block.column = 1),
            BlockProblem::Column(1)
        );
    }

    /// The blocks of every column cover the chunk's rows; a header claiming one more row
    /// leaves column 0 a row short.
    #[test]
    fn blocks_that_cover_other_rows_than_the_header_are_named() {
        let rows = chunk().directory().header.rows;
        let error = opening(|record| record.header.rows += 1);
        assert!(
            matches!(
                error,
                ReadError::InvalidColumn {
                    column: 0,
                    problem: ColumnProblem::Rows { expected, actual }
                } if expected == rows + 1 && actual == rows
            ),
            "{error}"
        );
    }

    #[test]
    fn invalid_large_entries_are_rejected_at_open() {
        let too_long = opening(|record| record.large[0].len = u32::MAX);
        assert!(
            matches!(
                too_long,
                ReadError::InvalidLargeEntry {
                    entry: 0,
                    problem: LargeProblem::Length(u32::MAX)
                }
            ),
            "{too_long}"
        );
        // The value needs two fragments.
        let too_few = opening(|record| record.large[0].fragments = 1);
        assert!(
            matches!(
                too_few,
                ReadError::InvalidLargeEntry {
                    entry: 0,
                    problem: LargeProblem::Fragments { count: 1, .. }
                }
            ),
            "{too_few}"
        );
    }

    /// A chunk with two large values of one fragment each.
    fn two_large_values() -> Fragments {
        let table = Table::generate(500, 31, &[MAX_INLINE_VALUE_LEN + 1, 20_000]);
        let fragments = table.build(false).0;
        let record = fragments.directory();
        assert_eq!(record.large.len(), 2);
        assert!(record.large.iter().all(|entry| entry.fragments == 1));
        fragments
    }

    /// Every fragment has one owner: two large values never share one, and neither do a
    /// block and a large value, even when every entry is valid on its own.
    #[test]
    fn a_fragment_has_one_owner() {
        let mut shared = two_large_values();
        let first = shared.directory().large[0];
        shared.edit_directory(|record| {
            record.large[1].first_fragment = first.first_fragment;
            record.large[1].len = first.len;
        });
        let error = open_chunk(&shared).unwrap_err();
        assert!(
            matches!(
                error,
                ReadError::InvalidLargeEntry {
                    entry: 1,
                    problem: LargeProblem::Shared(fragment)
                } if fragment == first.first_fragment
            ),
            "{error}"
        );

        let mut with_block = two_large_values();
        with_block.edit_directory(|record| {
            record.blocks[0].fragment = first.first_fragment;
            record.blocks[0].offset = 0;
        });
        let error = open_chunk(&with_block).unwrap_err();
        assert!(
            matches!(
                error,
                ReadError::InvalidLargeEntry {
                    entry: 0,
                    problem: LargeProblem::Shared(fragment)
                } if fragment == first.first_fragment
            ),
            "{error}"
        );

        // A fragment index that nothing uses.
        let mut unowned = chunk();
        let fragments = unowned.directory().header.fragments;
        unowned.edit_directory(|record| record.header.fragments += 1);
        let error = open_chunk(&unowned).unwrap_err();
        assert!(
            matches!(
                error,
                ReadError::InvalidHeader {
                    problem: HeaderProblem::UnownedFragment(index)
                } if index == fragments
            ),
            "{error}"
        );
    }

    /// Places `encoded` in the data fragment `data` and returns its directory entry.
    fn place(
        data: &mut crate::budget::BudgetVec<u8>,
        column: u16,
        rows: u32,
        encoded: &Encoded<'_>,
        stats: IntStats,
    ) -> BlockRecord {
        let offset = data.len().next_multiple_of(8);
        data.resize(offset, 0);
        encoded.write(data);
        BlockRecord {
            column,
            codec: encoded.codec as u8,
            encoding: encoded.encoding.tag(),
            first_row: 0,
            rows,
            fragment: 1,
            offset: offset as u32,
            stored_len: encoded.stored_len() as u32,
            uncompressed_len: encoded.uncompressed_len as u32,
            stats,
        }
    }

    /// A chunk built by hand of `rows` rows of 40,000-byte text values, each a large value
    /// with a fragment of its own. Every checksum is valid, and every block and large value
    /// passes its own checks, whatever the rows. With 200 rows, it is the chunk the builder
    /// refuses to write: more than 8 MB.
    fn hand_built_chunk(rows: u32) -> Fragments {
        const LEN: usize = 40_000;
        let mut fragments = Fragments::default();
        let mut scratch = EncodeScratch::new(
            &crate::budget::test_account(),
            crate::codec::block::ScratchCaps::default(),
        );
        let mut data = crate::budget::test_account()
            .buffer(MAX_DATA_FRAGMENT_LEN)
            .unwrap();

        let keys: Vec<i64> = (0..i64::from(rows)).collect();
        let key_stats = IntStats {
            range: Some(IntRange::Signed {
                min: 0,
                max: i64::from(rows) - 1,
            }),
            sum: None,
            presence: None,
        };
        let encoded = encode_ints(&keys, None, None, &mut scratch).unwrap();
        let key_block = place(&mut data, 0, rows, &encoded, key_stats);

        let values = vec![&[][..]; rows as usize];
        let large: Vec<(u32, u32)> = (0..rows).map(|row| (row, row)).collect();
        let encoded = encode_bytes(
            &values[..],
            None,
            &large,
            BytesHint::Text,
            None,
            &mut scratch,
        )
        .unwrap();
        let text_block = place(&mut data, 1, rows, &encoded, IntStats::default());
        checksum::seal(&mut data);
        fragments.put(1, &data).unwrap();

        let mut value = crate::budget::test_account()
            .buffer(LEN + TRAILER_LEN)
            .unwrap();
        value.resize(LEN, b'x');
        checksum::seal(&mut value);
        for row in 0..rows {
            fragments.put(2 + row, &value).unwrap();
        }

        let directory = DirectoryRecord {
            header: HeaderRecord {
                kind: ChunkKind::L0Base as u8,
                fingerprint: FINGERPRINT,
                rows,
                fragments: 2 + rows,
                key_min: 0,
                key_max: i64::from(rows) - 1,
            },
            columns: vec![
                ColumnRecord::new(KEY_COLUMN.ty, false, 1, key_stats),
                ColumnRecord::new(
                    PhysicalType::Bytes(BytesHint::Text),
                    false,
                    1,
                    IntStats::default(),
                ),
            ],
            blocks: vec![key_block, text_block],
            large: (0..rows)
                .map(|row| LargeEntry {
                    first_fragment: 2 + row,
                    fragments: 1,
                    len: LEN as u32,
                })
                .collect(),
        };
        let mut fragment0 = crate::budget::test_account()
            .buffer(crate::storage::MAX_FRAGMENT_LEN)
            .unwrap();
        directory.write(&mut fragment0);
        checksum::seal(&mut fragment0);
        fragments.put(0, &fragment0).unwrap();
        fragments
    }

    #[test]
    fn a_chunk_past_b_is_rejected_at_open_whatever_its_checksums() {
        let fragments = hand_built_chunk(200);
        assert!(fragments.total() > 8_000_000);
        let error = open_chunk(&fragments).unwrap_err();
        assert!(
            matches!(
                error,
                ReadError::InvalidHeader {
                    problem: HeaderProblem::ChunkLength(total)
                } if total == fragments.total()
            ),
            "{error}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);

        // With half the rows, the same chunk is within B, and opens and reads: only its
        // total was wrong.
        let fragments = hand_built_chunk(100);
        assert!(fragments.total() <= MAX_CHUNK_LEN);
        read_everything(&fragments).unwrap();
    }

    /// The directory fixes each fragment's length: a data fragment ends at its furthest
    /// block, a large-value fragment holds its share of the value. A longer one is fetched by
    /// that length, so the source refuses it; a shorter one is found once lent.
    #[test]
    fn a_fragment_of_another_length_than_its_directory_gives_is_rejected_when_read() {
        let mut fragments = chunk();
        let reader = open_chunk(&fragments).unwrap();
        let block = reader.blocks(0).unwrap()[0];
        fragments.reseal(block.fragment, |content| content.extend_from_slice(&[0; 8]));
        assert!(matches!(
            corrupted(&fragments),
            ReadError::ReadFragment {
                index,
                source: StorageError::Corrupted { .. }
            } if index == block.fragment
        ));
        let mut fragments = chunk();
        fragments.reseal(block.fragment, |content| {
            content.truncate(content.len() - 8)
        });
        let expected = fragments.map[&block.fragment].len() - TRAILER_LEN + 8;
        assert!(matches!(
            corrupted(&fragments),
            ReadError::ContentLength { index, expected: found, .. }
                if index == block.fragment && found == expected
        ));

        let mut fragments = chunk();
        let large = fragments.directory().large[0];
        fragments.reseal(large.first_fragment, |content| {
            content.truncate(content.len() - 1)
        });
        assert!(matches!(
            corrupted(&fragments),
            ReadError::ContentLength { index, .. } if index == large.first_fragment
        ));
    }

    #[test]
    fn a_block_past_its_fragment_is_rejected_when_read() {
        let mut fragments = chunk();
        let reader = open_chunk(&fragments).unwrap();
        let content_len = |block: &BlockEntry| fragments.map[&block.fragment].len() - TRAILER_LEN;
        // The block in the shortest data fragment, so that moving it past that fragment's
        // end stays within the largest fragment and passes the checks at open.
        let (entry, block) = (0..reader.columns().len() as u16)
            .flat_map(|column| reader.blocks(column).unwrap())
            .copied()
            .enumerate()
            .min_by_key(|(_, block)| content_len(block))
            .unwrap();
        let offset = (content_len(&block) as u32).next_multiple_of(8);
        fragments.edit_directory(|record| record.blocks[entry].offset = offset);
        assert!(matches!(
            corrupted(&fragments),
            ReadError::ContentLength { index, .. } if index == block.fragment
        ));
    }

    /// Puts `encoded` in place of block entry `entry`, the last block of its data fragment
    /// (so it can change length), and seals the fragment and the directory again.
    pub(super) fn replace_last_block(
        fragments: &mut Fragments,
        entry: usize,
        encoded: &Encoded<'_>,
    ) {
        let mut stored = crate::budget::test_account()
            .buffer(encoded.stored_len())
            .unwrap();
        encoded.write(&mut stored);
        replace_last_stored(fragments, entry, &stored, encoded.uncompressed_len);
    }

    /// Like [`replace_last_block`], with a block as it is stored: its codec and encoding tags,
    /// then its payload of `uncompressed_len` bytes before compression.
    pub(super) fn replace_last_stored(
        fragments: &mut Fragments,
        entry: usize,
        stored: &[u8],
        uncompressed_len: usize,
    ) {
        let blocks = fragments.directory().blocks;
        let block = blocks[entry];
        let last = blocks
            .iter()
            .filter(|other| other.fragment == block.fragment)
            .map(|other| other.offset)
            .max();
        assert_eq!(
            last,
            Some(block.offset),
            "not the last block of its fragment"
        );
        fragments.reseal(block.fragment, |content| {
            content.truncate(block.offset as usize);
            content.extend_from_slice(stored);
        });
        fragments.edit_directory(|record| {
            let block = &mut record.blocks[entry];
            block.codec = stored[0];
            block.encoding = stored[1];
            block.stored_len = stored.len() as u32;
            block.uncompressed_len = uncompressed_len as u32;
        });
    }

    /// Only a text column has large values. A block of another bytes column that sets the
    /// large flag and refers to a valid large value is damage, even with no inline value for
    /// the width check of a fixed column to see.
    #[test]
    fn only_a_text_column_refers_to_large_values() {
        let large = vec![9u8; 20_000];
        let fixed = [5u8; 17];
        for hint in [BytesHint::Fixed(17), BytesHint::LowCardinality] {
            let layout = [
                KEY_COLUMN,
                bytes(BytesHint::Text, false),
                bytes(hint, false),
            ];
            let (mut fragments, _) = build_chunk(&layout, false, |builder| {
                builder
                    .append(&[
                        Column {
                            values: Values::I64(&[1]),
                            validity: None,
                        },
                        Column {
                            values: Values::Bytes(&[&large[..]]),
                            validity: None,
                        },
                        Column {
                            values: Values::Bytes(&[&fixed[..]]),
                            validity: None,
                        },
                    ])
                    .unwrap();
            });
            read_everything(&fragments).unwrap();
            assert_eq!(fragments.directory().large.len(), 1);

            // The column's one row, as a reference to the text column's large value.
            let mut scratch = EncodeScratch::new(
                &crate::budget::test_account(),
                crate::codec::block::ScratchCaps::default(),
            );
            let encoded =
                encode_bytes(&[&b""[..]][..], None, &[(0, 0)], hint, None, &mut scratch).unwrap();
            replace_last_block(&mut fragments, 2, &encoded);
            let error = corrupted(&fragments);
            assert!(
                matches!(
                    error,
                    ReadError::DecodeBlock {
                        column: 2,
                        block: 0,
                        source: DecodeError::LargeOutsideText { hint: found }
                    } if found == hint
                ),
                "{error}"
            );
        }
    }

    /// A fixed column's values are of its width, however its blocks store them. A block
    /// stores the length its present values share; one stored at another width than the
    /// column's is damage, which the reader reports instead of handing such values on.
    #[test]
    fn a_fixed_column_reads_only_values_of_its_width() {
        let keys = [1, 2, 3, 4];
        let values: Vec<Vec<u8>> = (0..4u8).map(|row| vec![row; 16]).collect();
        let slices: Vec<&[u8]> = values.iter().map(Vec::as_slice).collect();
        let layout = [KEY_COLUMN, bytes(BytesHint::Fixed(16), false)];
        let (mut fragments, _) = build_chunk(&layout, false, |builder| {
            builder
                .append(&[
                    Column {
                        values: Values::I64(&keys),
                        validity: None,
                    },
                    Column {
                        values: Values::Bytes(&slices),
                        validity: None,
                    },
                ])
                .unwrap();
        });
        read_everything(&fragments).unwrap();

        // The same values, one byte shorter, as a valid fixed block in the column's place.
        let short: Vec<&[u8]> = values.iter().map(|value| &value[..15]).collect();
        let mut scratch = EncodeScratch::new(
            &crate::budget::test_account(),
            crate::codec::block::ScratchCaps::default(),
        );
        let encoded = encode_bytes(
            &short[..],
            None,
            &[],
            BytesHint::Fixed(15),
            None,
            &mut scratch,
        )
        .unwrap();
        assert_eq!(encoded.encoding, Encoding::Bytes(BytesEncoding::Fixed));
        replace_last_block(&mut fragments, 1, &encoded);
        assert!(matches!(
            corrupted(&fragments),
            ReadError::DecodeBlock {
                column: 1,
                block: 0,
                source: DecodeError::ColumnWidth { len: 15, width: 16 }
            }
        ));

        // Blocks whose present values share no length still decode: an all-null block of a
        // fixed column, and a block of empty values of a low-cardinality one, both stored
        // as fixed blocks of width 0.
        let layout = [
            KEY_COLUMN,
            bytes(BytesHint::Fixed(16), true),
            bytes(BytesHint::LowCardinality, false),
        ];
        let empty: [&[u8]; 4] = [b"", b"", b"", b""];
        let (fragments, _) = build_chunk(&layout, false, |builder| {
            builder
                .append(&[
                    Column {
                        values: Values::I64(&keys),
                        validity: None,
                    },
                    Column {
                        values: Values::Bytes(&slices),
                        validity: Some(&[false; 4]),
                    },
                    Column {
                        values: Values::Bytes(&empty),
                        validity: None,
                    },
                ])
                .unwrap();
        });
        let reader = open_chunk(&fragments).unwrap();
        for column in [1, 2] {
            let encoding = reader.blocks(column).unwrap()[0].encoding;
            assert_eq!(encoding, Encoding::Bytes(BytesEncoding::Fixed));
        }
        let read = content(&fragments);
        assert_eq!(read.cells[1], vec![ReadBack::Null; 4]);
        assert_eq!(read.cells[2], vec![ReadBack::Bytes(Vec::new()); 4]);
    }

    #[test]
    fn decompression_bombs_and_wrong_lengths_are_rejected() {
        let fragments = chunk();
        let reader = open_chunk(&fragments).unwrap();
        let (entry, block) = reader
            .blocks(5)
            .unwrap()
            .iter()
            .copied()
            .enumerate()
            .find(|(_, block)| block.codec == Codec::Zstd)
            .unwrap();
        let entry = reader.columns()[5].first_block as usize + entry;

        let declared = MAX_BLOCK_LEN as u32 + 1;
        let bomb = opening(|record| record.blocks[entry].uncompressed_len = declared);
        assert!(matches!(
            bomb,
            ReadError::InvalidBlockEntry {
                problem: BlockProblem::UncompressedLength(len),
                ..
            } if len == declared
        ));

        for (declared, short) in [
            (block.uncompressed_len - 1, true),
            (block.uncompressed_len + 1, false),
        ] {
            let mut wrong = chunk();
            wrong.edit_directory(|record| record.blocks[entry].uncompressed_len = declared);
            match corrupted(&wrong) {
                ReadError::DecodeBlock {
                    column: 5, source, ..
                } => {
                    if short {
                        assert!(matches!(source, DecodeError::Decompress { .. }), "{source}");
                    } else {
                        assert!(
                            matches!(source, DecodeError::DecompressedLength { .. }),
                            "{source}"
                        );
                    }
                }
                error => panic!("{error}"),
            }
        }
    }

    #[test]
    fn a_missing_fragment_is_corrupted() {
        let mut fragments = chunk();
        let last = *fragments.map.keys().last().unwrap();
        fragments.map.remove(&last);
        assert!(matches!(
            corrupted(&fragments),
            ReadError::MissingFragment { index } if index == last
        ));
    }

    /// The kind of the header, and the tags of the column records and block entries, as
    /// fragment 0 stores them: the numbers are the format, not the enums that write them.
    #[test]
    fn the_tags_of_a_chunk_are_stored_as_the_format_numbers_them() {
        for (kind, tag) in [
            (ChunkKind::L0Base, 1),
            (ChunkKind::L0Delta, 2),
            (ChunkKind::Consolidated, 3),
        ] {
            assert_eq!(kind as u8, tag);
            assert_eq!(ChunkKind::from_repr(tag), Some(kind));
        }
        assert_eq!(ChunkKind::from_repr(0), None);
        assert_eq!(ChunkKind::from_repr(4), None);

        for (tag, byte) in [
            (ColumnTag::I64, 1),
            (ColumnTag::U64, 2),
            (ColumnTag::U32, 3),
            (ColumnTag::Bytes, 4),
        ] {
            assert_eq!(tag as u8, byte);
            assert_eq!(ColumnTag::from_repr(byte), Some(tag));
        }
        assert_eq!(ColumnTag::from_repr(0), None);
        assert_eq!(ColumnTag::from_repr(5), None);

        let record = chunk().directory();
        assert_eq!(record.header.kind, 3);
        let uncompressed = Table::generate(100, 1, &[]).build(false).0;
        assert_eq!(uncompressed.directory().header.kind, 1);

        // Per column: the type, the hint, and the width of a fixed hint.
        let columns: Vec<_> = record
            .columns
            .iter()
            .map(|column| (column.ty, column.hint, column.width))
            .collect();
        assert_eq!(
            columns,
            [
                (1, 0, 0),
                (2, 0, 0),
                (3, 0, 0),
                (1, 0, 0),
                (4, 1, 0),
                (4, 2, 0),
                (4, 3, 17),
            ]
        );

        // Per block entry: a codec tag, and an encoding tag of the column's kind.
        let mut zstd = 0;
        for (entry, block) in record.blocks.iter().enumerate() {
            assert!(block.codec <= 1, "entry {entry} has codec {}", block.codec);
            zstd += usize::from(block.codec == 1);
            let allowed = if block.column < 4 { 1..=5 } else { 16..=18 };
            assert!(
                allowed.contains(&block.encoding),
                "entry {entry} of column {} has encoding {}",
                block.column,
                block.encoding
            );
        }
        assert!(zstd > 0, "the chunk has no compressed block");
    }

    #[test]
    fn unknown_tags_are_rejected_at_open() {
        assert!(matches!(
            opening(|record| record.header.kind = 9),
            ReadError::InvalidHeader {
                problem: HeaderProblem::Kind(9)
            }
        ));
        // The type of column 1, and the hint of column 4.
        assert!(matches!(
            opening(|record| record.columns[1].ty = 9),
            ReadError::InvalidColumn {
                column: 1,
                problem: ColumnProblem::Type(9)
            }
        ));
        assert!(matches!(
            opening(|record| record.columns[4].hint = 9),
            ReadError::InvalidColumn {
                column: 4,
                problem: ColumnProblem::Hint(9)
            }
        ));
        // A summed column that is not a u64 column.
        assert!(matches!(
            opening(|record| record.columns[2].summed = true),
            ReadError::InvalidColumn {
                column: 2,
                problem: ColumnProblem::Summed
            }
        ));
        // The codec and encoding of block entry 0, which belongs to the integer column 0.
        assert_eq!(
            entry_problem(0, |block| block.codec = 9),
            BlockProblem::Codec(9)
        );
        assert_eq!(
            entry_problem(0, |block| block.encoding = 9),
            BlockProblem::Encoding(9)
        );
        // Plain (16) is a tag, but of bytes columns.
        assert_eq!(
            entry_problem(0, |block| block.encoding = 16),
            BlockProblem::Encoding(16)
        );
        // A fixed column of no width, which neither the schema nor the builder makes.
        assert!(matches!(
            opening(|record| record.columns[6].width = 0),
            ReadError::InvalidColumn {
                column: 6,
                problem: ColumnProblem::FixedWidth(0)
            }
        ));
        // Stats of another column type: a signed range on the u64 column.
        assert!(matches!(
            opening(
                |record| record.columns[1].stats.range = Some(IntRange::Signed { min: 0, max: 1 })
            ),
            ReadError::InvalidColumn {
                column: 1,
                problem: ColumnProblem::Stats
            }
        ));
    }
}

/// The working-memory bound of the builder: every buffer it owns stays within its share of
/// [`working_memory`], at every append and at its largest, when `finish` has encoded
/// fragment 0, and the shares add up to the bound.
mod memory {
    use super::*;

    /// Checks every buffer of `builder` against its share, and that the shares are the bound.
    /// A buffer holds nothing or all of its share: it is allocated whole when first used, so
    /// it never grows, which would hold its old and new allocation together for a moment.
    fn check(builder: &ChunkBuilder<'_, Fragments>, bound: u64, when: &str) {
        let memory = builder.memory();
        for (name, capacity, cap) in &memory {
            assert!(
                *capacity == 0 || capacity == cap,
                "{when}: {name} holds {capacity} bytes, not 0 or its share of {cap}"
            );
        }
        let shares: usize = memory.iter().map(|&(_, _, cap)| cap).sum();
        assert_eq!(shares as u64, bound, "{when}: the shares are not the bound");
    }

    /// Builds `rows` in batches of the sizes in `sizes`, taken in turn, checking the builder
    /// after every batch and when it is largest.
    fn build_checked(
        layout: &[ColumnLayout],
        compressed: bool,
        rows: &[Vec<Cell>],
        sizes: &[usize],
    ) -> Fragments {
        let bound = working_memory(layout, compressed);
        let budget = MemoryBudget::new(bound);
        let account = crate::budget::MemoryAccount::new(
            &budget,
            "builder",
            crate::budget::MemoryPlan::default(),
        )
        .unwrap();
        let live = std::rc::Rc::new(std::cell::Cell::new(0u64));
        let observed = live.clone();
        account.observe_allocations(move |allocate, bytes, charged| {
            let next = if allocate {
                observed.get() + bytes
            } else {
                observed.get() - bytes
            };
            assert!(
                next <= charged && next <= bound,
                "actual {next}, charged {charged}, bound {bound}"
            );
            observed.set(next);
        });
        let mut fragments = Fragments::default();
        let mut compressor = compress::new_compressor(&crate::budget::test_account()).unwrap();
        let (kind, compressor) = if compressed {
            (ChunkKind::Consolidated, Some(&mut compressor))
        } else {
            (ChunkKind::L0Base, None)
        };
        let mut builder = ChunkBuilder::new(
            &mut fragments,
            kind,
            FINGERPRINT,
            compressor,
            layout,
            &account,
        )
        .unwrap();
        check(&builder, bound, "empty");
        let mut start = 0;
        for (turn, size) in sizes.iter().cycle().enumerate() {
            if start == rows.len() {
                break;
            }
            let end = (start + size).min(rows.len());
            append(&mut builder, layout, &rows[start..end]);
            check(&builder, bound, &format!("after batch {turn}"));
            assert_eq!(
                live.get(),
                builder
                    .memory()
                    .iter()
                    .map(|(_, capacity, _)| *capacity as u64)
                    .sum::<u64>()
            );
            start = end;
        }
        builder
            .finish_observed(|builder, directory| {
                check(builder, bound, "at fragment 0");
                let directory_bytes = directory.blocks.capacity() * size_of::<BlockRecord>()
                    + directory.large.capacity() * size_of::<LargeEntry>()
                    + directory.columns.capacity() * size_of::<ColumnRecord>();
                assert_eq!(
                    live.get(),
                    directory_bytes as u64
                        + builder
                            .memory()
                            .iter()
                            .map(|(_, capacity, _)| *capacity as u64)
                            .sum::<u64>()
                );
                let shares: BTreeMap<String, usize> = builder
                    .memory()
                    .into_iter()
                    .map(|(name, _, cap)| (name, cap))
                    .collect();
                for (name, capacity) in [
                    (
                        "directory blocks",
                        directory.blocks.capacity() * size_of::<BlockRecord>(),
                    ),
                    (
                        "directory large",
                        directory.large.capacity() * size_of::<LargeEntry>(),
                    ),
                    (
                        "directory columns",
                        directory.columns.capacity() * size_of::<ColumnRecord>(),
                    ),
                ] {
                    assert!(
                        capacity <= shares[name],
                        "{name} holds {capacity} bytes, past its share of {}",
                        shares[name]
                    );
                }
            })
            .unwrap();
        assert_eq!(live.get(), 0);
        assert_eq!(account.live_bytes(), 0);
        drop(account);
        assert_eq!(budget.reserved(), 0);
        read_everything(&fragments).unwrap();
        fragments
    }

    /// One value of a generated row.
    #[derive(Clone, Debug)]
    pub(super) enum Cell {
        Int(u64),
        Bytes(Vec<u8>),
        Null,
    }

    pub(super) fn append(
        builder: &mut ChunkBuilder<'_, Fragments>,
        layout: &[ColumnLayout],
        rows: &[Vec<Cell>],
    ) {
        let mut ints: Vec<Vec<u64>> = vec![Vec::new(); layout.len()];
        let mut bytes: Vec<Vec<&[u8]>> = vec![Vec::new(); layout.len()];
        let mut validity: Vec<Vec<bool>> = vec![Vec::new(); layout.len()];
        for row in rows {
            for (column, cell) in row.iter().enumerate() {
                validity[column].push(!matches!(cell, Cell::Null));
                match (cell, layout[column].ty) {
                    (Cell::Int(value), _) => ints[column].push(*value),
                    (Cell::Bytes(value), _) => bytes[column].push(value),
                    (Cell::Null, PhysicalType::Int { .. }) => ints[column].push(0),
                    (Cell::Null, PhysicalType::Bytes(_)) => bytes[column].push(b""),
                }
            }
        }
        let i64s: Vec<Vec<i64>> = ints
            .iter()
            .map(|values| values.iter().map(|&value| value as i64).collect())
            .collect();
        let u32s: Vec<Vec<u32>> = ints
            .iter()
            .map(|values| values.iter().map(|&value| value as u32).collect())
            .collect();
        let batch: Vec<Column<'_>> = layout
            .iter()
            .enumerate()
            .map(|(column, layout)| Column {
                values: match layout.ty {
                    PhysicalType::Int {
                        ty: IntType::I64, ..
                    } => Values::I64(&i64s[column]),
                    PhysicalType::Int {
                        ty: IntType::U64, ..
                    } => Values::U64(&ints[column]),
                    PhysicalType::Int {
                        ty: IntType::U32, ..
                    } => Values::U32(&u32s[column]),
                    PhysicalType::Bytes(_) => Values::Bytes(&bytes[column]),
                },
                validity: layout.nullable.then_some(&validity[column][..]),
            })
            .collect();
        builder.append(&batch).unwrap();
    }

    /// Every type and hint, nullable and not.
    pub(super) fn every_column() -> Vec<ColumnLayout> {
        vec![
            KEY_COLUMN,
            int(IntType::I64, true),
            ColumnLayout {
                ty: PhysicalType::Int {
                    ty: IntType::U64,
                    summed: true,
                },
                nullable: false,
            },
            int(IntType::U32, false),
            int(IntType::U32, true),
            bytes(BytesHint::LowCardinality, true),
            bytes(BytesHint::LowCardinality, false),
            bytes(BytesHint::Text, true),
            bytes(BytesHint::Text, false),
            bytes(BytesHint::Fixed(16), false),
            bytes(BytesHint::Fixed(16), true),
        ]
    }

    /// Rows for [`every_column`]: first dictionary-heavy ones, with 3000 distinct
    /// low-cardinality values, short text and some large values; then rows of empty values,
    /// which fill the blocks with the most rows a bytes block holds.
    pub(super) fn every_column_rows(seed: u64) -> Vec<Vec<Cell>> {
        let mut rng = Rng(seed | 1);
        let labels: Vec<Vec<u8>> = (0..3000)
            .map(|_| {
                let len = 1 + rng.below(12) as usize;
                rng.text(len)
            })
            .collect();
        (0..13_000u64)
            .map(|index| {
                let dense = index < 6000;
                let null = |rng: &mut Rng| rng.chance(4);
                let label = |rng: &mut Rng| {
                    if dense {
                        Cell::Bytes(labels[rng.below(3000) as usize].clone())
                    } else {
                        Cell::Bytes(Vec::new())
                    }
                };
                let text = |rng: &mut Rng| {
                    if !dense {
                        Cell::Bytes(Vec::new())
                    } else if rng.chance(800) {
                        let len = 20_000 + rng.below(40_000) as usize;
                        Cell::Bytes(rng.noise(len))
                    } else {
                        let len = rng.below(80) as usize;
                        Cell::Bytes(rng.text(len))
                    }
                };
                let mut row = vec![Cell::Int(index)];
                let value = rng.next();
                row.push(if null(&mut rng) {
                    Cell::Null
                } else {
                    Cell::Int(value)
                });
                row.push(Cell::Int(rng.below(1 << 20)));
                row.push(Cell::Int(rng.next()));
                let value = rng.next();
                row.push(if null(&mut rng) {
                    Cell::Null
                } else {
                    Cell::Int(value)
                });
                row.push(if dense && null(&mut rng) {
                    Cell::Null
                } else {
                    label(&mut rng)
                });
                row.push(label(&mut rng));
                row.push(if dense && null(&mut rng) {
                    Cell::Null
                } else {
                    text(&mut rng)
                });
                row.push(text(&mut rng));
                row.push(Cell::Bytes(rng.noise(16)));
                row.push(if null(&mut rng) {
                    Cell::Null
                } else {
                    Cell::Bytes(rng.noise(16))
                });
                row
            })
            .collect()
    }

    #[test]
    fn every_buffer_stays_within_its_share_of_the_bound() {
        let layout = every_column();
        let rows = every_column_rows(41);
        for compressed in [false, true] {
            build_checked(&layout, compressed, &rows, &[1, 333, 2, 1500, 77]);
        }
    }

    /// A chunk of more blocks than a doubling directory would allot room for: 252 blocks of
    /// one 16 KiB text value each, and seven integer columns, 259 entries in all, past the
    /// 256 a doubling `Vec` reaches before it doubles again.
    #[test]
    fn a_chunk_of_many_blocks_keeps_its_directory_within_its_share() {
        let mut layout = vec![KEY_COLUMN];
        layout.extend([int(IntType::U32, false); 6]);
        layout.push(bytes(BytesHint::Text, false));
        let mut rng = Rng(43);
        let rows: Vec<Vec<Cell>> = (0..252u64)
            .map(|index| {
                let mut row = vec![Cell::Int(index)];
                row.extend((0..6).map(|_| Cell::Int(rng.below(1 << 30))));
                row.push(Cell::Bytes(rng.noise(MAX_INLINE_VALUE_LEN)));
                row
            })
            .collect();
        let fragments = build_checked(&layout, false, &rows, &[1, 5, 20]);
        assert_eq!(fragments.directory().blocks.len(), 259);
    }

    /// A large value's first piece fills the fragment buffer, which a few blocks before it
    /// left at 128 KiB: doubling it would pass the fragment's 256 KiB.
    #[test]
    fn a_large_value_keeps_the_fragment_buffer_within_one_fragment() {
        let layout = [KEY_COLUMN, bytes(BytesHint::Text, false)];
        let mut rng = Rng(47);
        let rows: Vec<Vec<Cell>> = (0..10u64)
            .map(|index| {
                let len = if index == 6 {
                    300_000
                } else {
                    MAX_INLINE_VALUE_LEN
                };
                vec![Cell::Int(index), Cell::Bytes(rng.noise(len))]
            })
            .collect();
        let fragments = build_checked(&layout, false, &rows, &[1]);
        assert_eq!(fragments.directory().large.len(), 1);
    }

    /// The largest bytes block of these columns holds 6547 rows, fewer than a power of two:
    /// the scratch's present rows and the dictionary's order stop there, where doubling
    /// would reach 8192.
    #[test]
    fn the_scratch_stops_at_the_rows_of_the_largest_bytes_block() {
        let layout = [KEY_COLUMN, bytes(BytesHint::LowCardinality, true)];
        let rows: Vec<Vec<Cell>> = (0..7000u64)
            .map(|index| vec![Cell::Int(index), Cell::Bytes(Vec::new())])
            .collect();
        let fragments = build_checked(&layout, false, &rows, &[100, 900]);
        assert_eq!(fragments.directory().blocks[2].rows, 6547);
    }

    /// A planned chunk holds at most 255 large values, each longer than an inline value, and
    /// the directory's large-value entries stop there, where doubling would reach 256.
    #[test]
    fn many_large_values_keep_their_entries_within_their_share() {
        let layout = [KEY_COLUMN, bytes(BytesHint::Text, false)];
        let mut rng = Rng(53);
        let rows: Vec<Vec<Cell>> = (0..130u64)
            .map(|index| {
                vec![
                    Cell::Int(index),
                    Cell::Bytes(rng.noise(MAX_INLINE_VALUE_LEN + 1)),
                ]
            })
            .collect();
        let fragments = build_checked(&layout, false, &rows, &[1, 7]);
        assert_eq!(fragments.directory().large.len(), 130);
    }

    /// Review B1's case: 64 `i64` columns of noise, appended a row at a time, fill every
    /// column's block, then close them all into fragments. Had the fragment buffer grown as
    /// they did, from one block towards a fragment, each step would have held its old and new
    /// allocation together while every column was at its cap, past the bound. Every buffer is
    /// allocated whole instead, when first used, and checked after every row.
    #[test]
    fn full_columns_built_row_by_row_allocate_each_buffer_once() {
        let mut layout = vec![KEY_COLUMN];
        layout.extend([int(IntType::I64, false); 63]);
        let mut rng = Rng(71);
        let rows: Vec<Vec<Cell>> = (0..BLOCK_TARGET_LEN / 8 + 1)
            .map(|_| (0..64).map(|_| Cell::Int(rng.next())).collect())
            .collect();
        let fragments = build_checked(&layout, false, &rows, &[1]);
        assert_eq!(fragments.directory().blocks.len(), 128);
    }

    /// The bound of three reference chunks, which decide whether the mobile staging budget
    /// of 1 MiB builds a chunk (§10.1). If a number changes, the bound changed: say why.
    #[test]
    fn the_bound_of_the_reference_chunks_is_pinned() {
        let summed = ColumnLayout {
            ty: PhysicalType::Int {
                ty: IntType::U64,
                summed: true,
            },
            nullable: false,
        };
        let series = [KEY_COLUMN, int(IntType::U32, false), summed, summed, summed];
        let event = [
            KEY_COLUMN,
            int(IntType::U64, false),
            int(IntType::U64, false),
            bytes(BytesHint::LowCardinality, true),
            bytes(BytesHint::Text, false),
            bytes(BytesHint::Fixed(16), false),
            int(IntType::U32, false),
            int(IntType::U32, true),
        ];
        assert_eq!(working_memory(&series, false), SERIES);
        assert_eq!(working_memory(&series, true), SERIES_COMPRESSED);
        assert_eq!(working_memory(&event, false), EVENT);
    }

    const SERIES: u64 = 286_666;
    const SERIES_COMPRESSED: u64 = 319_583;
    const EVENT: u64 = 502_584;
}

/// The reader's memory (§7.4, §8.2): what opening a chunk and decoding its blocks hold,
/// against what was reserved for them beforehand.
mod read_memory {
    use std::collections::BTreeSet;

    use strum::VariantArray;

    use super::{
        hostile::{replace_last_block, replace_last_stored},
        memory::{append, every_column, every_column_rows},
        *,
    };
    use crate::{
        chunk::{
            format::{Directory, open_memory},
            reader::decode_memory,
        },
        codec::{
            MAX_BLOCK_ROWS,
            block::{BlockShape, DecodeMemory, MAX_BODY_OVERHEAD},
            int::IntEncoding,
        },
    };

    /// The bytes of `buffers`, added up.
    fn total(buffers: &[(&str, usize)]) -> u64 {
        buffers.iter().map(|&(_, bytes)| bytes as u64).sum()
    }

    /// A query's decode buffers: a decoder, and one decoded block per column type, which every
    /// column of that type shares, as a query reading the columns in turn could.
    struct Reading {
        decoder: BlockDecoder,
        signed: IntBlock<i64>,
        unsigned: IntBlock<u64>,
        narrow: IntBlock<u32>,
        bytes: BytesBlock,
    }

    impl Reading {
        fn new() -> Self {
            Self {
                decoder: BlockDecoder::new(&crate::budget::test_account()).unwrap(),
                signed: IntBlock::default(),
                unsigned: IntBlock::default(),
                narrow: IntBlock::default(),
                bytes: BytesBlock::default(),
            }
        }

        /// Reads block `block` of `column`. Whether the read succeeds or not, the decoder's
        /// scratch and the decoded block then hold exactly what the block declares, whatever
        /// they held before: its [`DecodeMemory`], for the chunk's large values.
        fn read(
            &mut self,
            reader: &ChunkReader,
            fragments: &Fragments,
            column: u16,
            block: u32,
        ) -> Result<(), ReadError> {
            let shape = reader.blocks(column)?[block as usize].shape();
            let large = reader.large_values().len() as u32;
            let (read, output, exact) = match reader.columns()[usize::from(column)].ty {
                PhysicalType::Int {
                    ty: IntType::I64, ..
                } => {
                    let out = &mut self.signed;
                    let read = reader.read_ints(fragments, column, block, &mut self.decoder, out);
                    (
                        read,
                        total(&out.buffers()),
                        DecodeMemory::ints(shape, IntType::I64),
                    )
                }
                PhysicalType::Int {
                    ty: IntType::U64, ..
                } => {
                    let out = &mut self.unsigned;
                    let read = reader.read_ints(fragments, column, block, &mut self.decoder, out);
                    (
                        read,
                        total(&out.buffers()),
                        DecodeMemory::ints(shape, IntType::U64),
                    )
                }
                PhysicalType::Int {
                    ty: IntType::U32, ..
                } => {
                    let out = &mut self.narrow;
                    let read = reader.read_ints(fragments, column, block, &mut self.decoder, out);
                    (
                        read,
                        total(&out.buffers()),
                        DecodeMemory::ints(shape, IntType::U32),
                    )
                }
                PhysicalType::Bytes(hint) => {
                    let out = &mut self.bytes;
                    let read = reader.read_bytes(fragments, column, block, &mut self.decoder, out);
                    let exact = DecodeMemory::bytes(shape, large, hint);
                    (read, total(&out.buffers()), exact)
                }
            };
            let held = DecodeMemory {
                output,
                scratch: total(&self.decoder.buffers()),
            };
            assert_eq!(held, exact, "after block {block} of column {column}");
            read
        }
    }

    /// A compressed chunk of the encodings [`every_column_rows`] leaves out: a constant
    /// column, timestamps whose steps grow, which delta of delta encodes best, and fixed
    /// values that compress.
    fn rarer_encodings() -> Fragments {
        let rows = 3000;
        let keys: Vec<i64> = (0..rows).collect();
        let constant = vec![7u64; rows as usize];
        let times: Vec<i64> = keys.iter().map(|&row| row * row).collect();
        let fixed: Vec<[u8; 16]> = (0..rows).map(|row| [row as u8 % 4; 16]).collect();
        let fixed: Vec<&[u8]> = fixed.iter().map(|value| &value[..]).collect();
        let layout = [
            KEY_COLUMN,
            int(IntType::U64, false),
            int(IntType::I64, false),
            bytes(BytesHint::Fixed(16), false),
        ];
        let (fragments, _) = build_chunk(&layout, true, |builder| {
            let column = |values| Column {
                values,
                validity: None,
            };
            builder
                .append(&[
                    column(Values::I64(&keys)),
                    column(Values::U64(&constant)),
                    column(Values::I64(&times)),
                    column(Values::Bytes(&fixed)),
                ])
                .unwrap();
        });
        fragments
    }

    /// The blocks of every column type and hint, in every encoding, raw and compressed, read
    /// in turn through shared buffers: blocks of many empty values follow blocks of long ones,
    /// and the bytes columns of every hint share one decoded block. Each read leaves the
    /// buffers holding exactly what its block declares, within [`decode_memory`] of its column
    /// in any chunk. The large values are read in turn into one `Vec`, which grows to exactly a
    /// value's length when it has less room.
    #[test]
    fn every_read_holds_exactly_what_its_block_declares() {
        let layout = every_column();
        let rows = every_column_rows(61);
        let mut chunks: Vec<Fragments> = [false, true]
            .map(|compressed| {
                build_chunk(&layout, compressed, |builder| {
                    for batch in rows.chunks(1000) {
                        append(builder, &layout, batch);
                    }
                })
                .0
            })
            .into();
        chunks.push(rarer_encodings());
        let mut seen = BTreeSet::new();
        let mut large = crate::budget::test_account()
            .buffer(0)
            .expect("empty storage is free");
        let mut grown = 0;
        for fragments in &chunks {
            let reader = open_chunk(fragments).unwrap();
            let large_count = reader.large_values().len() as u32;
            let mut reading = Reading::new();
            for (column, desc) in reader.columns().iter().enumerate() {
                let column = column as u16;
                for (block, entry) in reader.blocks(column).unwrap().iter().enumerate() {
                    let shape = entry.shape();
                    seen.insert((entry.codec as u8, entry.encoding.tag()));
                    let exact = match desc.ty {
                        PhysicalType::Int { ty, .. } => DecodeMemory::ints(shape, ty),
                        PhysicalType::Bytes(hint) => DecodeMemory::bytes(shape, large_count, hint),
                    };
                    let bound = decode_memory(shape, desc.ty);
                    assert!(exact.output <= bound.output && exact.scratch == bound.scratch);
                    reading
                        .read(&reader, fragments, column, block as u32)
                        .unwrap();
                    for value in reading.bytes.large_values().to_vec() {
                        let room = large.capacity();
                        let len = reader.large_values()[value.index as usize].len as usize;
                        reader
                            .read_large(fragments, value.index, &mut large)
                            .unwrap();
                        assert_eq!(large.capacity(), room.max(len));
                        grown += usize::from(room != 0 && room < len);
                    }
                }
            }
        }
        assert!(
            grown > 0,
            "no large value needed more room than one before it"
        );
        // Every encoding, and a bytes block of each in either codec: the scratch holds a
        // compressed block's body.
        let ints = IntEncoding::VARIANTS
            .iter()
            .map(|&encoding| Encoding::Int(encoding));
        for encoding in ints {
            assert!(
                seen.iter().any(|&(_, tag)| tag == encoding.tag()),
                "no block of {encoding:?} was read"
            );
        }
        for &encoding in BytesEncoding::VARIANTS {
            for codec in [Codec::Raw, Codec::Zstd] {
                let tags = (codec as u8, Encoding::Bytes(encoding).tag());
                assert!(
                    seen.contains(&tags),
                    "no {codec:?} block of {encoding:?} was read"
                );
            }
        }
    }

    /// A text column whose blocks go from many empty values to a few long ones and back,
    /// read in either order through one decoded block: each buffer grows and shrinks with the
    /// blocks, the offsets one way while the heap goes the other, and holds exactly what each
    /// block declares, never the most an earlier block needed.
    #[test]
    fn a_read_holds_its_block_as_the_blocks_grow_and_shrink() {
        let long = vec![b'x'; MAX_INLINE_VALUE_LEN];
        let mut values: Vec<&[u8]> = vec![b""; 7000];
        values.extend([&long[..]; 3]);
        values.extend(vec![&b""[..]; 7000]);
        let fragments = two_columns(bytes(BytesHint::Text, false), &values);
        let reader = open_chunk(&fragments).unwrap();
        let blocks = reader.blocks(1).unwrap().len() as u32;
        let mut changes = BTreeSet::new();
        for order in [(0..blocks).collect::<Vec<_>>(), (0..blocks).rev().collect()] {
            let mut reading = Reading::new();
            let mut before = reading.bytes.buffers();
            for block in order {
                reading.read(&reader, &fragments, 1, block).unwrap();
                let after = reading.bytes.buffers();
                for ((name, old), (_, new)) in before.iter().zip(&after) {
                    if old != new {
                        changes.insert((*name, old < new));
                    }
                }
                before = after;
            }
        }
        for buffer in ["offsets", "heap"] {
            assert!(changes.contains(&(buffer, true)), "{buffer} never grew");
            assert!(changes.contains(&(buffer, false)), "{buffer} never shrank");
        }
    }

    /// A chunk of a key and `column`, one row per value of `values`.
    fn two_columns(column: ColumnLayout, values: &[&[u8]]) -> Fragments {
        let keys: Vec<i64> = (0..values.len() as i64).collect();
        let (fragments, _) = build_chunk(&[KEY_COLUMN, column], false, |builder| {
            builder
                .append(&[
                    Column {
                        values: Values::I64(&keys),
                        validity: None,
                    },
                    Column {
                        values: Values::Bytes(values),
                        validity: None,
                    },
                ])
                .unwrap();
        });
        fragments
    }

    /// Reads the bytes column of a chunk of [`two_columns`], which must fail to decode.
    fn decode_error(fragments: &Fragments) -> DecodeError {
        let reader = open_chunk(fragments).unwrap();
        match Reading::new().read(&reader, fragments, 1, 0) {
            Err(ReadError::DecodeBlock { source, .. }) => source,
            other => panic!("{other:?}"),
        }
    }

    /// A dictionary has no more entries than values, which bounds its entries' ends by the
    /// block's rows. One of three empty entries for one value is damage, and decoding it grows
    /// nothing past what was reserved.
    #[test]
    fn a_dictionary_of_more_entries_than_values_stays_within_its_reservation() {
        let mut fragments = two_columns(bytes(BytesHint::LowCardinality, false), &[b"a"]);
        let mut stored = vec![
            Codec::Raw as u8,
            Encoding::Bytes(BytesEncoding::Dictionary).tag(),
        ];
        // No flags and one row; three entries, whose ends are all 0; then the one value's
        // code, of no bits.
        stored.push(0);
        stored.extend(1u32.to_le_bytes());
        stored.extend(3u32.to_le_bytes());
        stored.push(IntEncoding::Constant as u8);
        stored.extend(0u64.to_le_bytes());
        stored.push(0);
        let len = stored.len() - 2;
        replace_last_stored(&mut fragments, 1, &stored, len);
        let error = decode_error(&fragments);
        assert!(
            matches!(error, DecodeError::DictionaryLength { len: 3, values: 1 }),
            "{error}"
        );
    }

    /// A text block refers to each large value at most once, in order, so it holds no more
    /// references than the chunk has large values. One that refers to the chunk's one large
    /// value three times is damage, and decoding it grows nothing past what was reserved.
    #[test]
    fn a_text_block_repeating_a_large_value_stays_within_its_reservation() {
        let long = vec![b'x'; MAX_INLINE_VALUE_LEN + 1];
        let mut fragments = two_columns(bytes(BytesHint::Text, false), &[&long, b"a", b"b"]);
        assert_eq!(fragments.directory().large.len(), 1);
        let mut scratch = EncodeScratch::new(
            &crate::budget::test_account(),
            crate::codec::block::ScratchCaps::default(),
        );
        let encoded = encode_bytes(
            &[&b""[..]; 3][..],
            None,
            &[(0, 0), (1, 0), (2, 0)],
            BytesHint::Text,
            None,
            &mut scratch,
        )
        .unwrap();
        replace_last_block(&mut fragments, 1, &encoded);
        let error = decode_error(&fragments);
        assert!(
            matches!(error, DecodeError::LargeRefOrder { index: 0 }),
            "{error}"
        );
    }

    /// A dictionary block's values stop at MAX_BLOCK_LEN once expanded, which is its heap's
    /// share: nine codes of one 16 KiB entry are damage, and their expansion stops there.
    #[test]
    fn a_dictionary_expanding_past_the_limit_stays_within_its_reservation() {
        let short: Vec<&[u8]> = vec![b"a"; 9];
        let mut fragments = two_columns(bytes(BytesHint::LowCardinality, false), &short);
        let value = vec![b'x'; MAX_INLINE_VALUE_LEN];
        let mut scratch = EncodeScratch::new(
            &crate::budget::test_account(),
            crate::codec::block::ScratchCaps::default(),
        );
        let encoded = encode_bytes(
            &vec![&value[..]; 9][..],
            None,
            &[],
            BytesHint::LowCardinality,
            None,
            &mut scratch,
        )
        .unwrap();
        assert_eq!(encoded.encoding, Encoding::Bytes(BytesEncoding::Dictionary));
        replace_last_block(&mut fragments, 1, &encoded);
        let error = decode_error(&fragments);
        assert!(matches!(error, DecodeError::HeapLimit { .. }), "{error}");
    }

    /// A chunk of 300 blocks: the key and 19 `u64` columns of 61,440 rows, 15 blocks each.
    fn chunk_of_300_blocks() -> Fragments {
        let rows = 15 * BLOCK_TARGET_LEN / size_of::<u64>();
        let keys: Vec<i64> = (0..rows as i64).collect();
        let mut rng = Rng(67);
        let fields: Vec<Vec<u64>> = (0..19)
            .map(|_| (0..rows).map(|_| rng.below(1 << 20)).collect())
            .collect();
        let mut layout = vec![KEY_COLUMN];
        layout.extend([int(IntType::U64, false); 19]);
        let (fragments, _) = build_chunk(&layout, false, |builder| {
            let mut batch = vec![Column {
                values: Values::I64(&keys),
                validity: None,
            }];
            batch.extend(fields.iter().map(|field| Column {
                values: Values::U64(field),
                validity: None,
            }));
            builder.append(&batch).unwrap();
        });
        assert_eq!(fragments.directory().blocks.len(), 300);
        fragments
    }

    /// Fragment 0 of a chunk of a key and a `u32` column over 1,024 rows, with `blocks` blocks
    /// in all, each in a data fragment of its own, and records as short as they get: the
    /// key's blocks hold one row each, as do the `u32` column's but its last, which holds the
    /// rest. That is the most records a fragment 0 of its length holds, where
    /// [`open_memory`] is at its tightest.
    fn shortest_directory(blocks: usize) -> Vec<u8> {
        const ROWS: u32 = 1024;
        assert!((ROWS as usize + 1..=2 * ROWS as usize).contains(&blocks));
        let block = |column, first_row, rows, fragment| BlockRecord {
            column,
            codec: Codec::Raw as u8,
            encoding: Encoding::Int(IntEncoding::Raw).tag(),
            first_row,
            rows,
            fragment,
            offset: 0,
            stored_len: 2,
            uncompressed_len: 0,
            stats: IntStats::default(),
        };
        let narrow = (blocks - ROWS as usize) as u32;
        let mut records: Vec<BlockRecord> =
            (0..ROWS).map(|row| block(0, row, 1, 1 + row)).collect();
        records.extend((0..narrow).map(|index| {
            let rows = if index + 1 == narrow { ROWS - index } else { 1 };
            block(1, index, rows, 1 + ROWS + index)
        }));
        let record = DirectoryRecord {
            header: HeaderRecord {
                kind: ChunkKind::L0Base as u8,
                fingerprint: FINGERPRINT,
                rows: ROWS,
                fragments: 1 + blocks as u32,
                key_min: 0,
                key_max: 0,
            },
            columns: vec![
                ColumnRecord::new(KEY_COLUMN.ty, false, ROWS, IntStats::default()),
                ColumnRecord::new(
                    int(IntType::U32, false).ty,
                    false,
                    narrow,
                    IntStats::default(),
                ),
            ],
            blocks: records,
            large: Vec::new(),
        };
        let mut content = Vec::new();
        record.write(&mut content);
        content
    }

    /// What parsing `content` holds at its peak, and what its directory then keeps.
    fn parse_peak(content: &[u8]) -> (u64, u64) {
        let mut peak = 0;
        let directory =
            Directory::parse_observed(content, &crate::budget::test_account(), |buffers| {
                peak = total(buffers)
            })
            .unwrap();
        (peak, directory.memory())
    }

    /// Parsing a fragment 0 holds no more at its peak than [`open_memory`] of its length, from
    /// a small chunk's to one of MAX_BLOCKS records as short as they get, which is where the
    /// bound is tightest; and the directory keeps less.
    /// A reader is lent no more than it reserved. Fragment 0 is fetched only once what parsing
    /// a fragment 0 of the length its manifest entry records holds is reserved, and by that
    /// length; a data fragment by the length its directory gives it. One longer than that is
    /// refused by the source before anything sees it; one shorter is damage too, found before
    /// it is parsed or decoded. After each failure the budget holds what it held before.
    #[test]
    fn a_reader_is_lent_no_more_than_it_reserved() {
        /// Counts the fetches, and the longest fragment lent.
        struct Lending<'a> {
            fragments: &'a Fragments,
            fetched: Cell<u32>,
            longest: Cell<usize>,
        }

        impl FragmentSource for Lending<'_> {
            fn get<T>(
                &self,
                index: u32,
                limit: usize,
                f: impl FnOnce(Option<&[u8]>) -> T,
            ) -> Result<T, StorageError> {
                self.fetched.set(self.fetched.get() + 1);
                self.fragments.get(index, limit, |bytes| {
                    let len = bytes.map_or(0, <[u8]>::len);
                    self.longest.set(self.longest.get().max(len));
                    f(bytes)
                })
            }
        }

        let lending = |fragments| Lending {
            fragments,
            fetched: Cell::new(0),
            longest: Cell::new(0),
        };
        let fragments = chunk_of_300_blocks();
        let length = recorded(&fragments);
        let bound = open_memory(length as usize - TRAILER_LEN);
        let source = lending(&fragments);
        let budget = MemoryBudget::new(bound + u64::from(length) - 1);
        let error = ChunkReader::open(
            &source,
            &crate::budget::MemoryAccount::new(
                &budget,
                "test reader",
                crate::budget::MemoryPlan::default(),
            )
            .unwrap(),
            length,
        )
        .unwrap_err();
        assert!(
            matches!(error, ReadError::ReserveDirectory { .. }),
            "{error}"
        );
        assert_eq!(source.fetched.get(), 0);
        let budget = MemoryBudget::new(bound + u64::from(length));
        let reader = ChunkReader::open(
            &source,
            &crate::budget::MemoryAccount::new(
                &budget,
                "test reader",
                crate::budget::MemoryPlan::default(),
            )
            .unwrap(),
            length,
        )
        .unwrap();
        assert_eq!(budget.reserved(), reader.memory() + u64::from(length));
        drop(reader);

        // Fragment 0 longer than its entry records: never lent.
        let source = lending(&fragments);
        let budget = MemoryBudget::new(u64::MAX);
        let error = ChunkReader::open(
            &source,
            &crate::budget::MemoryAccount::new(
                &budget,
                "test reader",
                crate::budget::MemoryPlan::default(),
            )
            .unwrap(),
            length - 1,
        )
        .unwrap_err();
        assert!(
            matches!(
                error,
                ReadError::ReadFragment {
                    index: 0,
                    source: StorageError::Corrupted { .. }
                }
            ),
            "{error}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        assert_eq!(source.longest.get(), 0);
        assert_eq!(budget.reserved(), 0);
        // Shorter, whether or not it would parse: lent within the reservation, then refused.
        let mut garbled = chunk_of_300_blocks();
        garbled.reseal(0, |content| content.truncate(content.len() / 2));
        for (fragments, recorded) in [(&fragments, length + 1), (&garbled, length)] {
            let source = lending(fragments);
            let budget = MemoryBudget::new(u64::MAX);
            let error = ChunkReader::open(
                &source,
                &crate::budget::MemoryAccount::new(
                    &budget,
                    "test reader",
                    crate::budget::MemoryPlan::default(),
                )
                .unwrap(),
                recorded,
            )
            .unwrap_err();
            let actual = fragments.map[&0].len();
            assert!(
                matches!(
                    error,
                    ReadError::Fragment0Length { recorded: found, actual: len }
                        if found == recorded && len == actual
                ),
                "{error}"
            );
            assert_eq!(error.kind(), ErrorKind::Corrupted);
            assert_eq!(source.longest.get(), actual);
            assert_eq!(budget.reserved(), 0);
        }
        // A recorded length no fragment 0 has is refused before anything is reserved.
        for recorded in [MIN_FRAGMENT0_LEN as u32 - 1, MAX_FRAGMENT_LEN as u32 + 1] {
            let source = lending(&fragments);
            let error = ChunkReader::open(
                &source,
                &crate::budget::MemoryAccount::new(
                    &MemoryBudget::new(0),
                    "test reader",
                    crate::budget::MemoryPlan::default(),
                )
                .unwrap(),
                recorded,
            )
            .unwrap_err();
            assert!(
                matches!(error, ReadError::RecordedFragment0 { recorded: found } if found == recorded),
                "{error}"
            );
            assert_eq!(error.kind(), ErrorKind::Corrupted);
            assert_eq!(source.fetched.get(), 0);
        }

        // A data fragment longer than its directory gives it: never lent, and the open reader
        // keeps its directory's reservation and nothing more.
        let mut grown = chunk_of_300_blocks();
        let block = open_chunk(&grown).unwrap().blocks(0).unwrap()[0];
        grown.reseal(block.fragment, |content| content.extend_from_slice(&[0; 8]));
        let source = lending(&grown);
        let budget = MemoryBudget::new(u64::MAX);
        let account = crate::budget::MemoryAccount::new(
            &budget,
            "test reader",
            crate::budget::MemoryPlan::default(),
        )
        .unwrap();
        account
            .reserve_lent(crate::chunk::reader::lent_memory(recorded(&grown)))
            .unwrap();
        let reader = ChunkReader::open(&source, &account, recorded(&grown)).unwrap();
        drop(account);
        let held = budget.reserved();
        let mut decoder = BlockDecoder::new(&crate::budget::test_account()).unwrap();
        let mut out = IntBlock::<i64>::default();
        let error = reader
            .read_ints(&source, 0, 0, &mut decoder, &mut out)
            .unwrap_err();
        assert!(
            matches!(
                error,
                ReadError::ReadFragment {
                    index,
                    source: StorageError::Corrupted { .. }
                } if index == block.fragment
            ),
            "{error}"
        );
        assert_eq!(source.longest.get(), grown.map[&0].len());
        assert_eq!(budget.reserved(), held);
        drop(reader);
        assert_eq!(budget.reserved(), 0);
    }

    #[test]
    fn parsing_a_directory_holds_no_more_than_its_bound() {
        let content =
            |fragments: &Fragments| checksum::verify(&fragments.map[&0]).unwrap().to_vec();
        let mut contents = vec![
            content(&two_columns(bytes(BytesHint::Text, false), &[b"a"])),
            content(&chunk_of_300_blocks()),
        ];
        contents.extend([1025, 1500, MAX_BLOCKS].map(shortest_directory));
        for content in &contents {
            let (peak, kept) = parse_peak(content);
            let bound = open_memory(content.len());
            assert!(
                peak <= bound,
                "parsing {} bytes held {peak}, past {bound}",
                content.len()
            );
            assert!(kept <= peak);
        }
    }

    /// The numbers that decide whether mobile's query budget of 2.5 MiB runs a scan (§10.1).
    /// If one changes, the bound changed: say why.
    #[test]
    fn the_reader_bounds_are_pinned() {
        // The largest blocks the format allows, compressed.
        let largest = |encoding| BlockShape {
            codec: Codec::Zstd,
            encoding,
            uncompressed_len: MAX_BLOCK_LEN,
            rows: MAX_BLOCK_ROWS,
        };
        let ints = decode_memory(largest(Encoding::Int(IntEncoding::Raw)), KEY_COLUMN.ty);
        let dictionary = decode_memory(
            largest(Encoding::Bytes(BytesEncoding::Dictionary)),
            PhysicalType::Bytes(BytesHint::LowCardinality),
        );
        let text = decode_memory(
            largest(Encoding::Bytes(BytesEncoding::Plain)),
            PhysicalType::Bytes(BytesHint::Text),
        );
        assert_eq!(
            [ints, dictionary, text].map(|memory| (memory.output, memory.scratch)),
            LARGEST_BLOCKS
        );

        // Opening a chunk of 300 blocks, and one of MAX_BLOCKS: what is reserved at first,
        // and what the reader keeps.
        let fragments = chunk_of_300_blocks();
        let len = checksum::verify(&fragments.map[&0]).unwrap().len();
        let reader = open_chunk(&fragments).unwrap();
        let shortest = shortest_directory(MAX_BLOCKS);
        let (peak, kept) = parse_peak(&shortest);
        assert_eq!(
            [
                (len, open_memory(len), reader.memory()),
                (shortest.len(), open_memory(shortest.len()), kept),
            ],
            OPENING
        );
        assert_eq!(peak, MAX_BLOCKS_PEAK);
        let longest = max_fragment0_len(MAX_COLUMNS, MAX_BLOCKS, MAX_LARGE_VALUES);
        assert_eq!(open_memory(longest), open_memory(shortest.len()));

        // A merge of 8 chunks of closed connections (§11.1), each with its largest planned
        // block of every column decoded at once (§7.4's M_merge), sharing one decoder: the
        // key; the incarnation and the sequence; start and first-seen times; upload and
        // download; the combination; the connection id; and the folded target.
        let closed = [
            KEY_COLUMN,
            int(IntType::U64, false),
            int(IntType::U64, false),
            int(IntType::I64, false),
            int(IntType::I64, false),
            int(IntType::U64, false),
            int(IntType::U64, false),
            int(IntType::U32, false),
            bytes(BytesHint::LowCardinality, false),
            bytes(BytesHint::LowCardinality, true),
        ];
        // A planned block holds at most BLOCK_TARGET_LEN raw bytes, of rows that cost at
        // least their cheapest, and its body exceeds that by at most MAX_BODY_OVERHEAD.
        let planned = |layout: ColumnLayout| {
            let (encoding, row) = match layout.ty {
                PhysicalType::Int { ty, .. } => (
                    Encoding::Int(IntEncoding::Raw),
                    int_raw_len(ty, layout.nullable),
                ),
                PhysicalType::Bytes(hint) => (
                    Encoding::Bytes(BytesEncoding::Dictionary),
                    bytes_raw_len(0, hint, layout.nullable),
                ),
            };
            let shape = BlockShape {
                codec: Codec::Zstd,
                encoding,
                uncompressed_len: BLOCK_TARGET_LEN + MAX_BODY_OVERHEAD,
                rows: BLOCK_TARGET_LEN / row,
            };
            decode_memory(shape, layout.ty)
        };
        let way: u64 = closed.iter().map(|&layout| planned(layout).output).sum();
        let scratch = closed
            .iter()
            .map(|&layout| planned(layout).scratch)
            .max()
            .unwrap();
        assert_eq!((way, scratch, 8 * way + scratch), MERGE);
    }

    /// The largest integer block, low-cardinality dictionary block and text block: what each
    /// holds in its decoded block and in the decoder's scratch.
    const LARGEST_BLOCKS: [(u64, u64); 3] =
        [(266_240, 131_072), (266_244, 393_216), (268_292, 262_144)];
    /// For the chunk of 300 blocks and the shortest fragment 0 of MAX_BLOCKS records: the
    /// length of fragment 0, the bound reserved to open it, and what the reader keeps.
    const OPENING: [(usize, u64, u64); 2] = [(7_426, 275_336, 28_040), (28_331, 401_176, 188_564)];
    const MAX_BLOCKS_PEAK: u64 = 385_324;
    /// One way of the merge, the shared decoder's scratch, and eight ways with it.
    const MERGE: (u64, u64, u64) = (589_413, 98_214, 4_813_518);
}

/// Building a chunk a column at a time, as a merge that holds one column of its rows does,
/// and copying a large value a fragment at a time.
mod columns {
    use super::*;
    use crate::chunk::reader::ChunkReader;

    impl Table {
        /// Appends the rows a column at a time, each column in batches of the sizes in
        /// `sizes`, taken in turn; a large text value goes a piece at a time.
        fn append_columns(&self, builder: &mut ChunkBuilder<'_, Fragments>, sizes: &[usize]) {
            let labels: Vec<&[u8]> = self.labels.iter().map(Vec::as_slice).collect();
            let texts: Vec<&[u8]> = self.texts.iter().map(Vec::as_slice).collect();
            let ids: Vec<&[u8]> = self.ids.iter().map(Vec::as_slice).collect();
            for column in 0..7 {
                let (mut start, mut turn) = (0, 0);
                while start < self.rows() {
                    let mut end = (start + sizes[turn % sizes.len()]).min(self.rows());
                    turn += 1;
                    let rows = start..end;
                    let batch = match column {
                        0 => Column {
                            values: Values::I64(&self.keys[rows]),
                            validity: None,
                        },
                        1 => Column {
                            values: Values::U64(&self.upload[rows.clone()]),
                            validity: Some(&self.upload_valid[rows]),
                        },
                        2 => Column {
                            values: Values::U32(&self.levels[rows]),
                            validity: None,
                        },
                        3 => Column {
                            values: Values::I64(&self.signed[rows.clone()]),
                            validity: Some(&self.signed_valid[rows]),
                        },
                        4 => Column {
                            values: Values::Bytes(&labels[rows.clone()]),
                            validity: Some(&self.labels_valid[rows]),
                        },
                        5 => {
                            // Up to the next large value, which goes a piece at a time.
                            let large = |row: usize| {
                                self.texts_valid[row] && texts[row].len() > MAX_INLINE_VALUE_LEN
                            };
                            if large(start) {
                                let value = texts[start];
                                let mut writer = builder.large_value(5, value.len()).unwrap();
                                for piece in value.chunks(DATA_PAYLOAD_LEN) {
                                    writer.put(piece).unwrap();
                                }
                                writer.finish().unwrap();
                                start += 1;
                                continue;
                            }
                            end = (start..end).find(|&row| large(row)).unwrap_or(end);
                            Column {
                                values: Values::Bytes(&texts[start..end]),
                                validity: Some(&self.texts_valid[start..end]),
                            }
                        }
                        _ => Column {
                            values: Values::Bytes(&ids[rows]),
                            validity: None,
                        },
                    };
                    builder.append_column(column, &batch).unwrap();
                    start = end;
                }
            }
        }
    }

    /// The fragments of the large values of a chunk, in order.
    fn large_fragments(fragments: &Fragments) -> Vec<Vec<u8>> {
        let reader = open_chunk(fragments).unwrap();
        reader
            .large_values()
            .iter()
            .flat_map(|entry| entry.first_fragment..entry.first_fragment + entry.fragments)
            .map(|index| fragments.map[&index].clone())
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(16))]

        // A chunk built a column at a time, in batches of any sizes, with its large values a
        // piece at a time, reads back as the same rows appended a batch at a time do: the same
        // blocks and values, whatever fragments they are placed in.
        #[test]
        fn columns_appended_one_at_a_time_read_back_as_rows_do(
            rows in 1usize..6000,
            seed in any::<u64>(),
            large in prop::collection::vec(
                prop_oneof![
                    Just(MAX_INLINE_VALUE_LEN + 1),
                    Just(DATA_PAYLOAD_LEN),
                    Just(DATA_PAYLOAD_LEN + 1),
                    (MAX_INLINE_VALUE_LEN + 1)..300_000,
                ],
                0..3,
            ),
            compressed in any::<bool>(),
            sizes in prop::collection::vec(1usize..2500, 1..6),
        ) {
            let table = Table::generate(rows, seed, &large);
            let whole = table.build(compressed);
            let columns = build_chunk(&table_layout(), compressed, |builder| {
                table.append_columns(builder, &sizes);
            });
            check_round_trip(&table, &columns, compressed)?;
            prop_assert!(content(&whole.0) == content(&columns.0));
            prop_assert_eq!(large_fragments(&whole.0), large_fragments(&columns.0));
        }
    }

    // A large value copied from another chunk a fragment at a time, as the reader hands its
    // pieces out, takes one fragment for each of the source's, byte for byte.
    #[test]
    fn a_large_value_copied_a_fragment_at_a_time_keeps_its_fragments() {
        let lens = [
            MAX_INLINE_VALUE_LEN + 1,
            DATA_PAYLOAD_LEN,
            3 * DATA_PAYLOAD_LEN + 5,
        ];
        let table = Table::generate(50, 9, &lens);
        let (source, _) = table.build(false);
        let reader = open_chunk(&source).unwrap();
        let layout = [bytes(BytesHint::Text, true)];
        let key = [KEY_COLUMN, layout[0]];
        let (copied, built) = build_chunk(&key, false, |builder| {
            for (index, entry) in reader.large_values().iter().enumerate() {
                let keys = [index as i64];
                let column = Column {
                    values: Values::I64(&keys),
                    validity: None,
                };
                builder.append_column(0, &column).unwrap();
                let mut large = builder.large_value(1, entry.len as usize).unwrap();
                for piece in 0..entry.fragments {
                    reader
                        .read_large_piece(&source, index as u32, piece, |content| {
                            large.put(content)
                        })
                        .unwrap()
                        .unwrap();
                }
                large.finish().unwrap();
            }
        });
        assert_eq!(built.rows, 3);
        assert_eq!(large_fragments(&copied), large_fragments(&source));
        let copy = open_chunk(&copied).unwrap();
        let sizes = |reader: &ChunkReader| -> Vec<(u32, u32)> {
            reader
                .large_values()
                .iter()
                .map(|entry| (entry.len, entry.fragments))
                .collect()
        };
        assert_eq!(sizes(&copy), sizes(&reader));
        let mut lens_read: Vec<u32> = sizes(&copy).iter().map(|&(len, _)| len).collect();
        lens_read.sort_unstable();
        assert_eq!(lens_read, lens.map(|len| len as u32));
        // A piece past the value's, and a value the chunk does not have, are damage.
        let error = reader.read_large_piece(&source, 0, 99, |_| ()).unwrap_err();
        assert!(matches!(
            error,
            ReadError::NoSuchPiece {
                index: 0,
                piece: 99
            }
        ));
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        let error = reader.read_large_piece(&source, 99, 0, |_| ()).unwrap_err();
        assert!(matches!(error, ReadError::NoSuchLargeValue { index: 99 }));
    }

    // Mistakes of a caller that appends a column at a time are local errors: a column the
    // chunk lacks, a large value outside a text column or of no large length, a piece of
    // another length than the builder's, a value ended early, and columns of other rows.
    #[test]
    fn column_and_piece_mistakes_are_local_errors() {
        let layout = [
            KEY_COLUMN,
            int(IntType::U64, false),
            bytes(BytesHint::Text, true),
        ];
        let keys = [1i64, 2];
        let key = Column {
            values: Values::I64(&keys),
            validity: None,
        };
        let local = |error: BuildError| {
            assert_eq!(error.kind(), ErrorKind::Local, "{error:?}");
            error
        };
        let mut fragments = Fragments::default();
        let mut builder = ChunkBuilder::new(
            &mut fragments,
            ChunkKind::L0Base,
            FINGERPRINT,
            None,
            &layout,
            &crate::budget::test_account(),
        )
        .unwrap();
        assert!(matches!(
            local(builder.append_column(3, &key).unwrap_err()),
            BuildError::UnknownColumn { column: 3 }
        ));
        assert!(matches!(
            local(builder.append_column(1, &key).unwrap_err()),
            BuildError::ValuesType { column: 1 }
        ));
        for (column, len) in [
            (1, MAX_INLINE_VALUE_LEN + 1),
            (2, MAX_INLINE_VALUE_LEN),
            (2, MAX_LARGE_VALUE_LEN + 1),
        ] {
            let error = builder.large_value(column, len).err().unwrap();
            assert!(
                matches!(local(error), BuildError::NotLarge { column: found, len: found_len } if found == column && found_len == len)
            );
        }
        assert!(matches!(
            local(
                builder
                    .large_value(4, MAX_INLINE_VALUE_LEN + 1)
                    .err()
                    .unwrap()
            ),
            BuildError::UnknownColumn { column: 4 }
        ));
        builder.append_column(0, &key).unwrap();
        let len = DATA_PAYLOAD_LEN + 10;
        let mut large = builder.large_value(2, len).unwrap();
        let piece = vec![7u8; DATA_PAYLOAD_LEN];
        assert!(matches!(
            local(large.put(&piece[..10]).unwrap_err()),
            BuildError::PieceLength {
                expected: DATA_PAYLOAD_LEN,
                actual: 10
            }
        ));
        large.put(&piece).unwrap();
        assert!(matches!(
            local(large.put(&piece[..11]).unwrap_err()),
            BuildError::PieceLength {
                expected: 10,
                actual: 11
            }
        ));
        let error = large.finish().unwrap_err();
        assert!(matches!(
            local(error),
            BuildError::LargeIncomplete { len: found, received: DATA_PAYLOAD_LEN } if found == len
        ));
        // Columns that end with other rows than the key column.
        let mut fragments = Fragments::default();
        let mut builder = ChunkBuilder::new(
            &mut fragments,
            ChunkKind::L0Base,
            FINGERPRINT,
            None,
            &layout,
            &crate::budget::test_account(),
        )
        .unwrap();
        builder.append_column(0, &key).unwrap();
        let one = [5u64];
        let column = Column {
            values: Values::U64(&one),
            validity: None,
        };
        builder.append_column(1, &column).unwrap();
        let error = builder.finish().unwrap_err();
        assert!(matches!(
            local(error),
            BuildError::ColumnLength {
                column: 1,
                expected: 2,
                actual: 1
            }
        ));
    }
}
