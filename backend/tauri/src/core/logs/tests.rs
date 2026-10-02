use std::{
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use tempfile::TempDir;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{redb::Limits, *};

fn record(number: usize, size: usize) -> CoreLogRecord {
    CoreLogRecord {
        source: CoreLogSource {
            capture: "test:instance".into(),
            instance_id: "instance".into(),
            core_kind: Some("Mihomo".into()),
        },
        received_at: number as i64,
        time: Some("12:00:00".into()),
        log_type: if number.is_multiple_of(2) {
            "warning".into()
        } else {
            "debug".into()
        },
        payload: format!("{number:012} {}", "x".repeat(size)),
    }
}
fn encoded(number: usize, size: usize) -> Vec<u8> {
    serde_json::to_vec(&record(number, size)).unwrap()
}
fn query(direction: CoreLogDirection, cursor: Option<CoreLogCursor>) -> CoreLogQuery {
    CoreLogQuery {
        direction,
        cursor,
        level: None,
        keyword: String::new(),
        limit: 200,
    }
}

struct TestStore {
    // Field drop order keeps the directory until the database has closed.
    inner: RedbCoreLogStore,
    _directory: TempDir,
}
impl CoreLogStore for TestStore {
    fn append(&mut self, records: &[Vec<u8>]) -> CoreLogResult<()> {
        self.inner.append(records)
    }
    fn query(&mut self, request: CoreLogQuery) -> CoreLogResult<CoreLogPage> {
        self.inner.query(request)
    }
    fn detail(&mut self, cursor: CoreLogCursor) -> CoreLogResult<CoreLogRecord> {
        self.inner.detail(cursor)
    }
    fn clear(&mut self) -> CoreLogResult<()> {
        self.inner.clear()
    }
    fn status(&mut self) -> CoreLogResult<CoreLogStatus> {
        self.inner.status()
    }
}
impl CoreLogsClient {
    pub(crate) async fn test_client() -> Self {
        let directory = TempDir::new().unwrap();
        let store = TestStore {
            inner: RedbCoreLogStore::open(directory.path().into()).unwrap(),
            _directory: directory,
        };
        Self::spawn(
            Box::new(store),
            CancellationToken::new(),
            &TaskTracker::new(),
        )
        .await
        .unwrap()
    }
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "production actor memory benchmark; run alone with CORE_LOG_BENCH_BYTES"]
async fn actor_memory_under_sustained_capture() {
    let size: usize = std::env::var("CORE_LOG_BENCH_BYTES")
        .unwrap_or_else(|_| "1024".into())
        .parse()
        .unwrap();
    let directory = TempDir::new().unwrap();
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client = CoreLogsClient::spawn(
        Box::new(RedbCoreLogStore::open(directory.path().into()).unwrap()),
        shutdown.clone(),
        &tasks,
    )
    .await
    .unwrap();
    let mut captured = 0;
    for checkpoint in [1000, 10000, 100000] {
        while captured < checkpoint {
            client.append(record(captured, size)).await.unwrap();
            captured += 1;
        }
        client.flush().await.unwrap();
        let page = client
            .query(query(CoreLogDirection::Latest, None))
            .await
            .unwrap();
        assert!(page.rows.len() <= MAX_PAGE_ROWS);
        assert!(page.status.bytes <= page.status.budget);
        drop(page);
        let status = client.status().await.unwrap();
        let process = std::fs::read_to_string("/proc/self/status").unwrap();
        let memory = |name: &str| -> u64 {
            process
                .lines()
                .find_map(|line| line.strip_prefix(name))
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .parse()
                .unwrap()
        };
        println!(
            "CORE_LOG_MEMORY {}",
            serde_json::json!({
                "captured": captured, "payload_bytes": size,
                "rss_kib": memory("VmRSS:"), "peak_rss_kib": memory("VmHWM:"),
                "disk_bytes": status.bytes, "budget_bytes": status.budget,
                "first": status.first, "head": status.head,
            })
        );
    }
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[test]
fn rolling_is_a_contiguous_suffix_under_the_physical_budget_and_survives_restart() {
    let directory = TempDir::new().unwrap();
    let limits = Limits {
        total: 12 * 1024 * 1024,
        segment: 4 * 1024 * 1024,
    };
    let mut store = RedbCoreLogStore::open_with_limits(directory.path().into(), limits).unwrap();
    store.append(&[encoded(0, 1024)]).unwrap();
    let expired = store.status().unwrap().head.unwrap();
    for first in (1..20001).step_by(40) {
        let batch: Vec<_> = (first..first + 40).map(|n| encoded(n, 1024)).collect();
        store.append(&batch).unwrap();
        assert!(store.status().unwrap().bytes <= limits.total);
    }
    assert_eq!(
        store
            .query(query(CoreLogDirection::After, Some(expired)))
            .unwrap_err(),
        CoreLogError::CursorExpired
    );
    let before = store.status().unwrap();
    drop(store);
    let mut store = RedbCoreLogStore::open_with_limits(directory.path().into(), limits).unwrap();
    assert_eq!(store.status().unwrap().head, before.head);
    let mut request = query(CoreLogDirection::Latest, None);
    let mut ids = Vec::new();
    loop {
        let page = store.query(request).unwrap();
        ids.extend(page.rows.iter().map(|row| row.record.received_at));
        if !page.more {
            break;
        }
        request = query(CoreLogDirection::Before, page.cursor);
    }
    ids.sort_unstable();
    assert_eq!(ids.last(), Some(&20000));
    assert!(ids[0] > 0);
    assert!(ids.windows(2).all(|w| w[1] == w[0] + 1));
    let mut warnings = query(CoreLogDirection::Latest, None);
    warnings.level = Some("WARN".into());
    let page = store.query(warnings).unwrap();
    assert!(!page.rows.is_empty());
    assert!(page.rows.iter().all(|row| row.record.received_at % 2 == 0));
}

#[tokio::test]
async fn oversized_metadata_is_discarded_without_poisoning_the_pending_batch() {
    let directory = TempDir::new().unwrap();
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client = CoreLogsClient::spawn(
        Box::new(RedbCoreLogStore::open(directory.path().into()).unwrap()),
        shutdown.clone(),
        &tasks,
    )
    .await
    .unwrap();
    let mut malformed = record(1, 128);
    malformed.log_type = "x".repeat(129);
    assert_eq!(client.append(malformed).await, Err(CoreLogError::TooLarge));
    client.append(record(2, 128)).await.unwrap();
    client.flush().await.unwrap();
    let page = client
        .query(query(CoreLogDirection::Latest, None))
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.status.discarded, 1);
    assert!(page.status.error.is_none());
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[test]
fn response_budget_does_not_skip_rows_and_details_keep_the_complete_utf8_record() {
    let directory = TempDir::new().unwrap();
    let mut store = RedbCoreLogStore::open(directory.path().into()).unwrap();
    for first in (0..400).step_by(8) {
        let batch: Vec<_> = (first..first + 8)
            .map(|n| {
                let mut record = record(n, 0);
                record.payload = "猫".repeat(3000);
                serde_json::to_vec(&record).unwrap()
            })
            .collect();
        store.append(&batch).unwrap();
    }
    let first = store.query(query(CoreLogDirection::Latest, None)).unwrap();
    assert!(serde_json::to_vec(&first).unwrap().len() <= model::PAGE_BYTES);
    assert!(first.more);
    assert!(first.rows.len() < 200);
    assert!(first.rows.iter().all(|row| row.truncated
        && row.record.payload.len() <= model::PREVIEW_BYTES
        && row.record.payload.capacity() <= model::PREVIEW_BYTES));
    let full = store.detail(first.rows.last().unwrap().id.clone()).unwrap();
    assert_eq!(full.payload, "猫".repeat(3000));
    let mut ids: Vec<_> = first.rows.iter().map(|r| r.record.received_at).collect();
    let mut cursor = first.cursor;
    loop {
        let page = store
            .query(query(CoreLogDirection::Before, cursor))
            .unwrap();
        ids.extend(page.rows.iter().map(|r| r.record.received_at));
        if !page.more {
            break;
        }
        cursor = page.cursor;
    }
    ids.sort_unstable();
    assert_eq!(ids, (0..400).collect::<Vec<_>>());
}

#[test]
fn scan_budget_advances_through_nonmatches_and_clear_invalidates_all_cursors() {
    let directory = TempDir::new().unwrap();
    let mut store = RedbCoreLogStore::open(directory.path().into()).unwrap();
    for first in (0..3000).step_by(50) {
        store
            .append(
                &(first..first + 50)
                    .map(|n| encoded(n, 32))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    let mut request = query(CoreLogDirection::Latest, None);
    request.keyword = "no such message".into();
    let page = store.query(request.clone()).unwrap();
    assert!(page.rows.is_empty());
    assert!(page.more);
    request.direction = CoreLogDirection::Before;
    request.cursor = page.cursor;
    let last = store.query(request).unwrap();
    assert!(last.rows.is_empty());
    assert!(!last.more);
    let previous = store.status().unwrap();
    store.clear().unwrap();
    let cleared = store.status().unwrap();
    assert_ne!(previous.generation, cleared.generation);
    assert!(cleared.head.is_none());
    assert!(cleared.bytes < previous.bytes);
    assert_eq!(
        store
            .query(query(CoreLogDirection::After, previous.head))
            .unwrap_err(),
        CoreLogError::CursorExpired
    );
}

#[test]
fn directory_owner_and_invalid_requests_are_rejected() {
    let directory = TempDir::new().unwrap();
    let mut store = RedbCoreLogStore::open(directory.path().into()).unwrap();
    assert!(RedbCoreLogStore::open(directory.path().into()).is_err());
    let mut request = query(CoreLogDirection::Latest, None);
    request.limit = usize::MAX;
    assert_eq!(
        store.query(request).unwrap_err(),
        CoreLogError::InvalidRequest
    );
    assert_eq!(
        store
            .append(&[vec![0; model::MAX_RECORD_BYTES + 1]])
            .unwrap_err(),
        CoreLogError::TooLarge
    );
}

#[test]
fn corrupt_store_is_reported_and_preserved() {
    let directory = TempDir::new().unwrap();
    let store = RedbCoreLogStore::open(directory.path().into()).unwrap();
    drop(store);
    let path = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|s| s == "redb"))
        .unwrap();
    std::fs::write(&path, b"corrupt database").unwrap();
    assert!(RedbCoreLogStore::open(directory.path().into()).is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"corrupt database");
}

struct FailingStore {
    inner: TestStore,
    fail: Arc<AtomicBool>,
    attempts: Arc<AtomicUsize>,
}
impl CoreLogStore for FailingStore {
    fn append(&mut self, records: &[Vec<u8>]) -> CoreLogResult<()> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(CoreLogError::Unavailable("disk full".into()));
        }
        self.inner.append(records)
    }
    fn query(&mut self, request: CoreLogQuery) -> CoreLogResult<CoreLogPage> {
        self.inner.query(request)
    }
    fn detail(&mut self, cursor: CoreLogCursor) -> CoreLogResult<CoreLogRecord> {
        self.inner.detail(cursor)
    }
    fn clear(&mut self) -> CoreLogResult<()> {
        self.inner.clear()
    }
    fn status(&mut self) -> CoreLogResult<CoreLogStatus> {
        self.inner.status()
    }
}

