//! The system allocator keeps freed heap pages for reuse, so the heap peak of
//! a build stays behind as process footprint. Large build buffers therefore
//! live outside the heap; what a build leaves on the heap beyond the index
//! itself must stay small.
mod support;

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use nyanpasu_geodata::{AsnIndex, IpIndex, SiteIndex};
use support::{
    mmdb::{MmdbWriter, Value, array, map, s},
    proto::{DOMAIN, FULL, Rule, geoip, geosite},
};

thread_local! {
    // Builds run on the calling thread; per-thread counts keep concurrent
    // tests out of each other's figures.
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

struct Counting;

fn count(delta: isize) {
    let _ = LIVE.try_with(|live| {
        live.set(live.get() + delta);
        let _ = PEAK.try_with(|peak| peak.set(peak.get().max(live.get())));
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size() as isize);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count(-(layout.size() as isize));
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(size as isize - layout.size() as isize);
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const TRANSIENT_LIMIT: isize = 256 << 10;

/// Heap bytes the build held at its peak beyond what its result keeps.
fn transient<T>(build: impl FnOnce() -> T) -> (T, isize) {
    let base = LIVE.with(Cell::get);
    PEAK.with(|peak| peak.set(base));
    let built = build();
    let kept = LIVE.with(Cell::get) - base;
    let peak = PEAK.with(Cell::get) - base;
    (built, peak - kept)
}

#[test]
fn a_geoip_dat_build_keeps_its_buffers_off_the_heap() {
    let codes: Vec<String> = (0..50).map(|i| format!("c{i}")).collect();
    let cidrs: Vec<Vec<String>> = (0..50u32)
        .map(|code| {
            (0..1000u32)
                .map(|i| {
                    let n = code * 1000 + i;
                    if i % 2 == 0 {
                        format!("{}.{}.{}.0/24", 1 + n / 65536, (n / 256) % 256, n % 256)
                    } else {
                        format!("2400:{:x}:{:x}::/48", n / 65536 + 1, n % 65536)
                    }
                })
                .collect()
        })
        .collect();
    let cidrs: Vec<Vec<&str>> = cidrs
        .iter()
        .map(|c| c.iter().map(String::as_str).collect())
        .collect();
    let entries: Vec<(&str, &[&str])> = codes
        .iter()
        .map(String::as_str)
        .zip(cidrs.iter().map(Vec::as_slice))
        .collect();
    let dat = geoip(&entries);

    let (index, transient) = transient(|| IpIndex::from_geoip_dat(&dat).unwrap());
    assert!(index.lookup("1.0.0.1".parse().unwrap()).is_some());
    assert!(
        transient <= TRANSIENT_LIMIT,
        "{transient} transient heap bytes"
    );
}

#[test]
fn an_mmdb_build_keeps_its_buffers_off_the_heap() {
    let mut writer = MmdbWriter::new("Meta-geoip0");
    for n in 0..20_000u32 {
        let tags = if n % 3 == 0 {
            array(&["us", "google"])
        } else {
            s(["cn", "jp", "de"][n as usize % 3])
        };
        writer.insert(
            &format!("{}.{}.{}.0/24", 1 + n / 65536, (n / 256) % 256, n % 256),
            tags,
        );
        writer.insert(&format!("2400:{:x}::/32", n + 1), s("hk"));
    }
    let db = writer.build();

    let (index, transient) = transient(|| IpIndex::from_mmdb(&db).unwrap());
    assert!(index.lookup("1.0.0.1".parse().unwrap()).is_some());
    assert!(
        transient <= TRANSIENT_LIMIT,
        "{transient} transient heap bytes"
    );
}

#[test]
fn a_geosite_build_keeps_its_buffers_off_the_heap() {
    // The fst builder's node registry is the one buffer the crate cannot
    // place; its size is bounded, and at this size the kept index outgrows it.
    let values: Vec<String> = (0..120_000)
        .map(|i| format!("host{i}.example{}.com", i % 97))
        .collect();
    let rules: Vec<Rule<'_>> = values
        .iter()
        .enumerate()
        .map(|(i, value)| {
            (
                if i % 5 == 0 { FULL } else { DOMAIN },
                value.as_str(),
                &[][..],
            )
        })
        .collect();
    let dat = geosite(&[("big", rules.as_slice())]);

    let (index, transient) = transient(|| SiteIndex::from_geosite_dat(&dat, |_| true).unwrap());
    assert_eq!(index.lookup("a.host1.example1.com").len(), 1);
    assert!(
        transient <= TRANSIENT_LIMIT,
        "{transient} transient heap bytes"
    );
}

#[test]
fn an_asn_build_keeps_its_buffers_off_the_heap() {
    let mut writer = MmdbWriter::new("GeoLite2-ASN");
    for n in 0..6_000u32 {
        let record = map(&[
            ("autonomous_system_number", Value::U32(n + 1)),
            (
                "autonomous_system_organization",
                s(&format!("Organization {n}, Inc.")),
            ),
        ]);
        writer.insert(
            &format!("{}.{}.{}.0/24", 1 + n / 65536, (n / 256) % 256, n % 256),
            record,
        );
    }
    let db = writer.build();

    let (index, transient) = transient(|| AsnIndex::from_mmdb(&db).unwrap());
    assert_eq!(index.lookup("1.0.0.1".parse().unwrap()).unwrap().number, 1);
    assert!(
        transient <= TRANSIENT_LIMIT,
        "{transient} transient heap bytes"
    );
}
