//! File-to-actor latency, measured separately from GUI and allocator instrumentation.
use nyanpasu_logging::*;
#[cfg(not(feature = "alloc-stats"))]
use std::{
    collections::HashSet,
    io::Write,
    sync::Arc,
    time::{Duration, Instant},
};

fn record(message: &str, padding: usize) -> String {
    format!(
        "{}\n",
        serde_json::json!({"timestamp":chrono::Utc::now().to_rfc3339(),"level":"INFO",
        "target":"benchmark::writer","fields":{"message":message,"padding":"x".repeat(padding)}})
    )
}
#[cfg(not(feature = "alloc-stats"))]
fn query(session: &str, cursor: Option<LogCursor>) -> QueryLogs {
    QueryLogs {
        session: session.into(),
        filter: Filter::default(),
        direction: if cursor.is_some() {
            Direction::After
        } else {
            Direction::Latest
        },
        cursor,
        limit: 200,
    }
}
#[cfg(not(feature = "alloc-stats"))]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let seconds: usize = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "60".into())
        .parse()
        .unwrap();
    assert!((1..=120).contains(&seconds));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bench.2026-09-11.app.log");
    let mut file = std::fs::File::create(&path).unwrap();
    for i in 0..100_000 {
        file.write_all(record(&format!("initial:{i}"), 0).as_bytes())
            .unwrap();
    }
    file.flush().unwrap();
    drop(file);
    let initial_bytes = std::fs::metadata(&path).unwrap().len();
    let client = LogsClient::start(
        Arc::new(FsLogFiles::new(dir.path().into(), "bench".into())),
        Arc::new(MonotonicClock::default()),
    )
    .await
    .unwrap();
    let start = Instant::now();
    let session = client
        .open(
            "bench".into(),
            OpenLogs {
                request_id: "bench".into(),
                file: None,
            },
        )
        .await
        .unwrap();
    let initial = loop {
        let page = client
            .query("bench".into(), query(&session.id, None))
            .await
            .unwrap();
        if !page.building {
            break page;
        }
        tokio::task::yield_now().await;
    };
    let cold_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut cursor = initial.head;
    let writer = tokio::spawn(async move {
        let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let mut written = 0;
        for batch in 0..seconds {
            interval.tick().await;
            let mut bytes = String::new();
            for row in 0..1000 {
                bytes.push_str(&record(&format!("live:{}", batch * 1000 + row), 120));
            }
            written += bytes.len();
            file.write_all(bytes.as_bytes()).unwrap();
            file.flush().unwrap();
        }
        written
    });
    let mut seen = HashSet::new();
    let mut latency = Vec::new();
    let mut requests = 0;
    let deadline = Instant::now() + Duration::from_secs(seconds as u64 + 15);
    while seen.len() < seconds * 1000 {
        assert!(
            Instant::now() < deadline,
            "viewer failed to catch up: {}",
            seen.len()
        );
        let page = client
            .query("bench".into(), query(&session.id, Some(cursor.clone())))
            .await
            .unwrap();
        requests += 1;
        if page.building {
            tokio::task::yield_now().await;
            continue;
        }
        cursor = page.cursor;
        for row in page.rows {
            assert!(seen.insert(row.id), "duplicate delivery");
            assert!(row.message.starts_with("live:"));
            latency.push(
                (chrono::Utc::now().timestamp_millis()
                    - row.timestamp.unwrap().parse::<i64>().unwrap()) as u64,
            );
        }
        if !page.more && seen.len() < seconds * 1000 {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    let written = writer.await.unwrap();
    client.close("bench".into(), session.id).await.unwrap();
    client.shutdown().await.unwrap();
    latency.sort_unstable();
    println!(
        "{}",
        serde_json::json!({"mode":"file_to_actor","initial_rows":100000,"initial_bytes":initial_bytes,
        "cold_first_page_ms":cold_ms,"seconds":seconds,"rows_per_second":1000,"received":seen.len(),
        "appended_bytes":written,"requests":requests,"latency_p50_ms":latency[latency.len()/2],
        "latency_p95_ms":latency[(latency.len()-1)*95/100],"latency_max_ms":latency.last()})
    );
}

#[cfg(feature = "alloc-stats")]
fn main() {
    let line = record("allocation fixture", 0);
    let build = |count| {
        let mut index = Index::new(0);
        for _ in 0..count {
            index.append(index.next_read(), line.as_bytes()).unwrap();
        }
        index
    };
    drop(build(1));
    allocation::reset_peak();
    let before = allocation::snapshot();
    let index = build(100_000);
    let retained = allocation::snapshot();
    drop(index);
    assert_eq!(allocation::snapshot().2, before.2);
    for _ in 0..100 {
        let before = allocation::snapshot();
        let index = build(1000);
        drop(index.scan(&Filter::default(), Direction::Latest, None, 200));
        drop(index);
        assert_eq!(allocation::snapshot().2, before.2);
    }
    println!(
        "{}",
        serde_json::json!({"mode":"production_index_allocations","rows":100000,
        "live_bytes":retained.2-before.2,"peak_bytes":retained.3-before.2,
        "allocations":retained.0-before.0,"cycles":100,"unreleased_bytes":0})
    );
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
