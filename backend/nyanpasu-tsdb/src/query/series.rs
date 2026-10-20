//! Grouped sums (§8.1 `SeriesGroup`). One scan serves several group requests, each with its
//! own filters; rankings, for example, ignore the filter on their own dimension. Group keys
//! are `Total`, `Labels` and `Mapped`.
//!
//! # Sources
//!
//! A query reads the buckets its range touches in one tier (§8.4), from (§4.3, "读取"):
//!
//! - every chunk of the tier whose extent and key range meet them. L0 base, L0 delta and
//!   consolidated chunks are read alike: each holds sums of its own, and they add up;
//! - the head's accumulators of those buckets. The head is exactly the journal records that
//!   no chunk holds (§4.2), so every delta counts once (§7.2).
//!
//! Buckets in an extent at or below the tier's deletion watermark are left out: retention
//! expired them (§9.1), and cleanup may not have deleted their chunks yet. A range that
//! starts before the watermark reports it was truncated (§9.4).
//!
//! # Skipping
//!
//! - A chunk is opened only when the manifest says its extent and buckets meet the range.
//! - A chunk whose buckets all lie in the range is read without its bucket column. In any
//!   other, each block of the bucket column is decided by its stats (§6.1): skipped when
//!   its buckets all lie outside the range, taken whole when they all lie inside, and
//!   decoded only when they straddle a boundary.
//! - The blocks of the other columns are decoded only for the rows that remain.
//!
//! # Memory (§7.4, §8.2)
//!
//! A query reserves from the snapshot's budget before it allocates, and fails with a `Local`
//! [`BudgetExceeded`] instead of growing past it:
//!
//! - before the scan: buffers sized by the query itself;
//! - before the first chunk is read: the block decoder's zstd context
//!   ([`SERIES_DECOMPRESSION_CONTEXT`]). A query that reads only the head does not reserve it;
//! - for each chunk, before it is opened: the fragment the backend lends while the reader
//!   decodes from it, as large as the chunk's manifest entry says ([`lent_memory`]); and its
//!   directory, which the reader reserves before it fetches fragment 0 and holds while the
//!   chunk is open ([`ChunkReader::open`]);
//! - for each block: what decoding it leaves in the column's buffer and in the decoder's
//!   scratch ([`decode_memory`]), reserved before the read as the larger of what they hold and
//!   what the block declares, and shrunk to the block's after it. The query keeps one decoder
//!   and one buffer per column, and reuses them from block to block and chunk to chunk;
//! - the groups, as they appear. There is no spilling and no approximation: too many groups
//!   fail the query (§8.2).
//!
//! The result keeps its groups' reservation until it is dropped; the rest returns when the
//! query ends. The manifest is scanned chunk by chunk, so the listing holds nothing.

use std::{
    hash::BuildHasher,
    ops::{ControlFlow, Range},
};

use rustc_hash::FxBuildHasher;
use snafu::{IntoError, OptionExt, ResultExt, Snafu, ensure};

#[cfg(test)]
use crate::chunk::reader::decode_memory;

use super::{
    LabelFilter,
    plan::{self, Buckets},
    rows_of,
};
use crate::{
    ErrorKind, LabelId, Snapshot, TierId, TimeRange, Timestamp,
    budget::{
        BudgetExceeded, BudgetVec, FrozenVec, Lease, MemoryAccount, MemoryBudget, MemoryPlan,
        Reserved, Retain, Retained,
    },
    catalog::Catalog,
    chunk::{
        FragmentSource, StoredChunk,
        builder::{ColumnLayout, KEY_COLUMN},
        error::ReadError,
        format::{IntRange, PhysicalType},
        reader::{ChunkReader, check_recorded, lent_memory},
    },
    codec::{
        block::{BlockDecoder, IntBlock},
        compress::{DECOMPRESSION_CONTEXT, SERIES_DECOMPRESSION_CONTEXT},
        int::{IntType, IntValue},
    },
    head::{Head, SeriesSource},
    layout::manifest::{self, Listed, ManifestError, StreamRef, retained_from},
    schema::{MAX_DIMENSIONS, MAX_FIELDS},
    storage::{Direction, Storage, StorageRead},
    time::Grid,
    version::{CatalogToken, QueryCheck},
};

/// A grouped-sum query over one tier (§8.1 `SeriesGroup`).
#[derive(Clone, Copy, Debug)]
pub struct SeriesQuery<'a> {
    /// Rounded out to the buckets it touches, each counted whole (§8.4).
    pub range: TimeRange,
    pub tier: TierId,
    /// Served by one scan. Each has its own filters and key, and its own list of groups in
    /// the result.
    pub requests: &'a [GroupRequest<'a>],
    pub fields: FieldMask,
}

/// One list of groups to compute.
#[derive(Clone, Copy, Debug)]
pub struct GroupRequest<'a> {
    /// A row counts only when every filter admits it.
    pub filters: &'a [LabelFilter<'a>],
    pub key: GroupKey<'a>,
}

