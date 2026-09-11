from pathlib import Path

HERE = Path(__file__).parent
reference = HERE / 'reference-indexer.rs'
source = reference.read_text(encoding='utf-8')
types = source[source.index('#[derive('):source.index('struct LogIndex {')]
types = types.replace(', Type', '')
core = source[source.index('struct LogIndex {'):source.index('pub struct Indexer {')]
debug_start = core.index('impl core::fmt::Debug for LogIndex')
debug_end = core.index('impl LogIndex {')
core = core[:debug_start] + core[debug_end:]
core = core.replace('struct LogIndex {', 'pub struct LogIndex {', 1)
core = core.replace('        #[cfg(test)]\n        dbg!(&matching_lines);\n', '')

# This baseline changes only teardown. The workload always appends unique line IDs.
fixed = core + '''
impl Drop for LogIndex {
    fn drop(&mut self) {
        // The three maps uniquely own disjoint arena allocations. Values must
        // be dropped before arena storage; the workload never overwrites IDs.
        unsafe {
            for entry in self.line_index.values() { std::ptr::drop_in_place(*entry); }
            for list in self.level_index.values() { std::ptr::drop_in_place(*list); }
            for list in self.target_index.values() { std::ptr::drop_in_place(*list); }
        }
    }
}
'''

owned = core.replace('    arena: Bump,', '').replace('            arena: Bump::new(),', '')
owned = owned.replace('*mut LogEntry', 'LogEntry').replace('*mut Vec<LineNumber>', 'Vec<LineNumber>')
owned = owned.replace('let entry_ptr = self.arena.alloc(entry) as LogEntry;', 'let entry_ptr = entry;')
owned = owned.replace('let v = &mut **v;', 'let v = v;')
owned = owned.replace('let lines = &**lines;', 'let lines = lines;')
owned = owned.replace('let entry = &**self.line_index.get(&line_number).unwrap();', 'let entry = self.line_index.get(&line_number).unwrap();')
owned = owned.replace('let vec = self.arena.alloc(vec![line_number]);\n                    vec as *mut Vec<u64>', 'vec![line_number]')
assert 'arena.alloc' not in owned and '*mut' not in owned and '&**' not in owned

arena = owned.replace('pub struct LogIndex {', "pub struct IndexView<'a> {")
arena = arena.replace('impl LogIndex {', "impl<'a> IndexView<'a> {")
arena = arena.replace('BTreeMap<LineNumber, LogEntry>', "BTreeMap<LineNumber, &'a ArenaEntry<'a>>")
arena = arena.replace('FxHashMap<String, Vec<LineNumber>>', "FxHashMap<&'a str, Vec<LineNumber>>")
start = arena.index('    pub fn add_entry(')
end = arena.index('        // update level index', start)
arena = arena[:start] + '''
    pub fn add_borrowed(&mut self, arena: &'a Bump, input: InputRow<'_>) {
        let line_number = input.line;
        let timestamp = input.timestamp;
        let level = input.level;
        let target = match self.target_index.get_key_value(input.target) {
            Some((target, _)) => *target,
            None => &*arena.alloc_str(input.target),
        };
        let entry_ptr = &*arena.alloc(ArenaEntry { input: input.numbers(), target });
''' + arena[end:]
arena = arena.replace('entry.clone()', 'entry.owned()')
arena = arena.replace('self.target_index.get(&target)', 'self.target_index.get(target.as_str())')
arena += '''
struct ArenaEntry<'a> { input: Numbers, target: &'a str }
impl ArenaEntry<'_> {
    fn owned(&self) -> LogEntry { self.input.owned(self.target) }
}
self_cell::self_cell!(
    pub struct LogIndex {
        owner: Bump,
        #[covariant]
        dependent: IndexView,
    }
);
impl LogIndex {
    pub fn create() -> Self { Self::new(Bump::new(), |_| IndexView::new()) }
    pub fn add_borrowed(&mut self, row: InputRow<'_>) {
        self.with_dependent_mut(|arena, index| index.add_borrowed(arena, row));
    }
    pub fn query(&self, query: Query) -> Option<Vec<LogEntry>> {
        self.borrow_dependent().query(query)
    }
}
'''