#[tokio::test]
async fn failed_batch_stays_bounded_new_samples_are_counted_and_retry_is_not_duplicated() {
    let directory = TempDir::new().unwrap();
    let fail = Arc::new(AtomicBool::new(true));
    let attempts = Arc::new(AtomicUsize::new(0));
    let store = FailingStore {
        inner: TestStore {
            inner: RedbCoreLogStore::open(directory.path().into()).unwrap(),
            _directory: directory,
        },
        fail: fail.clone(),
        attempts: attempts.clone(),
    };
    let token = CancellationToken::new();
    let tasks = TaskTracker::new();
    let logs = CoreLogsClient::spawn(Box::new(store), token.clone(), &tasks)
        .await
        .unwrap();
    logs.append(record(1, 32)).await.unwrap();
    assert!(logs.flush().await.is_err());
    for number in 2..102 {
        assert!(logs.append(record(number, 32)).await.is_err());
    }
    assert!(attempts.load(Ordering::SeqCst) >= 1);
    assert_eq!(logs.status().await.unwrap().discarded, 100);
    fail.store(false, Ordering::SeqCst);
    logs.flush().await.unwrap();
    let page = logs
        .query(query(CoreLogDirection::Latest, None))
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].record.received_at, 1);
    assert!(page.status.error.is_none());
    logs.append(record(102, 32)).await.unwrap();
    token.cancel();
    tasks.close();
    tasks.wait().await;
    assert!(logs.status().await.is_err());
}