/// What a request groups by.
#[derive(Clone, Copy, Debug)]
pub enum GroupKey<'a> {
    /// One group: the total.
    Total,
    /// The label ids of these series dimensions, one [`KeyPart::Label`] each, in this
    /// order.
    Labels(&'a [LabelId]),
    /// The node each map gives the row's label of its dimension, one part each, in this
    /// order: the second pass of a topology (§8.1).
    Mapped(&'a [LabelMap<'a>]),
}

/// Maps the label ids of one series dimension to node numbers ([`KeyPart::Node`]). An id
/// the table does not list maps to [`KeyPart::Other`], and the `skip` id to
/// [`KeyPart::Skipped`]: the row leaves this level out, and its neighbouring levels join
/// directly, as an empty proxy chain does in a topology (§8.1). Several ids may share a
/// node; the first pair of an id counts, and `skip` comes before the table.
///
/// Node numbers are `u16`: a topology keeps a few nodes per level, and the narrow type
/// keeps a group key's part in one word.
#[derive(Clone, Copy, Debug)]
pub struct LabelMap<'a> {
    pub dimension: LabelId,
    /// (label id, node) pairs, scanned for each row.
    pub nodes: &'a [(u32, u16)],
    pub skip: Option<u32>,
}

/// A set of series fields, as bits of their indices in
/// [`SeriesSpec::fields`](crate::SeriesSpec::fields): bit `i` selects field `i`. A series
/// has at most 16 fields, so a `u16` holds every set. Group sums list the selected fields in
/// index order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldMask(pub u16);

const _: () = assert!(MAX_FIELDS <= u16::BITS as usize);

impl FieldMask {
    /// The selected field indices, ascending.
    fn indices(self) -> impl Iterator<Item = usize> {
        (0..u16::BITS as usize).filter(move |&field| self.0 >> field & 1 == 1)
    }
}

/// One part of a group key: a label id for [`GroupKey::Labels`], and a node, the other
/// nodes or a skipped level for [`GroupKey::Mapped`].
///
/// A label id means what it says only through a snapshot whose [`Snapshot::catalog`] equals
/// the result's [`SeriesGroups::catalog`]: after a release, a later write may give the id to
/// another value (§5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyPart {
    Label(u32),
    Node(u16),
    Other,
    Skipped,
}

/// A query's result: one [`GroupList`] per request, in request order.
#[derive(Debug)]
pub struct SeriesGroups {
    lists: Reserved<FrozenVec<GroupList>>,
    catalog: CatalogToken,
    covered: TimeRange,
    truncated_by_retention: bool,
    retention_watermark: Option<Timestamp>,
    folded: bool,
}

impl SeriesGroups {
    /// One list per request, in request order.
    pub fn lists(&self) -> &[GroupList] {
        self.lists.get()
    }

    /// The token of the snapshot the result was read from, which names what the label ids of
    /// its keys mean ([`KeyPart`]).
    pub fn catalog(&self) -> CatalogToken {
        self.catalog.clone()
    }

    /// The time the query read: its range rounded out to whole buckets, from where the first
    /// starts to where the last ends (§8.4), clamped to the timestamps an `i64` holds.
    pub fn covered(&self) -> TimeRange {
        self.covered
    }

    /// Whether the range starts before the tier's [retention
    /// watermark](Self::retention_watermark), so that retention deleted part of it (§9.4).
    pub fn truncated_by_retention(&self) -> bool {
        self.truncated_by_retention
    }

    /// Where the tier's retained data starts: retention deleted every bucket before it, and
    /// the groups leave them out (§9.1, §9.4). `None` while retention has deleted none.
    pub fn retention_watermark(&self) -> Option<Timestamp> {
        self.retention_watermark
    }

    /// Whether the scan met a folded combination in the range: one with the overflow marker,
    /// 0, in any dimension (§5.2, §9.4). It is set before the filters, so it also reports the
    /// folded rows that no request admitted. Those rows lost their values of the folded
    /// dimensions, so filters and groups by those dimensions may undercount.
    pub fn folded(&self) -> bool {
        self.folded
    }
}

/// The groups of one request, in no particular order. A group exists when at least one row
/// counted in it, so a request no row passes has none, `Total` included.
///
/// Groups carry no folded flag of their own: one would only say that a folded row counted in
/// the group, not that a filter turned folded rows away from it, and a group keyed by a
/// folded dimension already shows the overflow marker in its key. The result says whether the
/// scan met folded rows at all ([`SeriesGroups::folded`]).
#[derive(Debug)]
pub struct GroupList {
    /// Key parts per group.
    width: usize,
    /// Sums per group: the selected fields.
    fields: usize,
    /// The number of groups.
    len: usize,
    keys: FrozenVec<KeyPart>,
    sums: FrozenVec<u64>,
}

/// One group of a [`GroupList`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group<'a> {
    pub key: &'a [KeyPart],
    /// The selected fields' saturating sums, in field index order.
    pub sums: &'a [u64],
}

