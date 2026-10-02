//! Compiles an MMDB search tree into a range table.
use std::net::IpAddr;

use maxminddb::{LookupResult, Metadata, Reader, WithinOptions};

use crate::{
    GeoError, GeoResult,
    collection::{
        range_table::{Id, RangeSteps, RangeTableBuilder, Span},
        scratch::ScratchVec,
    },
};

/// Calls `record` once per distinct data record (barring cache evictions) and
/// stores the id it returns for every network pointing at that record;
/// `Id::NONE` leaves the networks out.
pub(crate) fn compile<'a, I: Id>(
    bytes: &'a [u8],
    reader: &'a Reader<&'a [u8]>,
    mut record: impl FnMut(&LookupResult<'a, &'a [u8]>) -> GeoResult<I>,
) -> GeoResult<RangeSteps<I>> {
    check_tree(bytes, reader.metadata())?;
    let mut cache = RecordCache::new()?;
    let mut builder = RangeTableBuilder::new()?;
    for item in reader.networks(WithinOptions::default())? {
        let item = item?;
        let Some(offset) = item.offset() else {
            continue;
        };
        let id = match cache.get(offset) {
            Some(id) => id,
            None => {
                let id = record(&item)?;
                cache.put(offset, id);
                id
            }
        };
        if id == I::NONE {
            continue;
        }
        let network = item.network()?;
        let (low, high) = match network.ip() {
            IpAddr::V4(ip) => (Some(Span::v4(u32::from(ip), network.prefix())?), None),
            IpAddr::V6(ip) => Span::v6(u128::from(ip), network.prefix())?,
        };
        for span in low.into_iter().chain(high) {
            builder.insert(span, id)?;
        }
    }
    builder.finish()
}

/// Walking a search tree visits every path, so nodes shared by crafted
/// records would make the walk exponential. Only the IPv4 start node may have
/// several parents: the aliases pointing at it are skipped by
/// `Reader::networks`. Cycles need no check; the walk fails on the first path
/// that outgrows the address width.
fn check_tree(bytes: &[u8], metadata: &Metadata) -> GeoResult<()> {
    let nodes = metadata.node_count as usize;
    let width = match metadata.record_size {
        24 => 6,
        28 => 7,
        32 => 8,
        _ => return Err(GeoError::Malformed("unsupported MMDB record size")),
    };
    let tree = nodes
        .checked_mul(width)
        .and_then(|len| bytes.get(..len))
        .ok_or(GeoError::Malformed(
            "search tree runs past the end of the file",
        ))?;
    let record = |node: usize, right: bool| -> usize {
        let n = &tree[node * width..(node + 1) * width];
        let value = match (width, right) {
            (6, false) => u32::from_be_bytes([0, n[0], n[1], n[2]]),
            (6, true) => u32::from_be_bytes([0, n[3], n[4], n[5]]),
            (7, false) => u32::from_be_bytes([n[3] >> 4, n[0], n[1], n[2]]),
            (7, true) => u32::from_be_bytes([n[3] & 0x0f, n[4], n[5], n[6]]),
            (_, false) => u32::from_be_bytes([n[0], n[1], n[2], n[3]]),
            (_, true) => u32::from_be_bytes([n[4], n[5], n[6], n[7]]),
        };
        value as usize
    };
    let mut ipv4_start = usize::MAX;
    if metadata.ip_version == 6 {
        ipv4_start = 0;
        for _ in 0..96 {
            if ipv4_start >= nodes {
                break;
            }
            ipv4_start = record(ipv4_start, false);
        }
    }
    let mut parented = ScratchVec::<u64>::zeroed(nodes.div_ceil(64))?;
    let parented = parented.as_mut_slice();
    for node in 0..nodes {
        for right in [false, true] {
            let child = record(node, right);
            if child >= nodes || child == ipv4_start {
                continue;
            }
            let (word, bit) = (child / 64, 1u64 << (child % 64));
            if parented[word] & bit != 0 {
                return Err(GeoError::Malformed("search tree shares a node"));
            }
            parented[word] |= bit;
        }
    }
    Ok(())
}

/// Direct-mapped, so a city database with millions of records costs a fixed
/// slot count instead of an entry per record.
struct RecordCache<I> {
    /// Data offset plus one; zero marks an empty slot.
    keys: ScratchVec<u64>,
    ids: ScratchVec<I>,
}

const SLOT_BITS: u32 = 16;

impl<I: Id> RecordCache<I> {
    fn new() -> GeoResult<Self> {
        Ok(Self {
            keys: ScratchVec::zeroed(1 << SLOT_BITS)?,
            ids: ScratchVec::zeroed(1 << SLOT_BITS)?,
        })
    }

    fn slot(key: u64) -> usize {
        (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - SLOT_BITS)) as usize
    }

    fn get(&self, offset: usize) -> Option<I> {
        let key = offset as u64 + 1;
        let slot = Self::slot(key);
        (self.keys.as_slice()[slot] == key).then(|| self.ids.as_slice()[slot])
    }

    fn put(&mut self, offset: usize, id: I) {
        let key = offset as u64 + 1;
        let slot = Self::slot(key);
        self.keys.as_mut_slice()[slot] = key;
        self.ids.as_mut_slice()[slot] = id;
    }
}
