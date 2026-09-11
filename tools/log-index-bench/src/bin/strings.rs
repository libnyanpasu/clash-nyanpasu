//! Controlled comparison: identical rows/query algorithm, only target storage varies.
use gxhash::GxBuildHasher;
use serde_json::json;
use std::{collections::HashMap, hint::black_box, time::Instant};

type Hasher = GxBuildHasher;
trait Pool {
    fn new() -> Self;
    type Key;
    fn intern(&mut self, value: &str) -> Self::Key;
    fn resolve<'a>(&'a self, key: &'a Self::Key) -> &'a str;
}
#[derive(Default)]
struct Owned;
impl Pool for Owned {
    fn new() -> Self {
        Self
    }
    type Key = String;
    fn intern(&mut self, value: &str) -> String {
        value.into()
    }
    fn resolve<'a>(&'a self, key: &'a String) -> &'a str {
        key
    }
}
#[derive(Default)]
struct Small;
impl Pool for Small {
    fn new() -> Self {
        Self
    }
    type Key = smol_str::SmolStr;
    fn intern(&mut self, value: &str) -> Self::Key {
        value.into()
    }
    fn resolve<'a>(&'a self, key: &'a Self::Key) -> &'a str {
        key
    }
}
#[derive(Default)]
struct Standard {
    map: HashMap<std::sync::Arc<str>, u32, Hasher>,
    names: Vec<std::sync::Arc<str>>,
}
impl Pool for Standard {
    fn new() -> Self {
        Self::default()
    }
    type Key = u32;
    fn intern(&mut self, value: &str) -> u32 {
        if let Some(id) = self.map.get(value) {
            return *id;
        }
        let id = u32::try_from(self.names.len()).unwrap();
        let value: std::sync::Arc<str> = value.into();
        self.map.insert(value.clone(), id);
        self.names.push(value);
        id
    }
    fn resolve<'a>(&'a self, key: &'a u32) -> &'a str {
        &self.names[*key as usize]
    }
}
impl Pool for lasso::Rodeo<lasso::Spur, Hasher> {
    fn new() -> Self {
        Self::with_hasher(Hasher::default())
    }
    type Key = lasso::Spur;
    fn intern(&mut self, value: &str) -> Self::Key {
        self.get_or_intern(value)
    }
    fn resolve<'a>(&'a self, key: &'a Self::Key) -> &'a str {
        self.resolve(key)
    }
}
type Interner = string_interner::StringInterner<string_interner::DefaultBackend, Hasher>;
impl Pool for Interner {
    fn new() -> Self {
        Self::with_hasher(Hasher::default())
    }
    type Key = string_interner::DefaultSymbol;
    fn intern(&mut self, value: &str) -> Self::Key {
        self.get_or_intern(value)
    }
    fn resolve<'a>(&'a self, key: &'a Self::Key) -> &'a str {
        self.resolve(*key).unwrap()
    }
}
struct Row<K> {
    timestamp: u64,
    start: u64,
    end: u64,
    level: u8,
    target: K,
}
fn run<P: Pool>(names: &[String], count: usize) -> [f64; 4] {
    #[cfg(feature = "alloc-stats")]
    let before = {
        allocation::reset_peak();
        allocation::snapshot()
    };
    let start = Instant::now();
    let mut pool = P::new();
    let mut rows = Vec::new();
    for i in 0..count {
        rows.push(Row {
            timestamp: (i / 3) as u64,
            start: i as u64 * 160,
            end: (i as u64 + 1) * 160,
            level: (i % 5) as u8,
            target: pool.intern(&names[i % names.len()]),
        });
    }
    let build = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    for i in count..count + 1000 {
        rows.push(Row {
            timestamp: (i / 3) as u64,
            start: i as u64 * 160,
            end: (i as u64 + 1) * 160,
            level: (i % 5) as u8,
            target: pool.intern(&names[i % names.len()]),
        });
    }
    let append = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    let actual: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.level == 0 && pool.resolve(&r.target) == names[0])
        .take(100)
        .map(|(i, r)| {
            black_box((r.timestamp, r.start, r.end));
            i
        })
        .collect();
    let query = start.elapsed().as_secs_f64() * 1000.;
    #[cfg(feature = "alloc-stats")]
    let retained = allocation::snapshot();
    let expected: Vec<_> = (0..count + 1000)
        .filter(|i| i % 5 == 0 && i % names.len() == 0)
        .take(100)
        .collect();
    assert_eq!(actual, expected);
    drop(actual);
    drop(expected);
    black_box((&rows, &pool));
    let start = Instant::now();
    drop(rows);
    drop(pool);
    let teardown = start.elapsed().as_secs_f64() * 1000.;
    #[cfg(feature = "alloc-stats")]
    {
        let after = allocation::snapshot();
        assert_eq!(before.2, after.2, "pool/entries/query leaked allocations");
        println!(
            "{}",
            json!({"mode":"allocations","pool":std::any::type_name::<P>(),"rows":count,
            "targets":names.len(),"length":names[0].len(),"allocations":retained.0-before.0,
            "live_bytes":retained.2-before.2,"peak_bytes":retained.3-before.2,"unreleased_bytes":0})
        );
    }
    [build, append, query, teardown]
}
fn main() {
    fn send<T: Send + 'static>() {}
    send::<lasso::Rodeo<lasso::Spur, Hasher>>();
    send::<Interner>();
    send::<Standard>();
    let smoke = std::env::args().any(|v| v == "smoke");
    let cases = if smoke {
        vec![(10_000, 32, 24)]
    } else {
        vec![
            (100_000, 32, 16),
            (100_000, 32, 23),
            (100_000, 32, 24),
            (100_000, 32, 64),
            (100_000, 50_000, 128),
            (1_000_000, 128, 64),
        ]
    };
    let variants = [
        "String",
        "SmolStr",
        "std_arc_ids",
        "lasso",
        "string_interner",
    ];
    for (count, cardinality, len) in cases {
        let names: Vec<_> = (0..cardinality)
            .map(|i| format!("{i:08}{}", "x".repeat(len - 8)))
            .collect();
        let mut samples = vec![Vec::new(); variants.len()];
        for round in 0..10 {
            for offset in 0..variants.len() {
                let kind = (offset + round) % variants.len();
                let result = match kind {
                    0 => run::<Owned>(&names, count),
                    1 => run::<Small>(&names, count),
                    2 => run::<Standard>(&names, count),
                    3 => run::<lasso::Rodeo<lasso::Spur, Hasher>>(&names, count),
                    _ => run::<Interner>(&names, count),
                };
                if round > 0 {
                    samples[kind].push(result);
                }
            }
        }
        #[cfg(not(feature = "alloc-stats"))]
        for (kind, samples) in samples.iter().enumerate() {
            let medians: Vec<_> = (0..4)
                .map(|metric| {
                    let mut values: Vec<_> = samples.iter().map(|s| s[metric]).collect();
                    values.sort_by(f64::total_cmp);
                    values[values.len() / 2]
                })
                .collect();
            println!(
                "{}",
                json!({"variant":variants[kind],"rows":count,"targets":cardinality,"length":len,
                "build_append_query_drop_ms":medians,"samples_ms":samples})
            );
        }
    }
}