impl GroupList {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = Group<'_>> {
        (0..self.len).map(|group| Group {
            key: self.key(group),
            sums: &self.sums[group * self.fields..(group + 1) * self.fields],
        })
    }

    fn key(&self, group: usize) -> &[KeyPart] {
        &self.keys[group * self.width..(group + 1) * self.width]
    }

    /// The retained keys and sums, measured by their actual buffer capacities.
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> u64 {
        (self.keys.capacity() * size_of::<KeyPart>() + self.sums.capacity() * size_of::<u64>())
            as u64
    }
}

/// One request's mutable groups and scan index, charged to the query's envelope.
struct GroupBuffer {
    width: usize,
    fields: usize,
    len: usize,
    capacity: usize,
    keys: BudgetVec<KeyPart>,
    sums: BudgetVec<u64>,
    slots: BudgetVec<u32>,
}

impl GroupBuffer {
    /// Starts empty charged buffers for this request's key width and selected fields.
    fn new(account: &MemoryAccount, width: usize, fields: usize) -> Result<Self, BudgetExceeded> {
        Ok(Self {
            width,
            fields,
            len: 0,
            capacity: 0,
            keys: account.buffer(0)?,
            sums: account.buffer(0)?,
            slots: account.buffer(0)?,
        })
    }

    /// Adds a row's values to the group of `key`, creating the group when it is new.
    fn add(
        &mut self,
        key: &[KeyPart],
        values: &[u64],
        account: &MemoryAccount,
    ) -> Result<(), BudgetExceeded> {
        if self.slots.is_empty() {
            self.index(MIN_SLOTS, account)?;
        }
        let mask = self.slots.len() - 1;
        let mut slot = FxBuildHasher.hash_one(key) as usize & mask;
        while let Some(group) = self.slots[slot].checked_sub(1) {
            let group = group as usize;
            if self.key(group) == key {
                let sums = &mut self.sums[group * self.fields..(group + 1) * self.fields];
                for (sum, &value) in sums.iter_mut().zip(values) {
                    *sum = sum.saturating_add(value);
                }
                return Ok(());
            }
            slot = (slot + 1) & mask;
        }

        let group = self.len;
        if group == self.capacity {
            let groups = group
                .checked_mul(2)
                .context(crate::budget::BudgetExceededSnafu {
                    what: "series groups",
                    needed: u64::MAX,
                    available: 0u64,
                })?
                .max(MIN_GROUPS);
            self.keys
                .try_grow_to(groups.checked_mul(self.width).context(
                    crate::budget::BudgetExceededSnafu {
                        what: "series keys",
                        needed: u64::MAX,
                        available: 0u64,
                    },
                )?)?;
            self.sums
                .try_grow_to(groups.checked_mul(self.fields).context(
                    crate::budget::BudgetExceededSnafu {
                        what: "series sums",
                        needed: u64::MAX,
                        available: 0u64,
                    },
                )?)?;
            self.capacity = groups;
        }
        self.keys.extend_from_slice(key);
        self.sums.extend_from_slice(values);
        self.len += 1;
        if 2 * self.len > self.slots.len() {
            self.index(2 * self.slots.len(), account)
        } else {
            self.slots[slot] = number(group);
            Ok(())
        }
    }

    fn key(&self, group: usize) -> &[KeyPart] {
        &self.keys[group * self.width..(group + 1) * self.width]
    }

    /// Rebuilds the index with `slots` slots, the new one reserved while the old is alive.
    fn index(&mut self, slots: usize, account: &MemoryAccount) -> Result<(), BudgetExceeded> {
        let mut index = account.buffer(slots)?;
        for _ in 0..slots {
            index.push(0);
        }
        let mask = slots - 1;
        for group in 0..self.len {
            let mut slot = FxBuildHasher.hash_one(self.key(group)) as usize & mask;
            while index[slot] != 0 {
                slot = (slot + 1) & mask;
            }
            index[slot] = number(group);
        }
        self.slots = index;
        Ok(())
    }
}

/// The consuming builder that transfers every request's groups into one retained result.
struct GroupsBuffer<'a> {
    account: &'a MemoryAccount,
    lists: BudgetVec<GroupBuffer>,
}

impl Retain<GroupList> for GroupsBuffer<'_> {
    /// Admits the output list, drops scan indexes, and moves keys, sums and their credit.
    fn retain(
        mut self,
        retained: &mut Retained<'_>,
    ) -> Result<FrozenVec<GroupList>, BudgetExceeded> {
        let mut lists = self.account.buffer(self.lists.len())?;
        while let Some(list) = self.lists.pop() {
            let GroupBuffer {
                width,
                fields,
                len,
                keys,
                sums,
                slots,
                ..
            } = list;
            drop(slots);
            lists.push(GroupList {
                width,
                fields,
                len,
                keys: keys.retain(retained)?,
                sums: sums.retain(retained)?,
            });
        }
        lists.reverse();
        lists.retain(retained)
    }
}

