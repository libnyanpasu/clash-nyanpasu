use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use nyanpasu_config::application::RpcAuditSettings;
use nyanpasu_tsdb::{ErrorKind, EventCursor, Retention};
use tokio::sync::{mpsc, mpsc::error::TrySendError};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    AUDIT_BATCH_ROWS, Appended, AuditArgs, AuditClient, AuditCursor, AuditDropped, AuditFilter,
    AuditPage, AuditRetention, AuditRow, AuditStore, AuditStoreError,
};

#[derive(Default)]
struct Observed {
    batches: Vec<Vec<AuditRow>>,
    saved: Vec<AuditRow>,
    maintenance: Vec<(SystemTime, Retention)>,
    queries: Vec<(AuditFilter, Option<AuditCursor>, usize)>,
    late: u64,
    append_failure: Option<ErrorKind>,
    maintain_failure: Option<ErrorKind>,
    query_failure: Option<ErrorKind>,
    closed: bool,
}

struct FakeStore(Arc<Mutex<Observed>>);

impl Drop for FakeStore {
    fn drop(&mut self) {
        self.0.lock().unwrap().closed = true;
    }
}

impl AuditStore for FakeStore {
    fn append(&mut self, rows: &[AuditRow]) -> Result<Appended, AuditStoreError> {
        let mut observed = self.0.lock().unwrap();
        observed.batches.push(rows.to_vec());
        if let Some(kind) = observed.append_failure.take() {
            return Err(AuditStoreError::Append {
                kind,
                source: std::io::Error::other("injected append failure").into(),
            });
        }
        let late = std::mem::take(&mut observed.late);
        observed.saved.extend_from_slice(&rows[late as usize..]);
        Ok(Appended { late })
    }

    fn maintain(&mut self, now: SystemTime, retention: Retention) -> Result<(), AuditStoreError> {
        let mut observed = self.0.lock().unwrap();
        observed.maintenance.push((now, retention));
        if let Some(kind) = observed.maintain_failure.take() {
            return Err(AuditStoreError::Maintain {
                kind,
                source: std::io::Error::other("injected maintenance failure").into(),
            });
        }
        Ok(())
    }

    fn query(
        &mut self,
        filter: AuditFilter,
        before: Option<AuditCursor>,
        limit: usize,
    ) -> Result<AuditPage, AuditStoreError> {
        let mut observed = self.0.lock().unwrap();
        observed.queries.push((filter, before, limit));
        if let Some(kind) = observed.query_failure.take() {
            return Err(AuditStoreError::Query {
                kind,
                source: std::io::Error::other("injected query failure").into(),
            });
        }
        Ok(AuditPage {
            rows: observed.saved.iter().rev().take(limit).cloned().collect(),
            next: Some(cursor()),
            // The actor must replace the adapter's value with the shared counter.
            dropped: u64::MAX,
        })
    }
}

struct FakeRetention(Mutex<(Duration, u64)>);

impl AuditRetention for FakeRetention {
    fn retention(&self) -> (Duration, u64) {
        *self.0.lock().unwrap()
    }
}

struct Harness {
    observed: Arc<Mutex<Observed>>,
    retention: Arc<FakeRetention>,
    rows: mpsc::Sender<AuditRow>,
    dropped: AuditDropped,
    client: AuditClient,
    shutdown: CancellationToken,
    tasks: TaskTracker,
}

impl Harness {
    async fn new() -> Self {
        let observed = Arc::new(Mutex::new(Observed::default()));
        let settings = RpcAuditSettings::default();
        let retention = Arc::new(FakeRetention(Mutex::new((
            Duration::from_secs(u64::from(settings.max_age_days) * 24 * 60 * 60),
            u64::from(settings.max_size_mib) * 1024 * 1024,
        ))));
        let (rows, receiver) = mpsc::channel(4096);
        let dropped = AuditDropped::default();
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let client = AuditClient::spawn(
            AuditArgs {
                store: Box::new(FakeStore(observed.clone())),
                rows: receiver,
                dropped: dropped.clone(),
                retention: retention.clone(),
            },
            shutdown.clone(),
            &tasks,
        )
        .await
        .unwrap();
        Self {
            observed,
            retention,
            rows,
            dropped,
            client,
            shutdown,
            tasks,
        }
    }