#[tokio::test]
#[ignore = "subprocess fixture for abrupt exit recovery"]
async fn crash_writer_child() {
    let Ok(directory) = std::env::var("NYANPASU_CORE_LOG_CRASH_DIRECTORY") else {
        return;
    };
    let logs = CoreLogsClient::spawn(
        Box::new(RedbCoreLogStore::open(directory.into()).unwrap()),
        CancellationToken::new(),
        &TaskTracker::new(),
    )
    .await
    .unwrap();
    for number in 0..100 {
        logs.append(record(number, 32)).await.unwrap();
    }
    logs.flush().await.unwrap();
    logs.append(record(100, 32)).await.unwrap();
    std::process::exit(91);
}

#[test]
fn abrupt_exit_keeps_committed_records_and_not_the_pending_batch() {
    let directory = TempDir::new().unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "core::logs::tests::crash_writer_child",
            "--ignored",
            "--nocapture",
        ])
        .env("NYANPASU_CORE_LOG_CRASH_DIRECTORY", directory.path())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(91));
    let mut store = RedbCoreLogStore::open(directory.path().into()).unwrap();
    let page = store.query(query(CoreLogDirection::Latest, None)).unwrap();
    assert_eq!(page.rows.len(), 100);
    assert_eq!(page.rows.last().unwrap().record.received_at, 99);
}