intern = owned.replace('BTreeMap<LineNumber, LogEntry>', 'BTreeMap<LineNumber, ArcEntry>')
intern = intern.replace('FxHashMap<String, Vec<LineNumber>>', 'FxHashMap<Arc<str>, Vec<LineNumber>>')
start = intern.index('    pub fn add_entry(')
end = intern.index('        // update level index', start)
intern = intern[:start] + '''
    pub fn add_borrowed(&mut self, input: InputRow<'_>) {
        let line_number = input.line;
        let timestamp = input.timestamp;
        let level = input.level;
        let target = match self.target_index.get_key_value(input.target) {
            Some((target, _)) => Arc::clone(target),
            None => Arc::<str>::from(input.target),
        };
        let entry_ptr = ArcEntry { input: input.numbers(), target: target.clone() };
''' + intern[end:]
intern = intern.replace('entry.clone()', 'entry.owned()')
intern = intern.replace('self.target_index.get(&target)', 'self.target_index.get(target.as_str())')
intern += '''
struct ArcEntry { input: Numbers, target: Arc<str> }
impl ArcEntry {
    fn owned(&self) -> LogEntry { self.input.owned(&self.target) }
}
'''

# An explicit optimized standard-library alternative. All record fields remain
# stored; only the monotonically appended line lookup changes from tree to Vec.
dense = owned.replace('BTreeMap<LineNumber, LogEntry>', 'Vec<IdEntry>')
dense = dense.replace('line_index: BTreeMap::new()', 'line_index: Vec::new()')
dense = dense.replace('FxHashMap<String, Vec<LineNumber>>', 'FxHashMap<Arc<str>, TargetPosting>')
dense = dense.replace('    last_line_number:', '    target_names: Vec<Arc<str>>,\n    last_line_number:', 1)
dense = dense.replace('            last_line_number: None,', '            target_names: Vec::new(),\n            last_line_number: None,')
start = dense.index('    pub fn add_entry(')
end = dense.index('        // update level index', start)
dense = dense[:start] + '''
    pub fn add_borrowed(&mut self, input: InputRow<'_>) {
        let line_number = input.line;
        let timestamp = input.timestamp;
        let level = input.level;
        let target_id = if let Some(posting) = self.target_index.get_mut(input.target) {
            posting.lines.push(line_number);
            posting.id
        } else {
            let target: Arc<str> = Arc::from(input.target);
            let id = self.target_names.len();
            self.target_names.push(target.clone());
            self.target_index.insert(target, TargetPosting { id, lines: vec![line_number] });
            id
        };
        let entry_ptr = IdEntry { input: input.numbers(), target_id };
''' + dense[end:]
start = dense.index('        // update target index')
end = dense.index('        // update line index', start)
dense = dense[:start] + dense[end:]
dense = dense.replace('self.line_index.insert(line_number, entry_ptr);', 'assert_eq!(self.line_index.len(), line_number as usize);\n            self.line_index.push(entry_ptr);')
dense = dense.replace('self.target_index.get(&target)', 'self.target_index.get(target.as_str())')
start = dense.index('        // query by target')
dense = dense[:start] + dense[start:].replace('let lines = lines;', 'let lines = &lines.lines;', 1)
dense = dense.replace('self.line_index.get(&line_number)', 'self.line_index.get(line_number as usize)')
dense = dense.replace('entry.clone()', 'entry.input.owned(&self.target_names[entry.target_id])')
dense += '''
struct IdEntry { input: Numbers, target_id: usize }
struct TargetPosting { id: usize, lines: Vec<u64> }
'''

header = '''
#![allow(dead_code, unused_imports, unused_unsafe, clippy::all)]
use std::{collections::{BTreeMap, BTreeSet}, ops::Range, sync::Arc, hint::black_box, time::Instant};
use bumpalo::Bump;
use derive_builder::Builder;
use itertools::Itertools;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
'''