    fn send(&self, ids: std::ops::Range<u64>) {
        for id in ids {
            self.rows.try_send(row(id)).unwrap();
        }
    }

    async fn stop(&self) {
        self.shutdown.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}

fn row(id: u64) -> AuditRow {
    AuditRow {
        at: 1_000,
        transport: "http".into(),
        method: "get_profiles".into(),
        owner: "hash".into(),
        invocation_id: id,
        request_id: None,
        phase: "finished".into(),
        outcome: Some("success".into()),
        kind: None,
        code: None,
        duration_ns: 123,
    }
}

fn cursor() -> AuditCursor {
    EventCursor {
        epoch: 1,
        key: 1_000,
        incarnation: 2,
        seq: 3,
    }
    .into()
}

#[tokio::test]
async fn tick_appends_one_bounded_batch_in_channel_order() {
    let h = Harness::new().await;
    h.send(0..AUDIT_BATCH_ROWS as u64 + 2);
    h.client.tick().await.unwrap();
    {
        let observed = h.observed.lock().unwrap();
        assert_eq!(observed.batches.len(), 1);
        assert_eq!(
            observed.batches[0],
            (0..AUDIT_BATCH_ROWS as u64).map(row).collect::<Vec<_>>()
        );
        assert_eq!(observed.maintenance.len(), 1);
    }
    h.client.tick().await.unwrap();
    assert_eq!(
        h.observed.lock().unwrap().batches[1],
        vec![row(1024), row(1025)]
    );
    h.stop().await;
}

#[tokio::test]
async fn late_and_refused_rows_count_as_dropped_without_replay() {
    let h = Harness::new().await;
    h.dropped.increment();
    h.observed.lock().unwrap().late = 2;
    h.send(0..3);
    h.client.tick().await.unwrap();
    assert_eq!(h.dropped.count(), 3);

    h.observed.lock().unwrap().append_failure = Some(ErrorKind::Local);
    h.send(3..5);
    assert!(matches!(
        h.client.tick().await,
        Err(AuditStoreError::Append {
            kind: ErrorKind::Local,
            ..
        })
    ));
    assert_eq!(h.dropped.count(), 5);
    assert!(!h.rows.is_closed());

    h.send(5..6);
    h.client.tick().await.unwrap();
    let page = h
        .client
        .query(AuditFilter::default(), None, 500)
        .await
        .unwrap();
    assert_eq!(page.dropped, 5);
    assert_eq!(page.rows, vec![row(5), row(2)]);
    assert_eq!(h.observed.lock().unwrap().maintenance.len(), 3);
    h.stop().await;
}

#[tokio::test]
async fn failed_backend_closes_admission_and_makes_queries_unavailable() {
    let h = Harness::new().await;
    h.observed.lock().unwrap().append_failure = Some(ErrorKind::BackendFailed);
    h.send(0..AUDIT_BATCH_ROWS as u64 + 1);
    assert!(matches!(
        h.client.tick().await,
        Err(AuditStoreError::Append {
            kind: ErrorKind::BackendFailed,
            ..
        })
    ));
    assert_eq!(h.dropped.count(), AUDIT_BATCH_ROWS as u64);
    assert!(h.observed.lock().unwrap().closed);
    assert!(matches!(
        h.rows.try_send(row(2000)),
        Err(TrySendError::Closed(_))
    ));
    assert!(matches!(
        h.client.query(AuditFilter::default(), None, 1).await,
        Err(AuditStoreError::AccessStore)
    ));
    assert!(matches!(
        h.client.tick().await,
        Err(AuditStoreError::AccessStore)
    ));
    h.stop().await;
    assert_eq!(h.observed.lock().unwrap().batches.len(), 1);
    // Closed-channel and shutdown-tail losses are deliberately not counted.
    assert_eq!(h.dropped.count(), AUDIT_BATCH_ROWS as u64);
}

#[tokio::test]
async fn maintenance_and_query_backend_failures_also_retire_the_store() {
    let h = Harness::new().await;
    h.observed.lock().unwrap().maintain_failure = Some(ErrorKind::BackendFailed);
    assert!(matches!(
        h.client.tick().await,
        Err(AuditStoreError::Maintain {
            kind: ErrorKind::BackendFailed,
            ..
        })
    ));
    assert!(h.rows.is_closed());
    assert!(h.observed.lock().unwrap().closed);
    h.stop().await;

    let h = Harness::new().await;
    h.observed.lock().unwrap().query_failure = Some(ErrorKind::BackendFailed);
    assert!(matches!(
        h.client.query(AuditFilter::default(), None, 1).await,
        Err(AuditStoreError::Query {
            kind: ErrorKind::BackendFailed,
            ..
        })
    ));
    assert!(h.rows.is_closed());
    assert!(matches!(
        h.client.query(AuditFilter::default(), None, 1).await,
        Err(AuditStoreError::AccessStore)
    ));
    h.stop().await;
}

#[tokio::test]
async fn every_tick_samples_retention_and_maintains_even_without_rows() {
    let h = Harness::new().await;
    let start = SystemTime::now();
    h.client.tick().await.unwrap();
    let changed = (Duration::from_secs(24 * 60 * 60), 4 * 1024 * 1024);
    *h.retention.0.lock().unwrap() = changed;
    h.observed.lock().unwrap().maintain_failure = Some(ErrorKind::Local);
    assert!(matches!(
        h.client.tick().await,
        Err(AuditStoreError::Maintain {
            kind: ErrorKind::Local,
            ..
        })
    ));
    h.send(0..1);
    h.client.tick().await.unwrap();
    {
        let observed = h.observed.lock().unwrap();
        assert_eq!(observed.batches, vec![vec![row(0)]]);
        assert_eq!(observed.maintenance.len(), 3);
        assert!(observed.maintenance[0].0 >= start);
        assert_eq!(
            observed.maintenance[1].1,
            Retention {
                max_age: Some(changed.0),
                max_bytes: Some(changed.1)
            }
        );
        assert_eq!(observed.maintenance[2].1, observed.maintenance[1].1);
    }
    h.stop().await;
}

#[tokio::test]
async fn query_passes_filter_and_cursor_caps_limit_and_does_not_flush() {
    let h = Harness::new().await;
    h.send(0..1);
    let filter = AuditFilter {
        transport: Some("local".into()),
        outcome: Some("failure".into()),
    };
    let before = Some(cursor());
    let page = h
        .client
        .query(filter.clone(), before, usize::MAX)
        .await
        .unwrap();
    assert!(page.rows.is_empty());
    assert_eq!(page.next, before);
    h.client
        .query(AuditFilter::default(), None, 0)
        .await
        .unwrap();
    {
        let observed = h.observed.lock().unwrap();
        assert_eq!(
            observed.queries,
            vec![(filter, before, 500), (AuditFilter::default(), None, 1)]
        );
        assert!(observed.batches.is_empty());
    }
    h.stop().await;
}

#[tokio::test]
async fn shutdown_writes_all_remaining_rows_as_the_final_batch() {
    let h = Harness::new().await;
    h.send(0..AUDIT_BATCH_ROWS as u64 + 1);
    h.stop().await;
    assert!(h.rows.is_closed());
    let observed = h.observed.lock().unwrap();
    assert_eq!(observed.batches.len(), 1);
    assert_eq!(
        observed.batches[0],
        (0..AUDIT_BATCH_ROWS as u64 + 1)
            .map(row)
            .collect::<Vec<_>>()
    );
    assert_eq!(observed.maintenance.len(), 1);
    assert!(observed.closed);
}

#[test]
fn wire_values_round_trip_without_losing_cursor_components() {
    let page = AuditPage {
        rows: vec![row(1)],
        next: Some(cursor()),
        dropped: 2,
    };
    let decoded: AuditPage = serde_json::from_str(&serde_json::to_string(&page).unwrap()).unwrap();
    assert_eq!(decoded, page);
    assert_eq!(
        EventCursor::from(decoded.next.unwrap()),
        EventCursor {
            epoch: 1,
            key: 1_000,
            incarnation: 2,
            seq: 3
        }
    );
}
