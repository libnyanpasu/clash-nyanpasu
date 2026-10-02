//! Load time, memory and lookup throughput over real database files.
//!
//! ```text
//! cargo run --release -p nyanpasu-geodata --example bench -- <kind> <file> [flags]
//! cargo run --release -p nyanpasu-geodata --example bench -- dir <dir> [flags]
//! ```
//!
//! `kind` is `ip`, `asn`, `geoip-dat` or `geosite`; `dir` benchmarks every
//! database file in a directory (a core home works), each in its own process
//! because process memory is measured for the whole process. Flags:
//! - `--heap-read` reads the file with `std::fs::read` instead of `read_source`;
//! - `--reopen` then stores the index in a temporary file and benchmarks it
//!   again in a fresh process that maps the file, as a cache would load it;
//! - `--image` takes `file` to be such a stored index.
//!
//! Process memory is the private / committed figure and the resident set:
//! `phys_footprint` / RSS on macOS, `RssAnon` / `VmRSS` on Linux. Windows adds
//! two columns: anonymous memory mapped through memmap2 (`read_source`, build
//! buffers) is a pagefile-backed section, which Windows counts as shared
//! commit and shared working set rather than private bytes, so the figures are
//! private bytes / shared commit / working set / private working set. Every
//! figure is the growth over a baseline taken before the file is read.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::Path,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
    thread,
    time::Instant,
};

use maxminddb::{PathElement, Reader};
use nyanpasu_geodata::{AsnIndex, IpIndex, SiteIndex, read_source};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
/// Off while measuring throughput: shared counters would serialize threads
/// that allocate.
static COUNTING: AtomicBool = AtomicBool::new(true);

fn grow(size: usize) {
    if COUNTING.load(Relaxed) {
        let live = LIVE.fetch_add(size, Relaxed) + size;
        PEAK.fetch_max(live, Relaxed);
    }
}

fn shrink(size: usize) {
    if COUNTING.load(Relaxed) {
        LIVE.fetch_sub(size, Relaxed);
    }
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            grow(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            grow(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        shrink(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, size) };
        if !new.is_null() {
            grow(size);
            shrink(layout.size());
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[cfg(target_os = "macos")]
const MEMORY_LABELS: &[&str] = &["footprint", "rss"];
#[cfg(target_os = "windows")]
const MEMORY_LABELS: &[&str] = &[
    "private bytes",
    "shared commit",
    "working set",
    "private WS",
];
#[cfg(target_os = "linux")]
const MEMORY_LABELS: &[&str] = &["anon", "rss"];
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
const MEMORY_LABELS: &[&str] = &["private", "resident"];

/// Bytes of this process, one per `MEMORY_LABELS` entry.
type Memory = [u64; MEMORY_LABELS.len()];

#[cfg(target_os = "macos")]
fn process_memory() -> Memory {
    // <libproc.h> `rusage_info_v2`; only the leading fields are named.
    #[repr(C)]
    struct RusageInfoV2 {
        uuid: [u8; 16],
        times_and_wakeups: [u64; 4],
        pageins: u64,
        wired_size: u64,
        resident_size: u64,
        phys_footprint: u64,
        rest: [u64; 32],
    }
    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV2) -> i32;
    }
    let mut info = RusageInfoV2 {
        uuid: [0; 16],
        times_and_wakeups: [0; 4],
        pageins: 0,
        wired_size: 0,
        resident_size: 0,
        phys_footprint: 0,
        rest: [0; 32],
    };
    const RUSAGE_INFO_V2: i32 = 2;
    unsafe { proc_pid_rusage(std::process::id() as i32, RUSAGE_INFO_V2, &mut info) };
    [info.phys_footprint, info.resident_size]
}

#[cfg(target_os = "windows")]
fn process_memory() -> Memory {
    /// `PROCESS_MEMORY_COUNTERS_EX2`.
    #[repr(C)]
    struct ProcessMemoryCountersEx2 {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
        private_working_set_size: usize,
        shared_commit_usage: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut ProcessMemoryCountersEx2,
            cb: u32,
        ) -> i32;
    }
    let mut counters = ProcessMemoryCountersEx2 {
        cb: size_of::<ProcessMemoryCountersEx2>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
        private_usage: 0,
        private_working_set_size: 0,
        shared_commit_usage: 0,
    };
    let cb = counters.cb;
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, cb) };
    [
        counters.private_usage as u64,
        counters.shared_commit_usage,
        counters.working_set_size as u64,
        counters.private_working_set_size as u64,
    ]
}