const MIN_SLOTS: usize = 16;
const MIN_GROUPS: usize = 8;

/// A group's number in the index. The budget runs out long before 2^32 groups.
fn number(group: usize) -> u32 {
    u32::try_from(group + 1).expect("fewer than 2^32 groups fit any budget")
}

/// What a scan holds whatever it reads, reserved before it starts: the layout of a series
/// chunk of `fields` fields, and the key of one row.
fn scan_plan(fields: usize, key_width: usize) -> Result<MemoryPlan, BudgetExceeded> {
    MemoryPlan::array::<ColumnLayout>(2 + fields)?.and(MemoryPlan::array::<KeyPart>(key_width)?)
}

#[cfg(test)]
fn scan_memory(fields: usize, key_width: usize) -> u64 {
    ((2 + fields) * size_of::<ColumnLayout>() + key_width * size_of::<KeyPart>()) as u64
}

/// The columns of a series chunk, as `write::chunking::series_columns` lays them out.
const BUCKET: u16 = 0;
const COMBO: u16 = 1;
const FIRST_FIELD: u16 = 2;

impl<S: Storage> Snapshot<S> {
    /// Sums the selected fields of the rows in `query`'s range and tier, into one list of
    /// groups per request, in one scan of the chunks and the head (§8.1).
    pub fn series(&self, query: &SeriesQuery<'_>) -> Result<SeriesGroups, SeriesQueryError> {
        self.revoked()
            .check("series::series:1")
            .context(CheckOpenSnafu)?;
        let result = self.series_inner(query);
        if let Err(error) = &result {
            if error.kind() == ErrorKind::BackendFailed {
                self.report(error.kind());
            } else {
                self.check_open().context(CheckOpenSnafu)?;
            }
        } else {
            self.revoked()
                .check("series::series:2")
                .context(CheckOpenSnafu)?;
        }
        result
    }

    /// Executes the admitted query; the public boundary reports failures and checks completion.
    fn series_inner(&self, query: &SeriesQuery<'_>) -> Result<SeriesGroups, SeriesQueryError> {
        let schema = self.schema();
        let tier_id = query.tier;
        let (series, tier) = schema
            .series
            .as_ref()
            .and_then(|series| Some((series, series.tiers.get(usize::from(tier_id.0))?)))
            .context(UnknownTierSnafu { tier: tier_id })?;
        let fields = series.fields.len();
        ensure!(
            u32::from(query.fields.0) >> fields == 0,
            UnknownFieldSnafu {
                mask: query.fields.0,
                fields
            }
        );
        for (index, request) in query.requests.iter().enumerate() {
            if let Some(dimension) = request
                .dimensions()
                .find(|dimension| !series.dimensions.contains(dimension))
            {
                return NotADimensionSnafu {
                    request: index,
                    dimension,
                }
                .fail();
            }
        }
        let grid = plan::tier_grid(tier);
        let buckets = plan::round(grid, query.range);
        let expired = manifest::read_watermark(self.storage(), StreamRef::Tier(tier_id))
            .context(ReadWatermarkSnafu)?;
        // Only for the result: it saturates at the ends of the timeline.
        let watermark = expired.map(|expired| retained_from(grid.extent(), expired));
        // The buckets the scan reads: those of the range not in an expired extent, bounded in
        // bucket numbers, which are exact where the watermark's timestamp saturates.
        let mut live = buckets.numbers.clone();
        if let Some(expired) = expired {
            live.start = match first_retained_bucket(grid, expired) {
                Some(first) => live.start.max(first).min(live.end),
                None => live.end,
            };
        }
        let read = Buckets {
            numbers: live,
            covered: buckets.covered,
        };

        let budget = self.budget();
        let key_width = query.requests.iter().map(GroupRequest::width).max();
        let masked = query.fields.indices().count();
        let workspace = MemoryAccount::new(
            budget,
            "series scan",
            scan_plan(fields, key_width.unwrap_or(0)).context(ReserveScanSnafu)?,
        )
        .context(ReserveScanSnafu)?;
        let groups = MemoryAccount::new(budget, "series groups", MemoryPlan::default())
            .context(ReserveListsSnafu)?;
        let mut lists = groups
            .buffer(query.requests.len())
            .context(ReserveListsSnafu)?;
        for request in query.requests {
            lists.push(
                GroupBuffer::new(&groups, request.width(), masked).context(ReserveListsSnafu)?,
            );
        }
        let mut layout = workspace.buffer(2 + fields).context(ReserveScanSnafu)?;
        layout.push(KEY_COLUMN);
        layout.push(ColumnLayout {
            ty: PhysicalType::Int {
                ty: IntType::U32,
                summed: false,
            },
            nullable: false,
        });
        for _ in 0..fields {
            layout.push(ColumnLayout {
                ty: PhysicalType::Int {
                    ty: IntType::U64,
                    summed: true,
                },
                nullable: false,
            });
        }
        let key = workspace
            .buffer(key_width.unwrap_or(0))
            .context(ReserveScanSnafu)?;
        let mut scan = Scan {
            revoked: self.revoked(),
            budget,
            catalog: &self.version().catalog,
            dimensions: &series.dimensions,
            requests: query.requests,
            fields: query.fields,
            buckets: read.numbers.clone(),
            layout,
            key,
            lists,
            account: &groups,
            folded: false,
            combo: Column::new(COMBO),
            values: std::array::from_fn(|_| Column::new(0)),
            bucket: Column::new(BUCKET),
        };
        for (column, field) in scan.values.iter_mut().zip(query.fields.indices()) {
            column.index = FIRST_FIELD + field as u16;
        }
        if !query.requests.is_empty() {
            scan.chunks(self.storage(), StreamRef::Tier(tier_id), grid, &read)?;
            scan.head(&self.version().head, tier_id)?;
        }
        let Scan { lists, folded, .. } = scan;
        let lists = groups
            .finish(GroupsBuffer {
                account: &groups,
                lists,
            })
            .context(ReserveListsSnafu)?;
        Ok(SeriesGroups {
            lists,
            catalog: self.catalog(),
            covered: buckets.covered,
            truncated_by_retention: watermark
                .is_some_and(|watermark| query.range.start() < watermark),
            retention_watermark: watermark,
            folded,
        })
    }
}

