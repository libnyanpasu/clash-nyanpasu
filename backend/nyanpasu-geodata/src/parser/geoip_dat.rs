//! `GeoIP.dat` (V2Ray `GeoIPList`) lists CIDRs per code; inverting it gives
//! each address range the set of codes containing it.
use bytemuck::{Pod, Zeroable};

use crate::{
    GeoError, GeoResult,
    collection::{
        range_table::{Id, RangeTable, RangeTableBuilder, Span},
        scratch::ScratchVec,
        tags::{MAX_SET, TagInterner, TagStore},
    },
    parser::proto::Fields,
};

/// A code entering or leaving at `pos`. Packed: a full list holds about a
/// million of them per address family.
#[repr(C, packed)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Event {
    pos: u128,
    tag: u16,
    /// 1 when the code starts covering `pos`, 0 when it stops.
    open: u8,
}

pub(crate) fn compile(bytes: &[u8]) -> GeoResult<(RangeTable<u16>, TagStore)> {
    let mut tags = TagInterner::default();
    let (mut v4, mut v6) = (0, 0);
    spans(bytes, &mut tags, |_, span| {
        match span {
            Span::V4(..) => v4 += 1,
            Span::V6(..) => v6 += 1,
        }
        Ok(())
    })?;
    let mut builder = RangeTableBuilder::new()?;
    // One family at a time, in a buffer sized up front, bounds the events in
    // memory to one family's.
    for (family_v6, count) in [(false, v4), (true, v6)] {
        let mut events = ScratchVec::with_capacity(count * 2)?;
        spans(bytes, &mut tags, |tag, span| {
            let (start, end) = match (span, family_v6) {
                (Span::V4(start, end), false) => (u128::from(start), u128::from(end)),
                (Span::V6(start, end), true) => (start, end),
                _ => return Ok(()),
            };
            events.push(Event {
                pos: start,
                tag,
                open: 1,
            })?;
            if let Some(next) = end.checked_add(1) {
                events.push(Event {
                    pos: next,
                    tag,
                    open: 0,
                })?;
            }
            Ok(())
        })?;
        sweep(events.as_mut_slice(), family_v6, &mut tags, &mut builder)?;
    }
    Ok((builder.finish()?, tags.finish()))
}

/// Calls `f` with the code tag and table span of every CIDR.
fn spans(
    bytes: &[u8],
    tags: &mut TagInterner,
    mut f: impl FnMut(u16, Span) -> GeoResult<()>,
) -> GeoResult<()> {
    for field in Fields::new(bytes) {
        let (number, entry) = field?;
        if number != 1 {
            continue;
        }
        let entry = entry.bytes()?;
        let mut code: &[u8] = &[];
        for field in Fields::new(entry) {
            if let (1, value) = field? {
                code = value.bytes()?;
            }
        }
        if code.is_empty() {
            continue;
        }
        let code = std::str::from_utf8(code)
            .map_err(|_| GeoError::Malformed("GeoIP code is not UTF-8"))?;
        let tag = tags.tag(code)?;
        for field in Fields::new(entry) {
            let (number, value) = field?;
            if number != 2 {
                continue;
            }
            let (mut ip, mut prefix): (&[u8], u64) = (&[], 0);
            for field in Fields::new(value.bytes()?) {
                match field? {
                    (1, value) => ip = value.bytes()?,
                    (2, value) => prefix = value.varint()?,
                    _ => {}
                }
            }
            let prefix = u8::try_from(prefix)
                .map_err(|_| GeoError::Malformed("CIDR prefix out of range"))?;
            let (low, high) = if let Ok(ip) = <[u8; 4]>::try_from(ip) {
                (Some(Span::v4(u32::from_be_bytes(ip), prefix)?), None)
            } else if let Ok(ip) = <[u8; 16]>::try_from(ip) {
                Span::v6(u128::from_be_bytes(ip), prefix)?
            } else {
                return Err(GeoError::Malformed(
                    "CIDR address is neither 4 nor 16 bytes",
                ));
            };
            for span in low.into_iter().chain(high) {
                f(tag, span)?;
            }
        }
    }
    Ok(())
}

fn sweep(
    events: &mut [Event],
    v6: bool,
    tags: &mut TagInterner,
    builder: &mut RangeTableBuilder<u16>,
) -> GeoResult<()> {
    events.sort_unstable_by_key(|event| event.pos);
    // Per code, how many of its ranges cover the sweep position; a code may
    // list overlapping ranges.
    let mut depth = vec![0u32; tags.len()];
    let mut active: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < events.len() {
        let pos = events[i].pos;
        while let Some(event) = events.get(i).filter(|event| event.pos == pos) {
            let (tag, open) = (event.tag, event.open);
            let depth = &mut depth[usize::from(tag)];
            if open == 1 {
                *depth += 1;
                if *depth == 1 {
                    if active.len() == MAX_SET {
                        return Err(GeoError::TooMany("tags on one address"));
                    }
                    active.insert(active.partition_point(|t| *t < tag), tag);
                }
            } else {
                *depth -= 1;
                if *depth == 0 {
                    active.retain(|t| *t != tag);
                }
            }
            i += 1;
        }
        let id = if active.is_empty() {
            u16::NONE
        } else {
            tags.set(&active)?
        };
        if v6 {
            builder.step(Span::V6(pos, pos), id)?;
        } else if let Ok(pos) = u32::try_from(pos) {
            builder.step(Span::V4(pos, pos), id)?;
        }
    }
    Ok(())
}
