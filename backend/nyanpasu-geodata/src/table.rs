//! Address-range tables: sorted step lists mapping each address to a record id.
use std::net::IpAddr;

use bytemuck::Pod;

use crate::{GeoError, GeoResult, scratch::ScratchVec};

/// Record id stored per range. `NONE` marks addresses without a record;
/// `FINE` sends a /64 block to the full-width table.
pub(crate) trait Id: Pod + Eq + std::fmt::Debug {
    const NONE: Self;
    const FINE: Self;

    /// `None` once `index` would collide with the reserved ids.
    fn new(index: usize) -> Option<Self>;
    fn index(self) -> usize;
}

macro_rules! id {
    ($ty:ty) => {
        impl Id for $ty {
            const NONE: Self = <$ty>::MAX;
            const FINE: Self = <$ty>::MAX - 1;

            fn new(index: usize) -> Option<Self> {
                Self::try_from(index).ok().filter(|id| *id < Self::FINE)
            }

            fn index(self) -> usize {
                self as usize
            }
        }
    };
}
id!(u16);
id!(u32);

/// An inclusive address range in the table of its family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Span {
    V4(u32, u32),
    V6(u128, u128),
}

impl Span {
    pub(crate) fn v4(start: u32, prefix: u8) -> GeoResult<Span> {
        if prefix > 32 {
            return Err(GeoError::Malformed("IPv4 prefix longer than 32 bits"));
        }
        let host = u32::MAX.checked_shr(u32::from(prefix)).unwrap_or(0);
        Ok(Span::V4(start & !host, start | host))
    }

    /// IPv6 addresses inside `::/96` are looked up as IPv4, the way an MMDB
    /// search tree nests its IPv4 subtree; a network covering `::/96` spans
    /// the whole IPv4 table as well.
    pub(crate) fn v6(start: u128, prefix: u8) -> GeoResult<(Option<Span>, Option<Span>)> {
        if prefix > 128 {
            return Err(GeoError::Malformed("IPv6 prefix longer than 128 bits"));
        }
        let host = u128::MAX.checked_shr(u32::from(prefix)).unwrap_or(0);
        let (start, end) = (start & !host, start | host);
        Ok(if end >> 32 == 0 {
            (Some(Span::V4(start as u32, end as u32)), None)
        } else if start >> 32 == 0 {
            (Some(Span::V4(0, u32::MAX)), Some(Span::V6(start, end)))
        } else {
            (None, Some(Span::V6(start, end)))
        })
    }
}

pub(crate) struct RangeTable<I> {
    v4: Steps<u32, I>,
    /// Keyed by the upper 64 bits; `FINE` blocks continue in `fine`.
    v6: Steps<u64, I>,
    fine: Steps<u128, I>,
}

struct Steps<K, I> {
    starts: Box<[K]>,
    ids: Box<[I]>,
}

impl<I: Id> RangeTable<I> {
    pub(crate) fn get(&self, ip: IpAddr) -> Option<I> {
        let id = match ip.to_canonical() {
            IpAddr::V4(ip) => self.v4.get(u32::from(ip)),
            IpAddr::V6(ip) => {
                let key = u128::from(ip);
                if key >> 32 == 0 {
                    self.v4.get(key as u32)
                } else {
                    match self.v6.get((key >> 64) as u64) {
                        id if id == I::FINE => self.fine.get(key),
                        id => id,
                    }
                }
            }
        };
        (id != I::NONE).then_some(id)
    }
}

impl<K: Copy + Ord, I: Copy> Steps<K, I> {
    fn get(&self, key: K) -> I {
        // Every list starts at address zero, so the index is at least one.
        self.ids[self.starts.partition_point(|start| *start <= key) - 1]
    }
}

/// Accepts ranges and steps in ascending address order per family.
pub(crate) struct RangeTableBuilder<I> {
    v4: StepList<u32, I>,
    v6: StepList<u128, I>,
}

struct StepList<K, I> {
    starts: ScratchVec<K>,
    ids: ScratchVec<I>,
}

impl<I: Id> RangeTableBuilder<I> {
    pub(crate) fn new() -> GeoResult<Self> {
        Ok(Self {
            v4: StepList::new(0)?,
            v6: StepList::new(0)?,
        })
    }