/// The first bucket of `grid` after the extents up to `expired`, or `None` when no bucket
/// number follows them. Computed in `i128`, where it is exact: a first bucket below
/// `i64::MIN` means that every bucket number is retained.
fn first_retained_bucket(grid: Grid, expired: i64) -> Option<i64> {
    let per_extent = i128::from(grid.extent().millis() / grid.bucket().millis());
    let first = (i128::from(expired) + 1) * per_extent;
    match i64::try_from(first) {
        Ok(first) => Some(first),
        Err(_) if first < 0 => Some(i64::MIN),
        Err(_) => None,
    }
}

impl GroupRequest<'_> {
    /// The series dimensions the request reads.
    fn dimensions(&self) -> impl Iterator<Item = LabelId> + '_ {
        let filters = self.filters.iter().map(|filter| filter.dimension);
        let (labels, maps): (&[LabelId], &[LabelMap<'_>]) = match self.key {
            GroupKey::Total => (&[], &[]),
            GroupKey::Labels(labels) => (labels, &[]),
            GroupKey::Mapped(maps) => (&[], maps),
        };
        filters
            .chain(labels.iter().copied())
            .chain(maps.iter().map(|map| map.dimension))
    }

    /// The parts of its keys.
    fn width(&self) -> usize {
        match self.key {
            GroupKey::Total => 0,
            GroupKey::Labels(labels) => labels.len(),
            GroupKey::Mapped(maps) => maps.len(),
        }
    }
}

impl LabelMap<'_> {
    fn part(&self, label: u32) -> KeyPart {
        if self.skip == Some(label) {
            return KeyPart::Skipped;
        }
        self.nodes
            .iter()
            .find(|&&(id, _)| id == label)
            .map_or(KeyPart::Other, |&(_, node)| KeyPart::Node(node))
    }
}

/// One query's scan: where rows come from, and the groups they go to.
struct Scan<'a, 'm> {
    /// The instance flag, borrowed without retaining its capture gate.
    revoked: QueryCheck<'a>,
    /// The snapshot's budget, which the decoder, the chunks and the blocks reserve from.
    budget: &'a MemoryBudget,
    catalog: &'a Catalog,
    /// The series dimensions, in combination order.
    dimensions: &'a [LabelId],
    requests: &'a [GroupRequest<'a>],
    fields: FieldMask,
    buckets: Range<i64>,
    /// The columns of a series chunk of the schema.
    layout: BudgetVec<ColumnLayout>,
    /// The key of the row being added, reused.
    key: BudgetVec<KeyPart>,
    lists: BudgetVec<GroupBuffer>,
    account: &'m MemoryAccount,
    /// Whether a row in the range had a folded combination, admitted or not.
    folded: bool,
    combo: Column<u32>,
    /// The selected fields' columns, the first as many as there are selected fields.
    values: [Column<u64>; MAX_FIELDS],
    bucket: Column<i64>,
}

impl Scan<'_, '_> {
    /// Reads the chunks of the tier that meet the buckets.
    fn chunks(
        &mut self,
        read: &impl StorageRead,
        stream: StreamRef,
        grid: Grid,
        buckets: &Buckets,
    ) -> Result<(), SeriesQueryError> {
        let numbers = &buckets.numbers;
        if numbers.is_empty() {
            return Ok(());
        }
        let extents = grid.extent_of_bucket(numbers.start)..=grid.extent_of_bucket(numbers.end - 1);
        let mut decoder = None;
        let stopped = manifest::scan_chunks(read, stream, extents, Direction::Forward, |listed| {
            if let Err(error) = self.revoked.check("series::chunks:1") {
                return ControlFlow::Break(CheckOpenSnafu.into_error(error));
            }
            if !buckets.meets(grid, &listed) {
                return ControlFlow::Continue(());
            }
            match self.read_listed(read, &listed, &mut decoder) {
                Ok(()) => ControlFlow::Continue(()),
                Err(error) => ControlFlow::Break(error),
            }
        })
        .context(ListChunksSnafu)?;
        stopped.map_or(Ok(()), Err)
    }