#[cfg(feature = "alloc-stats")]
mod allocation {
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        sync::atomic::{AtomicUsize, Ordering::Relaxed},
    };
    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static REALLOCS: AtomicUsize = AtomicUsize::new(0);
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);
    pub struct Counting;
    fn add(n: usize) {
        let live = LIVE.fetch_add(n, Relaxed) + n;
        PEAK.fetch_max(live, Relaxed);
    }
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            let p = unsafe { System.alloc(l) };
            if !p.is_null() {
                ALLOCS.fetch_add(1, Relaxed);
                add(l.size());
            }
            p
        }
        unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
            let p = unsafe { System.alloc_zeroed(l) };
            if !p.is_null() {
                ALLOCS.fetch_add(1, Relaxed);
                add(l.size());
            }
            p
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            LIVE.fetch_sub(l.size(), Relaxed);
            unsafe { System.dealloc(p, l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            let q = unsafe { System.realloc(p, l, n) };
            if !q.is_null() {
                REALLOCS.fetch_add(1, Relaxed);
                if n >= l.size() {
                    add(n - l.size());
                } else {
                    LIVE.fetch_sub(l.size() - n, Relaxed);
                }
            }
            q
        }
    }
    pub fn snapshot() -> (usize, usize, usize, usize) {
        (
            ALLOCS.load(Relaxed),
            REALLOCS.load(Relaxed),
            LIVE.load(Relaxed),
            PEAK.load(Relaxed),
        )
    }
    pub fn reset_peak() {
        PEAK.store(LIVE.load(Relaxed), Relaxed);
    }
}
#[cfg(feature = "alloc-stats")]
#[global_allocator]
static ALLOCATOR: allocation::Counting = allocation::Counting;