common = r'''
#[derive(Clone, Copy)]
struct InputRow<'a> { line: u64, timestamp: u64, level: LoggingLevel, target: &'a str }
#[derive(Clone, Copy)]
struct Numbers { line: u64, timestamp: u64, level: LoggingLevel, start_pos: usize, end_pos: usize }
impl InputRow<'_> {
    fn numbers(self) -> Numbers { Numbers { line: self.line, timestamp: self.timestamp, level: self.level,
        start_pos: self.line as usize*160, end_pos: (self.line as usize+1)*160 } }
    fn owned(self) -> LogEntry { self.numbers().owned(self.target) }
}
impl Numbers {
    fn owned(self, target: &str) -> LogEntry {
        LogEntry { line_number: self.line, timestamp: self.timestamp, level: self.level,
            target: target.to_string(), start_pos: self.start_pos, end_pos: self.end_pos }
    }
}

trait BenchIndex: Sized {
    fn create() -> Self;
    fn add(&mut self, row: InputRow<'_>);
    fn query(&self, query: Query) -> Option<Vec<LogEntry>>;
}
macro_rules! owned_impl {
    ($module:ident) => { impl BenchIndex for $module::LogIndex {
        fn create() -> Self { Self::new() }
        fn add(&mut self, row: InputRow<'_>) { self.add_entry(row.owned()); }
        fn query(&self, query: Query) -> Option<Vec<LogEntry>> { self.query(query) }
    } };
}
macro_rules! borrowed_impl {
    ($module:ident, $constructor:ident) => { impl BenchIndex for $module::LogIndex {
        fn create() -> Self { Self::$constructor() }
        fn add(&mut self, row: InputRow<'_>) { self.add_borrowed(row); }
        fn query(&self, query: Query) -> Option<Vec<LogEntry>> { self.query(query) }
    } };
}
owned_impl!(bump_fixed);
owned_impl!(std_owned_fx);
owned_impl!(std_owned_default);
borrowed_impl!(bump_intern, create);
borrowed_impl!(std_intern_fx, new);
borrowed_impl!(std_intern_default, new);
borrowed_impl!(std_vec_id_default, new);

struct Dataset { n: usize, targets: Vec<String> }
impl Dataset {
    fn new(n: usize, cardinality: usize, length: usize) -> Self {
        let targets = (0..cardinality).map(|i| {
            let prefix = format!("clash_nyanpasu::core::module_{i:06}::");
            format!("{prefix}{}", "x".repeat(length.saturating_sub(prefix.len())))
        }).collect();
        Self { n, targets }
    }
    fn row(&self, i: usize) -> InputRow<'_> {
        let level = match i % 100 { 0 => LoggingLevel::FATAL, 1..=3 => LoggingLevel::ERROR,
            4..=14 => LoggingLevel::WARN, 15..=64 => LoggingLevel::INFO, _ => LoggingLevel::DEBUG };
        InputRow { line: i as u64, timestamp: 1_780_000_000_000 + i as u64, level,
            target: &self.targets[i.wrapping_mul(2654435761) % self.targets.len()] }
    }
    fn queries(&self) -> Vec<(&'static str, Query)> {
        let n = self.n + 1000;
        vec![
            ("latest_100", QueryBuilder::default().offset(n-100).build().unwrap()),
            ("level_warn_error", QueryBuilder::default().level(vec![LoggingLevel::WARN, LoggingLevel::ERROR]).build().unwrap()),
            ("target", QueryBuilder::default().target(vec![self.row(n/2).target.to_string()]).build().unwrap()),
            ("time_1000", QueryBuilder::default().timestamp((1_780_000_000_000+n as u64-2000)..(1_780_000_000_000+n as u64-1000)).build().unwrap()),
            ("combined", QueryBuilder::default().level(vec![LoggingLevel::INFO,LoggingLevel::WARN]).target(vec![self.row(n-1500).target.to_string()]).timestamp((1_780_000_000_000+n as u64-10000)..(1_780_000_000_000+n as u64)).build().unwrap()),
        ]
    }
}

fn build<I: BenchIndex>(data: &Dataset, len: usize) -> I {
    let mut index = I::create();
    for i in 0..len { index.add(data.row(i)); }
    black_box(index)
}
fn signature(rows: &Option<Vec<LogEntry>>) -> u64 {
    let mut h = 0_u64;
    for row in rows.iter().flatten() {
        h = h.wrapping_mul(31).wrapping_add(row.line_number).wrapping_add(row.timestamp);
        h = h.wrapping_mul(31).wrapping_add(row.start_pos as u64).wrapping_add(row.end_pos as u64);
        for byte in row.target.bytes() { h = h.wrapping_mul(31).wrapping_add(byte as u64); }
        h = h.wrapping_mul(31).wrapping_add(row.level as u64);
    }
    h
}

#[derive(Default)]
struct Samples { build: Vec<f64>, append: Vec<f64>, drop: Vec<f64>, query: Vec<Vec<f64>> }
fn round<I: BenchIndex>(data: &Dataset, queries: &[(&str, Query)], expected: &[u64], samples: &mut Samples) {
    let start = Instant::now();
    let mut index = build::<I>(data, data.n);
    samples.build.push(start.elapsed().as_secs_f64()*1000.0);
    let start = Instant::now();
    for i in data.n..data.n+1000 { index.add(data.row(i)); }
    samples.append.push(start.elapsed().as_secs_f64()*1000.0);
    for (j, (_, query)) in queries.iter().enumerate() {
        assert_eq!(signature(&index.query(query.clone())), expected[j]);
        let start = Instant::now();
        drop(black_box(index.query(black_box(query.clone()))));
        let single = start.elapsed().as_secs_f64();
        let repeats = (0.002 / single.max(1e-9)).ceil().clamp(1.0, 100.0) as u32;
        let start = Instant::now();
        for _ in 0..repeats { drop(black_box(index.query(black_box(query.clone())))); }
        samples.query[j].push(start.elapsed().as_secs_f64()*1e6 / repeats as f64);
    }
    let start = Instant::now();
    drop(index);
    samples.drop.push(start.elapsed().as_secs_f64()*1000.0);
}
fn dispatch_round(kind: usize, data: &Dataset, queries: &[(&str, Query)], expected: &[u64], samples: &mut Samples) {
    match kind {
        0 => round::<bump_fixed::LogIndex>(data,queries,expected,samples),
        1 => round::<std_owned_fx::LogIndex>(data,queries,expected,samples),
        2 => round::<std_owned_default::LogIndex>(data,queries,expected,samples),
        3 => round::<bump_intern::LogIndex>(data,queries,expected,samples),
        4 => round::<std_intern_fx::LogIndex>(data,queries,expected,samples),
        5 => round::<std_intern_default::LogIndex>(data,queries,expected,samples),
        6 => round::<std_vec_id_default::LogIndex>(data,queries,expected,samples),
        _ => unreachable!(),
    }
}
fn stats(xs: &[f64]) -> serde_json::Value {
    let mut xs = xs.to_vec(); xs.sort_by(f64::total_cmp);
    serde_json::json!({"median":xs[xs.len()/2], "min":xs[0], "max":xs[xs.len()-1], "p90":xs[(xs.len()-1)*9/10]})
}
const NAMES: [&str;7] = ["bump_owned_fixed", "std_owned_fx", "std_owned_default", "bump_intern_self_cell", "std_arc_intern_fx", "std_arc_intern_default", "std_vec_target_id_default"];
fn timings(data: &Dataset) {
    let queries = data.queries();
    let reference = build::<bump_fixed::LogIndex>(data, data.n+1000);
    let expected: Vec<_> = queries.iter().map(|(_,q)| signature(&reference.query(q.clone()))).collect();
    drop(reference);
    let mut samples: Vec<_> = (0..NAMES.len()).map(|_| Samples { query: (0..queries.len()).map(|_|Vec::new()).collect(), ..Samples::default() }).collect();
    for round in 0..10 {
        for j in 0..NAMES.len() {
            let kind = (j+round)%NAMES.len();
            if round == 0 {
                let mut warm = Samples { query: (0..queries.len()).map(|_|Vec::new()).collect(), ..Samples::default() };
                dispatch_round(kind,data,&queries,&expected,&mut warm);
            } else { dispatch_round(kind,data,&queries,&expected,&mut samples[kind]); }
        }
        eprintln!("rows={} targets={} round={round}/9",data.n,data.targets.len());
    }
    for (kind,s) in samples.iter().enumerate() {
        let query: serde_json::Map<String,serde_json::Value> = queries.iter().enumerate().map(|(j,(name,_))| (name.to_string(),stats(&s.query[j]))).collect();
        println!("{}",serde_json::json!({"mode":"timing","rows":data.n,"targets":data.targets.len(),"target_len":data.targets[0].len(),"variant":NAMES[kind],"build_ms":stats(&s.build),"append_1000_ms":stats(&s.append),"drop_ms":stats(&s.drop),"queries_us":query}));
    }
}

#[cfg(feature="alloc-stats")]
mod allocation {
    use std::{alloc::{GlobalAlloc,Layout,System}, sync::atomic::{AtomicUsize,Ordering::Relaxed}};
    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static REALLOCS: AtomicUsize = AtomicUsize::new(0);
    static LIVE: AtomicUsize = AtomicUsize::new(0);
    static PEAK: AtomicUsize = AtomicUsize::new(0);
    pub struct Counting;
    fn add(n:usize) { let live=LIVE.fetch_add(n,Relaxed)+n; PEAK.fetch_max(live,Relaxed); }
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self,l:Layout)->*mut u8 { let p=unsafe{System.alloc(l)}; if !p.is_null(){ALLOCS.fetch_add(1,Relaxed);add(l.size());}p }
        unsafe fn alloc_zeroed(&self,l:Layout)->*mut u8 { let p=unsafe{System.alloc_zeroed(l)}; if !p.is_null(){ALLOCS.fetch_add(1,Relaxed);add(l.size());}p }
        unsafe fn dealloc(&self,p:*mut u8,l:Layout) { LIVE.fetch_sub(l.size(),Relaxed);unsafe{System.dealloc(p,l)} }
        unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8 { let q=unsafe{System.realloc(p,l,n)}; if !q.is_null(){REALLOCS.fetch_add(1,Relaxed);if n>=l.size(){add(n-l.size());}else{LIVE.fetch_sub(l.size()-n,Relaxed);}}q }
    }
    pub fn snapshot()->(usize,usize,usize,usize){(ALLOCS.load(Relaxed),REALLOCS.load(Relaxed),LIVE.load(Relaxed),PEAK.load(Relaxed))}
    pub fn reset_peak(){PEAK.store(LIVE.load(Relaxed),Relaxed);}
}
#[cfg(feature="alloc-stats")]
#[global_allocator]
static ALLOCATOR: allocation::Counting = allocation::Counting;

#[cfg(feature="alloc-stats")]
fn memory<I: BenchIndex>(kind: usize, data:&Dataset) {
    let query=data.queries().remove(0).1;
    allocation::reset_peak();
    let before=allocation::snapshot();
    let mut index=build::<I>(data,data.n);
    let built=allocation::snapshot();
    for i in data.n..data.n+1000 {index.add(data.row(i));}
    let appended=allocation::snapshot();
    let result=index.query(query);
    let queried=allocation::snapshot();
    black_box(&result);
    drop(result);
    drop(index);
    let after=allocation::snapshot();
    assert_eq!(after.2,before.2,"index/result teardown retained allocations: {}",NAMES[kind]);
    println!("{}",serde_json::json!({"mode":"allocations","rows":data.n,"targets":data.targets.len(),"target_len":data.targets[0].len(),"variant":NAMES[kind],"build_allocs":built.0-before.0,"build_reallocs":built.1-before.1,"build_live_bytes":built.2-before.2,"build_peak_bytes":built.3-before.2,"append_allocs":appended.0-built.0,"append_reallocs":appended.1-built.1,"latest_query_allocs":queried.0-appended.0,"latest_query_reallocs":queried.1-appended.1,"unreleased_bytes":after.2-before.2}));
}
#[cfg(feature="alloc-stats")]
fn memory_all(data:&Dataset) {
    memory::<bump_fixed::LogIndex>(0,data);
    memory::<std_owned_fx::LogIndex>(1,data);
    memory::<std_owned_default::LogIndex>(2,data);
    memory::<bump_intern::LogIndex>(3,data);
    memory::<std_intern_fx::LogIndex>(4,data);
    memory::<std_intern_default::LogIndex>(5,data);
    memory::<std_vec_id_default::LogIndex>(6,data);
}
#[cfg(feature="alloc-stats")]
fn repeated_teardown<I: BenchIndex>(name: &str) {
    let data = Dataset::new(1000,32,64);
    for _ in 0..100 {
        let before=allocation::snapshot().2;
        let mut index=build::<I>(&data,data.n);
        for i in data.n..data.n+1000 {index.add(data.row(i));}
        drop(black_box(index.query(QueryBuilder::default().offset(1900).build().unwrap())));
        drop(index);
        assert_eq!(allocation::snapshot().2,before,"repeated teardown: {name}");
    }
    eprintln!("repeated_teardown variant={name} cycles=100 unreleased_bytes=0");
}
#[cfg(windows)]
fn pin_thread() {
    #[link(name="kernel32")]
    unsafe extern "system" { fn GetCurrentThread()->*mut std::ffi::c_void; fn SetThreadAffinityMask(h:*mut std::ffi::c_void,mask:usize)->usize; }
    assert_ne!(unsafe{SetThreadAffinityMask(GetCurrentThread(),1)},0);
}
#[cfg(not(windows))]
fn pin_thread() {}

fn main() {
    // Both implementations can be owned by a Send + 'static actor state.
    fn assert_send<T: Send + 'static>() {}
    assert_send::<bump_intern::LogIndex>();
    assert_send::<std_owned_default::LogIndex>();
    assert_send::<std_intern_default::LogIndex>();
    assert_send::<std_vec_id_default::LogIndex>();
    pin_thread();
    eprintln!("allocation_counter={} affinity=logical_cpu_0",cfg!(feature="alloc-stats"));
    let mode=std::env::args().nth(1).unwrap_or_else(||"timing".into());
    let cases = if mode=="smoke" {vec![(10000,32,64)]} else {vec![(100000,32,64),(100000,50000,128),(1000000,128,64)]};
    for (n,targets,length) in cases {
        let data=Dataset::new(n,targets,length);
        #[cfg(feature="alloc-stats")]
        memory_all(&data);
        #[cfg(not(feature="alloc-stats"))]
        timings(&data);
    }
    #[cfg(feature="alloc-stats")]
    {
        repeated_teardown::<bump_fixed::LogIndex>(NAMES[0]);
        repeated_teardown::<std_owned_fx::LogIndex>(NAMES[1]);
        repeated_teardown::<std_owned_default::LogIndex>(NAMES[2]);
        repeated_teardown::<bump_intern::LogIndex>(NAMES[3]);
        repeated_teardown::<std_intern_fx::LogIndex>(NAMES[4]);
        repeated_teardown::<std_intern_default::LogIndex>(NAMES[5]);
        repeated_teardown::<std_vec_id_default::LogIndex>(NAMES[6]);
    }
}
'''

parts=[header,types,common]
for name,body in [('bump_fixed',fixed),('std_owned_fx',owned),('std_owned_default',owned),('bump_intern',arena),('std_intern_fx',intern),('std_intern_default',intern),('std_vec_id_default',dense)]:
    # Keep the hasher identical for the controlled comparison. The additional
    # default-hasher variants are the literal standard-library-only designs.
    extra='use std::collections::HashMap as FxHashMap;' if name.endswith('_default') else ''
    parts.append(f'mod {name} {{\nuse super::*;\n{extra}\n{body}\n}}\n')
(HERE/'src/bin/allocation_bench.rs').write_text('\n'.join(parts))
print('Generated allocation_bench.rs from', reference)