    /// Reads a chunk the manifest listed. The first one creates the decoder every chunk of the
    /// query shares.
    fn read_listed(
        &mut self,
        read: &impl StorageRead,
        listed: &Listed,
        decoder: &mut Option<Decoder>,
    ) -> Result<(), SeriesQueryError> {
        if decoder.is_none() {
            let context = self
                .budget
                .try_reserve("series decoder", SERIES_DECOMPRESSION_CONTEXT)
                .context(ReserveDecoderSnafu)?;
            let account = MemoryAccount::from_reservation(context);
            let padding = account
                .lease(MemoryPlan::bytes(
                    SERIES_DECOMPRESSION_CONTEXT - DECOMPRESSION_CONTEXT,
                ))
                .expect("the series context envelope includes its existing allowance");
            *decoder = Some(Decoder {
                blocks: BlockDecoder::new(&account).context(CreateDecoderSnafu)?,
                account,
                _padding: padding,
            });
        }
        let decoder = decoder.as_mut().expect("created above");
        // One chunk is read at a time, and its reader fetches one fragment at a time.
        let chunk = listed.chunk.0;
        check_recorded(listed.entry.fragment0).context(OpenChunkSnafu { chunk })?;
        let lent = lent_memory(listed.entry.fragment0);
        decoder
            .account
            .reserve_lent(lent)
            .context(ReserveFragmentSnafu { chunk })?;
        let source = StoredChunk {
            storage: read,
            chunk: listed.chunk,
        };
        self.chunk(chunk, &source, decoder, listed.entry.fragment0)
    }

    fn chunk(
        &mut self,
        chunk: u64,
        source: &impl FragmentSource,
        decoder: &mut Decoder,
        fragment0: u32,
    ) -> Result<(), SeriesQueryError> {
        self.revoked
            .check("series::chunk:1")
            .context(CheckOpenSnafu)?;
        let reader = ChunkReader::open(source, &decoder.account, fragment0)
            .context(OpenChunkSnafu { chunk })?;
        let columns = reader.columns();
        let shaped = columns.len() == self.layout.len()
            && columns
                .iter()
                .zip(self.layout.iter())
                .all(|(desc, layout)| desc.ty == layout.ty && desc.nullable == layout.nullable);
        ensure!(shaped, ChunkColumnsSnafu { chunk });
        let mut decode = Decode {
            revoked: self.revoked,
            chunk,
            reader: &reader,
            source,
            decoder,
        };
        self.combo.reset();
        self.bucket.reset();
        self.values.iter_mut().for_each(Column::reset);
        let (first, last) = reader.key_range();
        let buckets = self.buckets.clone();
        if buckets.contains(&first) && buckets.contains(&last) {
            return self.rows(&mut decode, 0..reader.rows(), false);
        }
        let blocks = reader
            .blocks(BUCKET)
            .expect("a series chunk has a bucket column");
        for entry in blocks {
            self.revoked
                .check("series::chunk:2")
                .context(CheckOpenSnafu)?;
            let rows = entry.first_row..entry.first_row + entry.rows;
            match entry.stats.range {
                Some(IntRange::Signed { min, max })
                    if max < buckets.start || min >= buckets.end => {}
                Some(IntRange::Signed { min, max })
                    if buckets.contains(&min) && buckets.contains(&max) =>
                {
                    self.rows(&mut decode, rows, false)?;
                }
                _ => self.rows(&mut decode, rows, true)?,
            }
        }
        Ok(())
    }

    /// Adds `rows` of the open chunk; when `check` is set, only those whose bucket is in
    /// range.
    fn rows(
        &mut self,
        decode: &mut Decode<'_, impl FragmentSource>,
        rows: Range<u32>,
        check: bool,
    ) -> Result<(), SeriesQueryError> {
        let masked = self.fields.indices().count();
        let mut values = [0u64; MAX_FIELDS];
        for row in rows {
            if check {
                let bucket = self.bucket.value(decode, row)?;
                if !self.buckets.contains(&bucket) {
                    continue;
                }
            }
            let combo = self.combo.value(decode, row)?;
            for (value, column) in values.iter_mut().zip(&mut self.values[..masked]) {
                *value = column.value(decode, row)?;
            }
            let mut labels = [0u32; MAX_DIMENSIONS];
            let labels =
                combination(self.catalog, combo, &mut labels).context(UnknownCombinationSnafu {
                    chunk: Some(decode.chunk),
                    combo,
                })?;
            self.add(labels, &values[..masked])?;
        }
        Ok(())
    }