    pub(crate) fn insert(&mut self, span: Span, id: I) -> GeoResult<()> {
        match span {
            Span::V4(start, end) => {
                self.v4.push(start, id)?;
                match end.checked_add(1) {
                    Some(next) => self.v4.push(next, I::NONE),
                    None => Ok(()),
                }
            }
            Span::V6(start, end) => {
                self.v6.push(start, id)?;
                match end.checked_add(1) {
                    Some(next) => self.v6.push(next, I::NONE),
                    None => Ok(()),
                }
            }
        }
    }

    /// From the first address of `at` on, addresses map to `id`.
    pub(crate) fn step(&mut self, at: Span, id: I) -> GeoResult<()> {
        match at {
            Span::V4(pos, _) => self.v4.push(pos, id),
            Span::V6(pos, _) => self.v6.push(pos, id),
        }
    }

    pub(crate) fn finish(self) -> GeoResult<RangeTable<I>> {
        let (v6, fine) = split_v6(&self.v6)?;
        Ok(RangeTable {
            v4: self.v4.into_steps(),
            v6: v6.into_steps(),
            fine: fine.into_steps(),
        })
    }
}

impl<K: Pod + Ord, I: Id> StepList<K, I> {
    fn new(zero: K) -> GeoResult<Self> {
        let mut list = Self {
            starts: ScratchVec::new(),
            ids: ScratchVec::new(),
        };
        list.starts.push(zero)?;
        list.ids.push(I::NONE)?;
        Ok(list)
    }

    fn push(&mut self, pos: K, id: I) -> GeoResult<()> {
        let last = *self
            .starts
            .as_slice()
            .last()
            .expect("a step list is never empty");
        if pos < last {
            return Err(GeoError::Malformed("address ranges out of order"));
        }
        if pos == last {
            self.starts.pop();
            self.ids.pop();
        }
        if self.ids.as_slice().last() != Some(&id) {
            self.starts.push(pos)?;
            self.ids.push(id)?;
        }
        Ok(())
    }

    fn into_steps(self) -> Steps<K, I> {
        Steps {
            starts: self.starts.as_slice().into(),
            ids: self.ids.as_slice().into(),
        }
    }
}

