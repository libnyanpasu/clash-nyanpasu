use std::{
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use tempfile::TempDir;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::*;

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
        let client = Self::spawn(
            Box::new(store),
            CancellationToken::new(),
            &TaskTracker::new(),
        )
        .await
        .unwrap();
        client.set_instance(Some("instance".into())).await.unwrap();
        client
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
    client.set_instance(Some("instance".into())).await.unwrap();
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
        assert_eq!(page.status.first.as_ref().unwrap().sequence, 1);
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
                "disk_bytes": status.bytes,
                "first": status.first, "head": status.head,
            })
        );
    }
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[test]
fn one_database_keeps_all_records_until_clear_and_startup_discards_old_history() {
    let directory = TempDir::new().unwrap();
    let mut store = RedbCoreLogStore::open(directory.path().into()).unwrap();
    for first in (0..10000).step_by(40) {
        store
            .append(
                &(first..first + 40)
                    .map(|n| encoded(n, 1024))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }
    let status = store.status().unwrap();
    assert_eq!(status.first.as_ref().unwrap().sequence, 1);
    assert_eq!(status.head.as_ref().unwrap().sequence, 10000);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
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
    assert_eq!(ids, (0..10000).collect::<Vec<_>>());
    let mut warnings = query(CoreLogDirection::Latest, None);
    warnings.level = Some("WARN".into());
    let page = store.query(warnings).unwrap();
    assert!(!page.rows.is_empty());
    assert!(page.rows.iter().all(|row| row.record.received_at % 2 == 0));
    drop(store);
    let mut store = RedbCoreLogStore::open(directory.path().into()).unwrap();
    assert!(!directory.path().join("current.redb").exists());
    assert!(store.status().unwrap().head.is_none());
    assert!(matches!(
        store.detail(status.head.unwrap()),
        Err(CoreLogError::RecordGone)
    ));
    store.append(&[encoded(10000, 32)]).unwrap();
    assert!(directory.path().join("current.redb").exists());
    store.clear().unwrap();
    assert!(!directory.path().join("current.redb").exists());
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
    client.set_instance(Some("instance".into())).await.unwrap();
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
fn startup_removes_corrupt_previous_session_and_legacy_shards() {
    let directory = TempDir::new().unwrap();
    let shard = directory.path().join(format!(
        "{}.00000000000000000001.redb",
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&shard, b"old shard").unwrap();
    std::fs::write(directory.path().join("current.redb"), b"corrupt database").unwrap();
    std::fs::write(directory.path().join("control.json"), b"old control").unwrap();
    std::fs::write(directory.path().join("unrelated.txt"), b"keep").unwrap();
    let store = RedbCoreLogStore::open(directory.path().into()).unwrap();
    assert!(!shard.exists());
    assert!(!directory.path().join("current.redb").exists());
    assert!(!directory.path().join("control.json").exists());
    assert!(directory.path().join("unrelated.txt").exists());
    drop(store);
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
async fn write_failure_stops_capture_without_replay_until_explicit_clear() {
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
    logs.set_instance(Some("instance".into())).await.unwrap();
    logs.append(record(1, 32)).await.unwrap();
    assert!(logs.flush().await.is_err());
    for number in 2..102 {
        assert!(logs.append(record(number, 32)).await.is_err());
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(logs.status().await.unwrap().discarded, 101);
    fail.store(false, Ordering::SeqCst);
    logs.flush().await.unwrap();
    let page = logs
        .query(query(CoreLogDirection::Latest, None))
        .await
        .unwrap();
    assert!(page.rows.is_empty());
    assert!(page.status.error.is_some());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert!(logs.append(record(102, 32)).await.is_err());
    logs.clear().await.unwrap();
    logs.append(record(103, 32)).await.unwrap();
    logs.flush().await.unwrap();
    assert_eq!(
        logs.query(query(CoreLogDirection::Latest, None))
            .await
            .unwrap()
            .rows[0]
            .record
            .received_at,
        103
    );
    token.cancel();
    tasks.close();
    tasks.wait().await;
    assert!(logs.status().await.is_err());
}

#[tokio::test]
#[ignore = "subprocess fixture for previous session cleanup"]
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
    logs.set_instance(Some("instance".into())).await.unwrap();
    for number in 0..100 {
        logs.append(record(number, 32)).await.unwrap();
    }
    logs.flush().await.unwrap();
    logs.append(record(100, 32)).await.unwrap();
    std::process::exit(91);
}

#[test]
fn startup_discards_both_committed_and_pending_records_after_abrupt_exit() {
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
    assert!(page.rows.is_empty());
    assert!(!directory.path().join("current.redb").exists());
}

#[tokio::test]
async fn session_changes_delete_committed_and_pending_logs_but_reconnect_preserves_them() {
    let directory = TempDir::new().unwrap();
    let token = CancellationToken::new();
    let tasks = TaskTracker::new();
    let logs = CoreLogsClient::spawn(
        Box::new(RedbCoreLogStore::open(directory.path().into()).unwrap()),
        token.clone(),
        &tasks,
    )
    .await
    .unwrap();
    logs.set_instance(Some("instance".into())).await.unwrap();
    logs.append(record(1, 32)).await.unwrap();
    logs.flush().await.unwrap();
    let before = logs.status().await.unwrap();
    logs.append(record(2, 32)).await.unwrap();
    logs.set_instance(Some("instance".into())).await.unwrap();
    assert_eq!(logs.status().await.unwrap().generation, before.generation);
    logs.flush().await.unwrap();
    assert_eq!(
        logs.query(query(CoreLogDirection::Latest, None))
            .await
            .unwrap()
            .rows
            .len(),
        2
    );
    logs.append(record(3, 32)).await.unwrap();
    logs.set_instance(None).await.unwrap();
    assert!(!directory.path().join("current.redb").exists());
    assert!(
        logs.query(query(CoreLogDirection::Latest, None))
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    assert_eq!(
        logs.query(query(CoreLogDirection::After, before.head.clone()))
            .await
            .unwrap_err(),
        CoreLogError::CursorExpired
    );
    assert!(logs.append(record(4, 32)).await.is_err());
    logs.set_instance(Some("replacement".into())).await.unwrap();
    assert!(logs.append(record(5, 32)).await.is_err());
    let mut replacement = record(6, 32);
    replacement.source.instance_id = "replacement".into();
    logs.append(replacement).await.unwrap();
    logs.flush().await.unwrap();
    assert_eq!(
        logs.query(query(CoreLogDirection::Latest, None))
            .await
            .unwrap()
            .rows[0]
            .record
            .received_at,
        6
    );
    token.cancel();
    tasks.close();
    tasks.wait().await;
    assert!(!directory.path().join("current.redb").exists());
}