    /// Adds the head's sums of the buckets.
    fn head(&mut self, head: &Head, tier: TierId) -> Result<(), SeriesQueryError> {
        let masked = self.fields.indices().count();
        let mut values = [0u64; MAX_FIELDS];
        let catalog = self.catalog;
        for (
            SeriesSource {
                tier: source,
                bucket,
            },
            accumulator,
        ) in head.series()
        {
            self.revoked
                .check("series::head:1")
                .context(CheckOpenSnafu)?;
            if source != tier || !self.buckets.contains(&bucket) {
                continue;
            }
            for (combos, columns) in accumulator.pages() {
                self.revoked
                    .check("series::head:2")
                    .context(CheckOpenSnafu)?;
                for (row, &combo) in combos.iter().enumerate() {
                    for (value, field) in values.iter_mut().zip(self.fields.indices()) {
                        *value = columns[field][row];
                    }
                    let mut labels = [0u32; MAX_DIMENSIONS];
                    // The head's combinations were checked against the catalog as they entered
                    // it, so one missing is damage, as in a chunk.
                    let labels = combination(catalog, combo, &mut labels)
                        .context(UnknownCombinationSnafu { chunk: None, combo })?;
                    self.add(labels, &values[..masked])?;
                }
            }
        }
        Ok(())
    }

    /// Adds a row of a combination with `labels` and the selected fields' `values` to the
    /// group of each request that admits it.
    fn add(&mut self, labels: &[u32], values: &[u64]) -> Result<(), SeriesQueryError> {
        // Before the filters, which may turn the folded rows away (§9.4).
        self.folded |= labels.contains(&0);
        let dimensions = self.dimensions;
        let label = |dimension: LabelId| {
            let position = dimensions
                .iter()
                .position(|&other| other == dimension)
                .expect("the query checked its dimensions");
            labels[position]
        };
        for (index, (request, list)) in self.requests.iter().zip(self.lists.iter_mut()).enumerate()
        {
            let admitted = request
                .filters
                .iter()
                .all(|filter| filter.ids.contains(&label(filter.dimension)) != filter.exclude);
            if !admitted {
                continue;
            }
            self.key.clear();
            match request.key {
                GroupKey::Total => {}
                GroupKey::Labels(dimensions) => {
                    for &dimension in dimensions {
                        self.key.push(KeyPart::Label(label(dimension)));
                    }
                }
                GroupKey::Mapped(maps) => {
                    for map in maps {
                        self.key.push(map.part(label(map.dimension)));
                    }
                }
            }
            list.add(&self.key, values, self.account)
                .context(ReserveGroupsSnafu { request: index })?;
        }
        Ok(())
    }
}

/// The label ids of `combo`, one per dimension, written into `out`; `None` when the catalog
/// does not hold it.
fn combination<'o>(
    catalog: &Catalog,
    combo: u32,
    out: &'o mut [u32; MAX_DIMENSIONS],
) -> Option<&'o [u32]> {
    let labels = catalog.combo(combo)?;
    let len = labels.len();
    for (slot, label) in out.iter_mut().zip(labels) {
        *slot = label;
    }
    Some(&out[..len])
}

/// What decoding a block of the open chunk needs.
struct Decode<'c, F> {
    /// Checked on both sides of each actual block decode.
    revoked: QueryCheck<'c>,
    chunk: u64,
    reader: &'c ChunkReader,
    source: &'c F,
    decoder: &'c mut Decoder,
}

/// The block decoder a query shares across its chunks, with what it holds reserved.
struct Decoder {
    blocks: BlockDecoder,
    account: MemoryAccount,
    _padding: Lease,
}

/// One column of the open chunk, with its decoded block.
struct Column<T> {
    index: u16,
    /// The rows of the decoded block; empty before the first of each chunk.
    rows: Range<u32>,
    block: IntBlock<T>,
}

impl<T: IntValue> Column<T> {
    fn new(index: u16) -> Self {
        Self {
            index,
            rows: 0..0,
            block: IntBlock::default(),
        }
    }

    /// Forgets the decoded block, at the start of a chunk. Its buffer stays, reserved.
    fn reset(&mut self) {
        self.rows = 0..0;
    }