#[cfg(target_os = "linux")]
fn process_memory() -> Memory {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
            .map_or(0, |kib| kib * 1024)
    };
    [field("RssAnon:"), field("VmRSS:")]
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn process_memory() -> Memory {
    [0; MEMORY_LABELS.len()]
}

fn mib(bytes: f64) -> f64 {
    bytes / (1 << 20) as f64
}

struct Baseline {
    process: Memory,
    live: usize,
}

impl Baseline {
    fn take() -> Self {
        Self {
            process: process_memory(),
            live: LIVE.load(Relaxed),
        }
    }

    fn stage(&self, name: &str) {
        // Read before formatting, which allocates.
        let (live, memory) = (LIVE.load(Relaxed), process_memory());
        let figures: Vec<String> = memory
            .iter()
            .zip(self.process)
            .map(|(now, base)| format!("{:>8.2}", mib(*now as f64 - base as f64)))
            .collect();
        println!(
            "    {name:<24} {} MiB   (live heap {:.2} MiB)",
            figures.join(" / "),
            mib(live as f64 - self.live as f64),
        );
    }
}

enum Index {
    Ip(IpIndex),
    Asn(AsnIndex),
    Site(SiteIndex),
}

impl Index {
    /// Opens a stored index by mapping it read-only.
    fn open(kind: &str, path: &Path) -> Self {
        let file = std::fs::File::open(path).unwrap();
        // SAFETY: the bench's own temporary file, not written while mapped.
        let map = unsafe { memmap2::Mmap::map(&file) }.unwrap();
        match kind {
            "ip" | "geoip-dat" => Index::Ip(IpIndex::from_bytes(map).unwrap()),
            "asn" => Index::Asn(AsnIndex::from_bytes(map).unwrap()),
            "geosite" => Index::Site(SiteIndex::from_bytes(map).unwrap()),
            other => panic!("unknown kind {other}"),
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Index::Ip(index) => index.as_bytes(),
            Index::Asn(index) => index.as_bytes(),
            Index::Site(index) => index.as_bytes(),
        }
    }