/// Keys whole /64 blocks by their upper half; a block with any boundary
/// inside it is marked `FINE` and copied into the full-width list.
fn split_v6<I: Id>(list: &StepList<u128, I>) -> GeoResult<(StepList<u64, I>, StepList<u128, I>)> {
    let (starts, ids) = (list.starts.as_slice(), list.ids.as_slice());
    let mut coarse = StepList::new(0)?;
    let mut fine = StepList::new(0)?;
    let mut i = 0;
    while i < starts.len() {
        let block = (starts[i] >> 64) as u64;
        let end = i + starts[i..].partition_point(|start| (start >> 64) as u64 == block);
        if starts[i..end].iter().all(|start| *start as u64 == 0) {
            // Starts are distinct, so an aligned block holds exactly one.
            coarse.push(block, ids[i])?;
        } else {
            coarse.push(block, I::FINE)?;
            if starts[i] as u64 != 0 {
                let open = if i == 0 { I::NONE } else { ids[i - 1] };
                fine.push(u128::from(block) << 64, open)?;
            }
            for k in i..end {
                fine.push(starts[k], ids[k])?;
            }
            if let Some(next) = block.checked_add(1) {
                coarse.push(next, ids[end - 1])?;
            }
        }
        i = end;
    }
    Ok((coarse, fine))
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(s.parse::<Ipv4Addr>().unwrap())
    }

    fn v6(s: &str) -> IpAddr {
        IpAddr::V6(s.parse::<Ipv6Addr>().unwrap())
    }

    fn bits(s: &str) -> u128 {
        u128::from(s.parse::<Ipv6Addr>().unwrap())
    }

    fn insert_v6(builder: &mut RangeTableBuilder<u16>, start: &str, prefix: u8, id: u16) {
        let (low, high) = Span::v6(bits(start), prefix).unwrap();
        for span in low.into_iter().chain(high) {
            builder.insert(span, id).unwrap();
        }
    }

    #[test]
    fn v4_lookup_finds_ranges_and_reports_gaps() {
        let mut builder = RangeTableBuilder::<u16>::new().unwrap();
        builder
            .insert(Span::v4(0x0100_0100, 24).unwrap(), 1)
            .unwrap(); // 1.0.1.0/24
        builder
            .insert(Span::v4(0x0800_0000, 8).unwrap(), 2)
            .unwrap(); // 8.0.0.0/8
        let table = builder.finish().unwrap();

        assert_eq!(table.get(v4("0.0.0.0")), None);
        assert_eq!(table.get(v4("1.0.0.255")), None);
        assert_eq!(table.get(v4("1.0.1.0")), Some(1));
        assert_eq!(table.get(v4("1.0.1.255")), Some(1));
        assert_eq!(table.get(v4("1.0.2.0")), None);
        assert_eq!(table.get(v4("8.8.8.8")), Some(2));
        assert_eq!(table.get(v4("9.0.0.0")), None);
        assert_eq!(table.get(v4("255.255.255.255")), None);
    }

    #[test]
    fn adjacent_ranges_with_one_id_share_a_step() {
        let mut builder = RangeTableBuilder::<u16>::new().unwrap();
        builder
            .insert(Span::v4(0x0A00_0000, 9).unwrap(), 3)
            .unwrap(); // 10.0.0.0/9
        builder
            .insert(Span::v4(0x0A80_0000, 9).unwrap(), 3)
            .unwrap(); // 10.128.0.0/9
        let table = builder.finish().unwrap();

        // leading gap, 10.0.0.0/8, trailing gap
        assert_eq!(table.v4.starts.len(), 3);
        assert_eq!(table.get(v4("10.127.255.255")), Some(3));
        assert_eq!(table.get(v4("10.128.0.0")), Some(3));
    }

    #[test]
    fn whole_address_spaces_cover_their_last_address() {
        let mut builder = RangeTableBuilder::<u16>::new().unwrap();
        builder.insert(Span::v4(0, 0).unwrap(), 1).unwrap();
        let table = builder.finish().unwrap();
        assert_eq!(table.get(v4("255.255.255.255")), Some(1));

        // `::/0` also covers the nested IPv4 space.
        let mut builder = RangeTableBuilder::<u16>::new().unwrap();
        insert_v6(&mut builder, "::", 0, 2);
        let table = builder.finish().unwrap();
        assert_eq!(
            table.get(v6("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff")),
            Some(2)
        );
        assert_eq!(table.get(v6("2001:db8::1")), Some(2));
        assert_eq!(table.get(v4("255.255.255.255")), Some(2));
    }

    #[test]
    fn v6_lookup_finds_ranges_and_reports_gaps() {
        let mut builder = RangeTableBuilder::<u16>::new().unwrap();
        insert_v6(&mut builder, "2001:db8::", 32, 1);
        insert_v6(&mut builder, "2400:cb00::", 48, 2);
        let table = builder.finish().unwrap();

        assert_eq!(table.get(v6("2001:db7:ffff::")), None);
        assert_eq!(table.get(v6("2001:db8:1234::1")), Some(1));
        assert_eq!(table.get(v6("2001:db9::")), None);
        assert_eq!(table.get(v6("2400:cb00:0:ffff::1")), Some(2));
        assert_eq!(table.get(v6("2400:cb00:1::")), None);
    }

    #[test]
    fn prefixes_longer_than_64_bits_resolve_inside_their_block() {
        // 2001:db8::/48 -> 1, except 2001:db8:0:5::80/121 -> 2.
        let mut builder = RangeTableBuilder::<u16>::new().unwrap();
        let hole = bits("2001:db8:0:5::80");
        builder
            .insert(Span::V6(bits("2001:db8::"), hole - 1), 1)
            .unwrap();
        builder.insert(Span::V6(hole, hole + 127), 2).unwrap();
        builder
            .insert(
                Span::V6(hole + 128, bits("2001:db8:0:ffff:ffff:ffff:ffff:ffff")),
                1,
            )
            .unwrap();
        let table = builder.finish().unwrap();

        assert_eq!(table.get(v6("2001:db8:0:4::1")), Some(1));
        assert_eq!(table.get(v6("2001:db8:0:5::")), Some(1));
        assert_eq!(table.get(v6("2001:db8:0:5::7f")), Some(1));
        assert_eq!(table.get(v6("2001:db8:0:5::80")), Some(2));
        assert_eq!(table.get(v6("2001:db8:0:5::ff")), Some(2));
        assert_eq!(table.get(v6("2001:db8:0:5::100")), Some(1));
        assert_eq!(table.get(v6("2001:db8:0:6::")), Some(1));
        assert_eq!(table.get(v6("2001:db8:1::")), None);
    }

    #[test]
    fn a_fine_block_starts_with_the_range_open_at_its_first_address() {
        // The block 2001:db8:0:5::/64 opens inside the range of id 1 and
        // closes inside a gap.
        let mut builder = RangeTableBuilder::<u16>::new().unwrap();
        builder
            .insert(Span::V6(bits("2001:db8::"), bits("2001:db8:0:5::ff")), 1)
            .unwrap();
        builder
            .insert(
                Span::V6(bits("2001:db8:0:5::200"), bits("2001:db8:0:5::2ff")),
                2,
            )
            .unwrap();
        let table = builder.finish().unwrap();

        assert_eq!(table.get(v6("2001:db8:0:5::")), Some(1));
        assert_eq!(table.get(v6("2001:db8:0:5::100")), None);
        assert_eq!(table.get(v6("2001:db8:0:5::2ff")), Some(2));
        assert_eq!(table.get(v6("2001:db8:0:5::300")), None);
        assert_eq!(table.get(v6("2001:db8:0:6::")), None);
    }

    #[test]
    fn ipv4_compatible_and_mapped_queries_use_the_v4_table() {
        let mut builder = RangeTableBuilder::<u16>::new().unwrap();
        builder
            .insert(Span::v4(0x0102_0300, 24).unwrap(), 1)
            .unwrap(); // 1.2.3.0/24
        let table = builder.finish().unwrap();

        assert_eq!(table.get(v6("::1.2.3.4")), Some(1));
        assert_eq!(table.get(v6("::ffff:1.2.3.4")), Some(1));
    }

    #[test]
    fn v6_networks_inside_the_ipv4_compatible_prefix_become_v4_spans() {
        assert_eq!(
            Span::v6(bits("::1.2.3.0"), 120).unwrap(),
            (Some(Span::V4(0x0102_0300, 0x0102_03ff)), None)
        );
        assert_eq!(
            Span::v6(bits("::"), 64).unwrap(),
            (
                Some(Span::V4(0, u32::MAX)),
                Some(Span::V6(0, (1 << 64) - 1))
            )
        );
        assert_eq!(
            Span::v6(bits("2001:db8::"), 32).unwrap(),
            (
                None,
                Some(Span::V6(
                    bits("2001:db8::"),
                    bits("2001:db8:ffff:ffff:ffff:ffff:ffff:ffff")
                ))
            )
        );
    }

    #[test]
    fn host_bits_of_a_network_are_ignored() {
        assert_eq!(
            Span::v4(0x0102_0304, 24).unwrap(),
            Span::V4(0x0102_0300, 0x0102_03ff)
        );
        assert_eq!(
            Span::v4(0x0102_0304, 32).unwrap(),
            Span::V4(0x0102_0304, 0x0102_0304)
        );
    }

    #[test]
    fn prefixes_beyond_the_address_width_are_rejected() {
        assert!(matches!(Span::v4(0, 33), Err(GeoError::Malformed(_))));
        assert!(matches!(Span::v6(0, 129), Err(GeoError::Malformed(_))));
    }

    #[test]
    fn ranges_out_of_order_are_rejected() {
        let mut builder = RangeTableBuilder::<u16>::new().unwrap();
        builder
            .insert(Span::v4(0x0800_0000, 8).unwrap(), 1)
            .unwrap();
        assert!(matches!(
            builder.insert(Span::v4(0x0100_0000, 8).unwrap(), 2),
            Err(GeoError::Malformed(_))
        ));
    }

    #[test]
    fn ids_stop_before_the_reserved_values() {
        assert_eq!(u16::new(65533), Some(65533));
        assert_eq!(u16::new(65534), None);
        assert_eq!(u32::new(70_000), Some(70_000));
    }
}