    /// The column's value in `row`, decoding the block that holds it into the column's buffer
    /// when it is not the one decoded, with its memory reserved as [`decode_memory`] says.
    fn value(
        &mut self,
        decode: &mut Decode<'_, impl FragmentSource>,
        row: u32,
    ) -> Result<T, SeriesQueryError> {
        if !self.rows.contains(&row) {
            decode
                .revoked
                .check("series::value:1")
                .context(CheckOpenSnafu)?;
            let (chunk, column) = (decode.chunk, self.index);
            let blocks = decode
                .reader
                .blocks(column)
                .expect("the chunk has the column");
            let block = blocks.partition_point(|entry| entry.first_row + entry.rows <= row);
            let entry = blocks[block];
            let prepared = decode
                .decoder
                .blocks
                .prepare_ints(entry.shape(), &mut self.block)
                .context(ReserveBlockSnafu { chunk })?;
            self.rows = 0..0;
            let block = block as u32;
            decode
                .reader
                .read_prepared_ints(decode.source, column, block, prepared)
                .context(ReadBlockSnafu {
                    chunk,
                    column,
                    block,
                })?;
            decode
                .revoked
                .check("series::value:2")
                .context(CheckOpenSnafu)?;
            ensure!(
                !self.block.validity().has_nulls(),
                NullValuesSnafu { chunk, column }
            );
            self.rows = entry.first_row..entry.first_row + entry.rows;
        }
        Ok(self.block.values()[(row - self.rows.start) as usize])
    }
}

/// A series query that failed. Nothing it reserved stays reserved.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum SeriesQueryError {
    /// The owner retired the instance before this operation could continue.
    #[snafu(display("the store is closed to this operation"))]
    CheckOpen {
        source: crate::storage::StorageError,
    },

    #[snafu(display("the schema has no series tier {tier:?}"))]
    UnknownTier { tier: TierId },

    #[snafu(display("field mask {mask:#06x} selects a field past the series's {fields}"))]
    UnknownField { mask: u16, fields: usize },

    #[snafu(display("request {request} reads {dimension:?}, which is not a series dimension"))]
    NotADimension { request: usize, dimension: LabelId },

    #[snafu(display("failed to read the deletion watermark of the tier"))]
    ReadWatermark { source: ManifestError },

    #[snafu(display("no room in the query budget to start the scan"))]
    ReserveScan { source: BudgetExceeded },

    /// The query budget has no room for the block decoder's zstd context.
    #[snafu(display("no room in the query budget for the block decoder"))]
    ReserveDecoder { source: BudgetExceeded },

    /// The query budget has no room for the fragment the backend lends while the chunk's
    /// reader decodes from it.
    #[snafu(display("no room in the query budget for a fragment of chunk {chunk}"))]
    ReserveFragment { chunk: u64, source: BudgetExceeded },

    #[snafu(display("no room in the query budget for the list of each request's groups"))]
    ReserveLists { source: BudgetExceeded },

    /// The query budget has no room for what decoding a block of the chunk holds.
    #[snafu(display("no room in the query budget for a decoded block of chunk {chunk}"))]
    ReserveBlock { chunk: u64, source: BudgetExceeded },

    /// The groups of a request outgrew the budget (§8.2).
    #[snafu(display("no room in the query budget for the groups of request {request}"))]
    ReserveGroups {
        request: usize,
        source: BudgetExceeded,
    },

    /// zstd could not allocate its decompression context.
    #[snafu(display("failed to create a block decoder"))]
    CreateDecoder { source: std::io::Error },

    #[snafu(display("failed to list the chunks of the tier"))]
    ListChunks { source: ManifestError },

    #[snafu(display("failed to open chunk {chunk}"))]
    OpenChunk { chunk: u64, source: ReadError },

    #[snafu(display("failed to read block {block} of column {column} of chunk {chunk}"))]
    ReadBlock {
        chunk: u64,
        column: u16,
        block: u32,
        source: ReadError,
    },

    /// The chunk's columns are not those of a series chunk of the schema.
    #[snafu(display("chunk {chunk} does not have the columns of a series chunk"))]
    ChunkColumns { chunk: u64 },

    /// A block of a column that has no nulls holds some.
    #[snafu(display("column {column} of chunk {chunk} has nulls"))]
    NullValues { chunk: u64, column: u16 },

    /// A row refers to a combination the snapshot's catalog does not hold, although every
    /// combination a chunk or the head refers to stays until they no longer do (§5.3).
    #[snafu(display(
        "{} refers to combination {combo}, which the catalog does not hold",
        rows_of(*chunk)
    ))]
    UnknownCombination {
        /// The chunk of the row; `None` for the head.
        chunk: Option<u64>,
        combo: u32,
    },
}

impl SeriesQueryError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::CheckOpen { source } => source.kind(),
            Self::UnknownTier { .. }
            | Self::UnknownField { .. }
            | Self::NotADimension { .. }
            | Self::CreateDecoder { .. } => ErrorKind::Local,
            Self::ReserveScan { source }
            | Self::ReserveDecoder { source }
            | Self::ReserveFragment { source, .. }
            | Self::ReserveLists { source }
            | Self::ReserveBlock { source, .. }
            | Self::ReserveGroups { source, .. } => source.kind(),
            Self::ListChunks { source } | Self::ReadWatermark { source } => source.kind(),
            Self::OpenChunk { source, .. } | Self::ReadBlock { source, .. } => source.kind(),
            Self::ChunkColumns { .. }
            | Self::NullValues { .. }
            | Self::UnknownCombination { .. } => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