    fn hit(&self, ip: IpAddr, host: &str) -> bool {
        match self {
            Index::Ip(index) => index.lookup(ip).is_some(),
            Index::Asn(index) => index.lookup(ip).is_some(),
            Index::Site(index) => !index.lookup(host).is_empty(),
        }
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Three IPv4 addresses (anywhere) per IPv6 one (in 2000::/3).
fn addresses(count: usize) -> Vec<IpAddr> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    (0..count)
        .map(|i| {
            if i % 4 == 3 {
                let bits = (u128::from(rng.next()) << 64 | u128::from(rng.next()))
                    & !(0b111 << 125)
                    | 1 << 125;
                IpAddr::V6(Ipv6Addr::from(bits))
            } else {
                IpAddr::V4(Ipv4Addr::from(rng.next() as u32))
            }
        })
        .collect()
}

const POPULAR: &[&str] = &[
    "google.com",
    "youtube.com",
    "googlevideo.com",
    "github.com",
    "githubusercontent.com",
    "baidu.com",
    "qq.com",
    "bilibili.com",
    "taobao.com",
    "jd.com",
    "apple.com",
    "icloud.com",
    "microsoft.com",
    "live.com",
    "office.com",
    "amazon.com",
    "cloudfront.net",
    "akamaized.net",
    "netflix.com",
    "nflxvideo.net",
    "twitter.com",
    "x.com",
    "twimg.com",
    "facebook.com",
    "instagram.com",
    "whatsapp.net",
    "telegram.org",
    "discord.com",
    "spotify.com",
    "steampowered.com",
    "doubleclick.net",
    "googlesyndication.com",
    "wikipedia.org",
    "reddit.com",
    "zhihu.com",
    "douyin.com",
    "weibo.com",
    "163.com",
    "aliyun.com",
    "cloudflare.com",
    "jsdelivr.net",
    "openai.com",
    "anthropic.com",
    "tiktok.com",
    "pixiv.net",
    "dmm.co.jp",
    "naver.com",
    "yahoo.co.jp",
];

/// Popular sites under random labels, and unknown domains.
fn hosts(count: usize) -> Vec<String> {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    (0..count)
        .map(|i| {
            let label = format!("{:x}", rng.next() % 0xfffff);
            if i % 2 == 0 {
                let site = POPULAR[(rng.next() % POPULAR.len() as u64) as usize];
                match rng.next() % 3 {
                    0 => site.to_owned(),
                    1 => format!("www.{site}"),
                    _ => format!("{label}.cdn.{site}"),
                }
            } else {
                let tld = ["com", "net", "org", "cn", "io"][(rng.next() % 5) as usize];
                format!("{label}.example{}.{tld}", rng.next() % 1000)
            }
        })
        .collect()
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Codes<'a> {
    One(&'a str),
    Many(Vec<&'a str>),
    Other(serde::de::IgnoredAny),
}

/// The core's own per-lookup decode over the whole file kept in memory.
fn raw_lookup_ns(bytes: &[u8], kind: &str, ips: &[IpAddr]) -> f64 {
    let reader = Reader::from_source(bytes).unwrap();
    let database_type = reader.metadata().database_type.clone();
    let start = Instant::now();
    let mut found = 0usize;
    for ip in ips {
        let result = reader.lookup(*ip).unwrap();
        found += match (kind, database_type.as_str()) {
            ("asn", _) => result
                .decode_path::<u32>(&[PathElement::Key("autonomous_system_number")])
                .ok()
                .flatten()
                .is_some() as usize,
            (_, "sing-geoip" | "Meta-geoip0") => match result.decode::<Codes>().ok().flatten() {
                Some(Codes::One(code)) => !code.is_empty() as usize,
                Some(Codes::Many(codes)) => !codes.is_empty() as usize,
                _ => 0,
            },
            _ => result
                .decode_path::<&str>(&[PathElement::Key("country"), PathElement::Key("iso_code")])
                .ok()
                .flatten()
                .is_some() as usize,
        };
    }
    black_box(found);
    start.elapsed().as_nanos() as f64 / ips.len() as f64
}

/// The kind of a database file in a core home, if it is one.
fn kind_of(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".dat") {
        return Some(if name.contains("site") {
            "geosite"
        } else {
            "geoip-dat"
        });
    }
    if [".mmdb", ".metadb", ".db"]
        .iter()
        .any(|ext| name.ends_with(ext))
    {
        let bytes = std::fs::read(path).ok()?;
        let reader = Reader::from_source(bytes.as_slice()).ok()?;
        let asn = reader
            .metadata()
            .database_type
            .to_ascii_lowercase()
            .contains("asn");
        return Some(if asn { "asn" } else { "ip" });
    }
    None
}

fn bench_dir(dir: &Path, flags: &[String]) {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    let exe = std::env::current_exe().unwrap();
    for path in files {
        let Some(kind) = kind_of(&path) else {
            continue;
        };
        let status = std::process::Command::new(&exe)
            .arg(kind)
            .arg(&path)
            .args(flags)
            .status()
            .unwrap();
        if !status.success() {
            println!("  ({} failed: {status})", path.display());
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags: Vec<String> = args
        .iter()
        .filter(|a| a.starts_with("--"))
        .cloned()
        .collect();
    let args: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let [kind, path] = args[..] else {
        eprintln!(
            "usage: bench <ip|asn|geoip-dat|geosite|dir> <path> [--heap-read] [--reopen] [--image]"
        );
        std::process::exit(2);
    };
    let (kind, path) = (kind.as_str(), Path::new(path));
    if kind == "dir" {
        return bench_dir(path, &flags);
    }
    let flag = |name: &str| flags.iter().any(|f| f == name);
    let (heap_read, image) = (flag("--heap-read"), flag("--image"));

    // The workload is allocated before the baseline so it stays out of it.
    let site = kind == "geosite";
    let queries = if site { 200_000 } else { 1_000_000 };
    let ips = addresses(if site { 1 } else { queries });
    let hosts = hosts(if site { queries } else { 1 });
    let probe = |i: usize| (ips[i % ips.len()], hosts[i % hosts.len()].as_str());

    let base = Baseline::take();
    println!(
        "{} ({kind}, {})",
        path.file_name().unwrap().to_string_lossy(),
        if image {
            "a stored index mapped from a file"
        } else if heap_read {
            "read into the heap"
        } else {
            "read into anonymous memory"
        }
    );
    println!(
        "  process memory over baseline, {}:",
        MEMORY_LABELS.join(" / ")
    );
    let mut file = std::fs::metadata(path).unwrap().len() as usize;
    let source: Option<Box<dyn std::ops::Deref<Target = [u8]>>> = if image {
        None
    } else if heap_read {
        Some(Box::new(std::fs::read(path).unwrap()))
    } else {
        Some(Box::new(read_source(path).unwrap()))
    };
    if let Some(source) = &source {
        file = source.len();
        base.stage("file read");
    }

    PEAK.store(LIVE.load(Relaxed), Relaxed);
    let before = LIVE.load(Relaxed);
    let start = Instant::now();
    let index = match (kind, &source) {
        (kind, None) => Index::open(kind, path),
        ("ip", Some(source)) => Index::Ip(IpIndex::from_mmdb(source).unwrap()),
        ("asn", Some(source)) => Index::Asn(AsnIndex::from_mmdb(source).unwrap()),
        ("geoip-dat", Some(source)) => Index::Ip(IpIndex::from_geoip_dat(source).unwrap()),
        ("geosite", Some(source)) => {
            Index::Site(SiteIndex::from_geosite_dat(source, |_| true).unwrap())
        }
        (other, _) => panic!("unknown kind {other}"),
    };
    let build = start.elapsed();
    let heap_peak = PEAK.load(Relaxed) - before;
    let index_heap = LIVE.load(Relaxed) - before;
    if image {
        base.stage("index opened (loaded)");
    } else {
        base.stage("index built");
        drop(source);
        base.stage("file dropped (loaded)");
    }

    let mut hits = 0usize;
    for i in 0..queries {
        let (ip, host) = probe(i);
        hits += index.hit(ip, host) as usize;
    }
    let start = Instant::now();
    for i in 0..queries * 2 {
        let (ip, host) = probe(i);
        black_box(index.hit(ip, host));
    }
    let single = start.elapsed().as_nanos() as f64 / (queries * 2) as f64;
    base.stage("after lookups");

    let cores = thread::available_parallelism().map_or(8, |n| n.get());
    // Per-thread caches (the regex pool) that concurrent lookups leave behind.
    thread::scope(|scope| {
        for t in 0..cores {
            let (index, probe) = (&index, &probe);
            scope.spawn(move || {
                for i in 0..10_000 {
                    let (ip, host) = probe(i + t * 7919);
                    black_box(index.hit(ip, host));
                }
            });
        }
    });
    base.stage(&format!("after {cores} threads"));

    COUNTING.store(false, Relaxed);
    let mut scaling = Vec::new();
    for threads in [1, 2, 4, 8, cores] {
        if threads > cores || scaling.iter().any(|(t, _)| *t == threads) {
            continue;
        }
        let per_thread = queries * 2;
        let start = Instant::now();
        thread::scope(|scope| {
            for t in 0..threads {
                let (index, probe) = (&index, &probe);
                scope.spawn(move || {
                    for i in 0..per_thread {
                        let (ip, host) = probe(i + t * 7919);
                        black_box(index.hit(ip, host));
                    }
                });
            }
        });
        let ops = (per_thread * threads) as f64 / start.elapsed().as_secs_f64();
        scaling.push((threads, ops));
    }
    let raw = (matches!(kind, "ip" | "asn") && !image)
        .then(|| raw_lookup_ns(&std::fs::read(path).unwrap(), kind, &ips));

    println!(
        "  file {:.1} MiB | {} {:.1} ms | heap peak {:.2} MiB | index heap {:.2} MiB | image {:.2} MiB",
        mib(file as f64),
        if image { "open" } else { "build" },
        build.as_secs_f64() * 1e3,
        mib(heap_peak as f64),
        mib(index_heap as f64),
        mib(index.bytes().len() as f64),
    );
    println!(
        "  lookup {single:.0} ns/op single-thread ({:.1}% hits){}",
        100.0 * hits as f64 / queries as f64,
        raw.map_or(String::new(), |ns| format!(
            " | raw MMDB in memory {ns:.0} ns/op"
        )),
    );
    println!(
        "  throughput {}",
        scaling
            .iter()
            .map(|(t, ops)| format!("{t}T {:.1} M/s", ops / 1e6))
            .collect::<Vec<_>>()
            .join(" | ")
    );

    if flag("--reopen") && !image {
        let stored =
            std::env::temp_dir().join(format!("nyanpasu-geodata-{}.idx", std::process::id()));
        std::fs::write(&stored, index.bytes()).unwrap();
        drop(index);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([kind, stored.to_str().unwrap(), "--image"])
            .status()
            .unwrap();
        std::fs::remove_file(&stored).unwrap();
        assert!(status.success(), "reopened bench failed: {status}");
    }
}
