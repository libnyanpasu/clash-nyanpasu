#[path = "tests/support.rs"]
mod support;
use super::{TrafficActorArgs, TrafficClient};
use nyanpasu_traffic::{
    test_support::{FakeClock, FakeTrafficStore, IdleSource},
    *,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};
use support::*;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
#[derive(Default)]
struct FaultStore {
    inner: FakeTrafficStore,
    commit_fault: AtomicU8,
    begin_fail: AtomicBool,
    end_fail: AtomicBool,
    pause: AtomicBool,
    entered: Notify,
    release: Notify,
}
#[async_trait::async_trait]
impl TrafficStore for FaultStore {
    async fn recover(&self, h: HostId) -> TrafficResult<RecoveryState> {
        self.inner.recover(h).await
    }
    async fn begin_session(&self, s: NewSession) -> TrafficResult<SessionRecord> {
        if self.begin_fail.load(Ordering::SeqCst) {
            return Err(StoreError::Unavailable("begin fail".into()));
        }
        self.inner.begin_session(s).await
    }
    async fn commit_observation(&self, b: ObservationCommit) -> TrafficResult<CommitReceipt> {
        if self.pause.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        match self.commit_fault.swap(0, Ordering::SeqCst) {
            1 => Err(StoreError::CapacityExhausted("known uncommitted".into())),
            2 => Err(StoreError::UnknownOutcome("unknown before".into())),
            3 => {
                self.inner.commit_observation(b).await?;
                Err(StoreError::UnknownOutcome("unknown after".into()))
            }
            _ => self.inner.commit_observation(b).await,
        }
    }
    async fn committed_position(&self, s: SessionId) -> TrafficResult<CommittedPosition> {
        self.inner.committed_position(s).await
    }
    async fn finish_session(&self, e: SessionEnd) -> TrafficResult<CommitReceipt> {
        if self.end_fail.load(Ordering::SeqCst) {
            return Err(StoreError::Unavailable("end fail".into()));
        }
        self.inner.finish_session(e).await
    }
    async fn session(&self, s: SessionId) -> TrafficResult<SessionRecord> {
        self.inner.session(s).await
    }
    async fn latest_session(&self, h: HostId) -> TrafficResult<Option<SessionRecord>> {
        self.inner.latest_session(h).await
    }
    async fn connection(
        &self,
        s: SessionId,
        id: String,
    ) -> TrafficResult<Option<ConnectionRecord>> {
        self.inner.connection(s, id).await
    }
    async fn query_connections(&self, q: ConnectionsQuery) -> TrafficResult<ConnectionPage> {
        self.inner.query_connections(q).await
    }
    async fn query_usage(&self, q: UsageQuery) -> TrafficResult<UsageResult> {
        self.inner.query_usage(q).await
    }
    async fn query_topology(&self, q: TopologyQuery) -> TrafficResult<TopologyResult> {
        self.inner.query_topology(q).await
    }
    async fn prune(&self, p: RetentionPolicy) -> TrafficResult<PruneReport> {
        self.inner.prune(p).await
    }
    async fn flush(&self) -> TrafficResult<()> {
        self.inner.flush().await
    }
}
async fn client(store: Arc<FaultStore>) -> (TrafficClient, CancellationToken) {
    let cancellation = CancellationToken::new();
    let client = TrafficClient::start(TrafficActorArgs {
        host: HostId("contract-host".into()),
        source: Arc::new(IdleSource),
        store,
        clock: Arc::new(FakeClock::new(0, 0)),
        cancellation: cancellation.clone(),
    })
    .await
    .unwrap();
    (client, cancellation)
}
fn frame(id: &str, time: u64, rows: Vec<ConnectionSample>) -> Observation {
    let mut o = observation(rows, time);
    o.instance_id = id.into();
    o.generation = UInt(0);
    o
}
#[tokio::test]
async fn no_subscribers_records_and_unknown_result_does_not_discard_next_frame() {
    for fault in [1, 2, 3] {
        let store = Arc::new(FaultStore::default());
        let (c, _) = client(store.clone()).await;
        let s = c.instance_started(new_session("instance")).await.unwrap();
        c.set_current_instance(Some("instance".into()))
            .await
            .unwrap();
        store.commit_fault.store(fault, Ordering::SeqCst);
        assert!(
            c.observe(frame("instance", 1000, vec![sample("a", 10)]))
                .await
                .is_err()
        );
        let receipt = c
            .observe(frame("instance", 2000, vec![sample("a", 15)]))
            .await
            .unwrap();
        assert_eq!(
            receipt.position.sequence,
            UInt(if fault == 1 { 1 } else { 2 })
        );
        assert_eq!(
            c.session(s.id.clone())
                .await
                .unwrap()
                .attributed_bytes
                .upload,
            UInt(15)
        );
        assert_eq!(
            c.query_connections(connections(s.id))
                .await
                .unwrap()
                .connections[0]
                .counters
                .upload,
            UInt(15)
        );
        c.shutdown().await.unwrap();
    }
}
#[tokio::test]
async fn dropping_waiter_does_not_cancel_owner_commit() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store.clone()).await;
    let s = c.instance_started(new_session("instance")).await.unwrap();
    store.pause.store(true, Ordering::SeqCst);
    let waiter = tokio::spawn({
        let c = c.clone();
        async move {
            c.observe(frame("instance", 1000, vec![sample("a", 10)]))
                .await
        }
    });
    store.entered.notified().await;
    waiter.abort();
    store.release.notify_one();
    assert_eq!(
        c.session(s.id).await.unwrap().attributed_bytes.upload,
        UInt(10)
    );
    c.shutdown().await.unwrap();
}
#[tokio::test]
async fn drain_and_stale_generation_never_change_selected_summary() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store).await;
    let a = c.instance_started(new_session("a")).await.unwrap();
    let b = c.instance_started(new_session("b")).await.unwrap();
    c.set_current_instance(Some("b".into())).await.unwrap();
    c.observe(frame("b", 1000, vec![sample("b", 10)]))
        .await
        .unwrap();
    c.observe(frame("a", 2000, vec![sample("a", 20)]))
        .await
        .unwrap();
    assert_eq!(
        c.subscribe_summary().borrow().as_ref().unwrap().session.id,
        b.id
    );
    c.controller_bound(SourceBinding {
        instance_id: "a".into(),
        endpoint: SourceEndpoint::Http("http://unused".into()),
        secret: None,
    })
    .await
    .unwrap();
    assert!(matches!(
        c.observe(frame("a", 3000, vec![sample("a", 25)])).await,
        Err(StoreError::Conflict(_))
    ));
    let mut fresh = frame("a", 4000, vec![sample("a", 30)]);
    fresh.generation = UInt(1);
    c.observe(fresh).await.unwrap();
    assert_eq!(
        c.session(a.id.clone())
            .await
            .unwrap()
            .attributed_bytes
            .upload,
        UInt(30)
    );
    c.instance_exited(SessionEnd {
        session_id: a.id,
        detected_at: UInt(5000),
        reason: CloseReason::CoreExited,
    })
    .await
    .unwrap();
    assert_eq!(
        c.subscribe_summary().borrow().as_ref().unwrap().session.id,
        b.id
    );
    c.set_current_instance(Some("unknown".into()))
        .await
        .unwrap();
    assert!(c.subscribe_summary().borrow().is_none());
    assert!(c.subscribe_details().borrow().is_none());
    assert!(c.current_session().await.is_err());
    c.shutdown().await.unwrap();
}
#[tokio::test]
async fn filtered_rule_rates_topology_and_freshness_share_commit() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store).await;
    let s = c.instance_started(new_session("instance")).await.unwrap();
    c.set_current_instance(Some("instance".into()))
        .await
        .unwrap();
    let mut b = sample("b", 10);
    b.rule_payload = "other".into();
    c.observe(frame("instance", 1000, vec![sample("a", 10), b.clone()]))
        .await
        .unwrap();
    b.upload = 110;
    b.download = 220;
    c.observe(frame("instance", 2000, vec![sample("a", 15), b]))
        .await
        .unwrap();
    let mut q = usage(s.id.clone());
    q.filter.rule = Some(RuleKey {
        kind: "DOMAIN".into(),
        payload: "example.org".into(),
        context: None,
        ambiguous: false,
    });
    q.group_by = Some(GroupBy::Rule);
    let result = c.query_usage(q.clone()).await.unwrap();
    assert_eq!(result.current_rate.as_ref().unwrap().upload, 5.0);
    assert_eq!(result.groups[0].current_rate.as_ref().unwrap().upload, 5.0);
    let topology = c
        .query_topology(TopologyQuery {
            session_id: s.id.clone(),
            filter: q.filter.clone(),
            scope: QueryScope::Session,
            limit: 100,
        })
        .await
        .unwrap();
    assert_eq!(topology.paths[0].current_rate.as_ref().unwrap().upload, 5.0);
    assert!(
        topology
            .edges
            .iter()
            .all(|e| e.current_rate.as_ref().unwrap().upload == 5.0)
    );
    c.source_disconnected("instance".into(), UInt(0))
        .await
        .unwrap();
    assert_eq!(
        c.session(s.id.clone()).await.unwrap().freshness,
        Freshness::Stale
    );
    assert!(c.query_usage(q).await.unwrap().current_rate.is_none());
    assert_eq!(
        c.query_connections(connections(s.id.clone()))
            .await
            .unwrap()
            .meta
            .session
            .freshness,
        Freshness::Stale
    );
    assert_eq!(
        c.query_topology(TopologyQuery {
            session_id: s.id,
            filter: ConnectionFilter::default(),
            scope: QueryScope::Session,
            limit: 100
        })
        .await
        .unwrap()
        .meta
        .session
        .freshness,
        Freshness::Stale
    );
    c.shutdown().await.unwrap();
}
#[tokio::test]
async fn lifecycle_storage_failures_are_visible_and_retried_by_commands() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store.clone()).await;
    store.begin_fail.store(true, Ordering::SeqCst);
    c.notify_instance_started(new_session("short"), None)
        .unwrap();
    c.set_current_instance(Some("short".into())).await.unwrap();
    assert!(c.subscribe_status().borrow().is_some());
    assert!(c.current_session().await.is_err());
    store.begin_fail.store(false, Ordering::SeqCst);
    let s = c.current_session().await.unwrap().unwrap();
    assert_eq!(s.instance_id, "short");
    store.end_fail.store(true, Ordering::SeqCst);
    c.notify_instance_exited("short".into(), 5000).unwrap();
    assert!(
        c.current_session()
            .await
            .unwrap()
            .unwrap()
            .quality
            .contains(&Quality::LifecycleUnknown)
    );
    store.end_fail.store(false, Ordering::SeqCst);
    assert_eq!(
        c.current_session().await.unwrap().unwrap().ended_at,
        Some(UInt(5000))
    );
    c.set_current_instance(None).await.unwrap();
    assert_eq!(c.current_session().await.unwrap().unwrap().id, s.id);
    c.shutdown().await.unwrap();
}
#[tokio::test]
async fn recovery_resets_monotonic_continuity_and_cancelled_shutdown_joins() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store.clone()).await;
    let s = c.instance_started(new_session("instance")).await.unwrap();
    c.observe(frame("instance", 10000, vec![sample("a", 10)]))
        .await
        .unwrap();
    c.shutdown().await.unwrap();
    let (c, token) = client(store).await;
    let recovered = c.current_session().await.unwrap().unwrap();
    assert_eq!(recovered.freshness, Freshness::Stale);
    assert!(recovered.quality.contains(&Quality::LifecycleUnknown));
    c.instance_started(new_session("instance")).await.unwrap();
    c.set_current_instance(Some("instance".into()))
        .await
        .unwrap();
    c.observe(frame("instance", 1000, vec![sample("a", 15)]))
        .await
        .unwrap();
    assert!(
        c.subscribe_summary()
            .borrow()
            .as_ref()
            .unwrap()
            .current_rate
            .is_none()
    );
    assert_eq!(
        c.session(s.id).await.unwrap().attributed_bytes.upload,
        UInt(15)
    );
    token.cancel();
    c.shutdown().await.unwrap();
    c.shutdown().await.unwrap();
}

struct FrameSource {
    receiver: tokio::sync::Mutex<Option<tokio::sync::mpsc::Receiver<Observation>>>,
    polls: Arc<std::sync::atomic::AtomicUsize>,
    opened: Notify,
}
#[tokio::test]
async fn remote_uuid_history_is_unloaded_and_same_uuid_resumes_without_double_counting() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store.clone()).await;
    let mut first = None;
    for index in 0..20 {
        let id = format!("remote-{index}");
        let s = c.instance_started(new_session(&id)).await.unwrap();
        if index == 0 {
            first = Some(s.id);
        }
        c.set_current_instance(Some(id.clone())).await.unwrap();
        c.observe(frame(&id, 1000, vec![sample("a", 10), sample("b", 10)]))
            .await
            .unwrap();
        assert_eq!(c.resident_active().await.unwrap(), 2);
        c.detach(id).await.unwrap();
        assert_eq!(c.resident_active().await.unwrap(), 0);
        assert!(c.subscribe_summary().borrow().is_none());
        assert!(c.subscribe_status().borrow().is_none());
    }
    let first = first.unwrap();
    c.instance_started(new_session("remote-0")).await.unwrap();
    assert_eq!(c.resident_active().await.unwrap(), 2);
    c.set_current_instance(Some("remote-0".into()))
        .await
        .unwrap();
    let mut next = frame("remote-0", 3000, vec![sample("a", 15), sample("b", 15)]);
    next.generation = UInt(1);
    c.observe(next).await.unwrap();
    let resumed = c.session(first.clone()).await.unwrap();
    assert_eq!(resumed.attributed_bytes.upload, UInt(30));
    assert_eq!(resumed.observed_connections, UInt(2));
    assert!(resumed.ended_at.is_none());
    c.detach("remote-0".into()).await.unwrap();
    c.shutdown().await.unwrap();
    let (reopened, _) = client(store.clone()).await;
    assert_eq!(reopened.resident_active().await.unwrap(), 0);
    assert!(
        reopened
            .session(first.clone())
            .await
            .unwrap()
            .quality
            .contains(&Quality::LifecycleUnknown)
    );
    // Direct typed observation also restores missing active baselines before diffing.
    let mut direct = frame("remote-0", 4000, vec![sample("a", 20)]);
    direct.generation = UInt(1);
    reopened.observe(direct).await.unwrap();
    assert_eq!(
        reopened
            .session(first.clone())
            .await
            .unwrap()
            .attributed_bytes
            .upload,
        UInt(35)
    );
    assert!(matches!(
        store
            .inner
            .connection(first.clone(), "b".into())
            .await
            .unwrap()
            .unwrap()
            .status,
        ConnectionStatus::Closed {
            reason: CloseReason::MissingFromSnapshot,
            ..
        }
    ));
    reopened.detach("remote-0".into()).await.unwrap();
    assert_eq!(reopened.resident_active().await.unwrap(), 0);
    reopened
        .instance_exited(SessionEnd {
            session_id: first.clone(),
            detected_at: UInt(5000),
            reason: CloseReason::CoreExited,
        })
        .await
        .unwrap();
    assert_eq!(
        reopened.session(first).await.unwrap().ended_at,
        Some(UInt(5000))
    );
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn detach_preserves_an_unresolved_commit_and_pending_exit_failure() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store.clone()).await;
    let s = c.instance_started(new_session("remote")).await.unwrap();
    c.observe(frame("remote", 1000, vec![sample("a", 10)]))
        .await
        .unwrap();
    store.commit_fault.store(2, Ordering::SeqCst);
    assert!(
        c.observe(frame("remote", 2000, vec![sample("a", 15)]))
            .await
            .is_err()
    );
    store.commit_fault.store(1, Ordering::SeqCst);
    assert!(c.detach("remote".into()).await.is_err());
    assert_eq!(c.resident_active().await.unwrap(), 2);
    assert!(c.subscribe_status().borrow().is_some());
    c.detach("remote".into()).await.unwrap();
    assert_eq!(c.resident_active().await.unwrap(), 0);
    assert_eq!(
        c.session(s.id.clone())
            .await
            .unwrap()
            .attributed_bytes
            .upload,
        UInt(15)
    );
    store.end_fail.store(true, Ordering::SeqCst);
    assert!(
        c.instance_exited(SessionEnd {
            session_id: s.id,
            detected_at: UInt(3000),
            reason: CloseReason::CoreExited
        })
        .await
        .is_err()
    );
    c.detach("remote".into()).await.unwrap();
    assert!(c.subscribe_status().borrow().is_some());
    store.end_fail.store(false, Ordering::SeqCst);
    c.shutdown().await.unwrap();
}
#[tokio::test]
async fn binding_source_is_worker_local_and_cannot_revive_an_exited_session() {
    let store = Arc::new(FaultStore::default());
    let (default_sender, default_receiver) = tokio::sync::mpsc::channel(1);
    let default_source = Arc::new(FrameSource {
        receiver: tokio::sync::Mutex::new(Some(default_receiver)),
        polls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        opened: Notify::new(),
    });
    let (bound_sender, bound_receiver) = tokio::sync::mpsc::channel(1);
    let bound_source = Arc::new(FrameSource {
        receiver: tokio::sync::Mutex::new(Some(bound_receiver)),
        polls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        opened: Notify::new(),
    });
    let c = TrafficClient::start(TrafficActorArgs {
        host: HostId("contract-host".into()),
        source: default_source.clone(),
        store,
        clock: Arc::new(FakeClock::new(0, 0)),
        cancellation: CancellationToken::new(),
    })
    .await
    .unwrap();
    let s = c.instance_started(new_session("instance")).await.unwrap();
    let binding = SourceBinding {
        instance_id: "instance".into(),
        endpoint: SourceEndpoint::Http("http://unused".into()),
        secret: None,
    };
    c.controller_bound_with_source(binding.clone(), bound_source.clone())
        .await
        .unwrap();
    bound_source.opened.notified().await;
    assert!(default_source.receiver.lock().await.is_some());
    c.detach("instance".into()).await.unwrap();
    assert!(bound_sender.is_closed());
    c.controller_bound(binding.clone()).await.unwrap();
    default_source.opened.notified().await;
    assert!(!default_sender.is_closed());
    c.instance_exited(SessionEnd {
        session_id: s.id.clone(),
        detected_at: UInt(2000),
        reason: CloseReason::CoreExited,
    })
    .await
    .unwrap();
    assert!(default_sender.is_closed());
    let before_late_bind = c.session(s.id.clone()).await.unwrap();
    assert!(matches!(
        c.controller_bound(binding.clone()).await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        c.controller_bound_with_source(binding, bound_source).await,
        Err(StoreError::Conflict(_))
    ));
    let ended = c.session(s.id).await.unwrap();
    assert_eq!(ended.ended_at, Some(UInt(2000)));
    assert_eq!(ended.quality, before_late_bind.quality);
    assert_eq!(ended.freshness, before_late_bind.freshness);
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn pending_exit_rejects_a_late_controller_binding() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store.clone()).await;
    let s = c.instance_started(new_session("instance")).await.unwrap();
    store.end_fail.store(true, Ordering::SeqCst);
    assert!(
        c.instance_exited(SessionEnd {
            session_id: s.id,
            detected_at: UInt(2000),
            reason: CloseReason::CoreExited,
        })
        .await
        .is_err()
    );
    assert!(matches!(
        c.controller_bound(SourceBinding {
            instance_id: "instance".into(),
            endpoint: SourceEndpoint::Http("http://unused".into()),
            secret: None,
        })
        .await,
        Err(StoreError::Conflict(_))
    ));
    store.end_fail.store(false, Ordering::SeqCst);
    c.shutdown().await.unwrap();
}
#[tokio::test]
async fn detached_failed_start_retry_does_not_restart_collection() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store.clone()).await;
    store.begin_fail.store(true, Ordering::SeqCst);
    c.notify_instance_started(
        new_session("remote"),
        Some(SourceBinding {
            instance_id: "remote".into(),
            endpoint: SourceEndpoint::Http("http://unused".into()),
            secret: None,
        }),
    )
    .unwrap();
    c.detach("remote".into()).await.unwrap();
    store.begin_fail.store(false, Ordering::SeqCst);
    let s = c.instance_started(new_session("remote")).await.unwrap();
    // If the pending binding restarted a worker, this old generation would be rejected.
    c.observe(frame("remote", 1000, vec![sample("a", 10)]))
        .await
        .unwrap();
    assert!(c.session(s.id).await.unwrap().ended_at.is_none());
    c.shutdown().await.unwrap();
}
#[tokio::test]
async fn remote_detach_joins_worker_and_reconnect_preserves_live_process_baseline() {
    let store = Arc::new(FaultStore::default());
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    let source = Arc::new(FrameSource {
        receiver: tokio::sync::Mutex::new(Some(receiver)),
        polls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        opened: Notify::new(),
    });
    let cancellation = CancellationToken::new();
    let c = TrafficClient::start(TrafficActorArgs {
        host: HostId("contract-host".into()),
        source: source.clone(),
        store: store.clone(),
        clock: Arc::new(FakeClock::new(0, 0)),
        cancellation: cancellation.clone(),
    })
    .await
    .unwrap();
    let s = c.instance_started(new_session("remote")).await.unwrap();
    c.observe(frame("remote", 1000, vec![sample("a", 10)]))
        .await
        .unwrap();
    let binding = SourceBinding {
        instance_id: "remote".into(),
        endpoint: SourceEndpoint::Http("http://unused".into()),
        secret: None,
    };
    c.controller_bound(binding.clone()).await.unwrap();
    source.opened.notified().await;
    // Detach must join the worker and release its stream, rather than only change a label.
    c.detach("remote".into()).await.unwrap();
    assert!(sender.is_closed());
    let detached = c.session(s.id.clone()).await.unwrap();
    assert!(detached.ended_at.is_none());
    assert_eq!(detached.freshness, Freshness::Stale);
    assert!(detached.quality.contains(&Quality::LifecycleUnknown));
    assert!(matches!(
        c.observe(frame("remote", 2000, vec![sample("a", 99)]))
            .await,
        Err(StoreError::Conflict(_))
    ));
    let (_next_sender, next_receiver) = tokio::sync::mpsc::channel(2);
    *source.receiver.lock().await = Some(next_receiver);
    assert_eq!(
        c.instance_started(new_session("remote")).await.unwrap().id,
        s.id
    );
    c.controller_bound(binding).await.unwrap();
    let mut resumed = frame("remote", 3000, vec![sample("a", 15)]);
    resumed.generation = UInt(3);
    c.observe(resumed).await.unwrap();
    let record = c.session(s.id.clone()).await.unwrap();
    assert_eq!(record.attributed_bytes.upload, UInt(15));
    assert_eq!(record.observed_connections, UInt(1));
    assert!(record.ended_at.is_none());
    cancellation.cancel();
    c.shutdown().await.unwrap();
    // Closing the application supplies no evidence that this remote process exited.
    assert!(
        store
            .inner
            .session(s.id.clone())
            .await
            .unwrap()
            .ended_at
            .is_none()
    );
    let (reopened, _) = client(store.clone()).await;
    let recovered = reopened.session(s.id).await.unwrap();
    assert_eq!(recovered.attributed_bytes.upload, UInt(15));
    assert!(recovered.quality.contains(&Quality::LifecycleUnknown));
    assert!(recovered.ended_at.is_none());
    reopened.shutdown().await.unwrap();
}
#[async_trait::async_trait]
impl TrafficSource for FrameSource {
    async fn connect(
        &self,
        _: SourceBinding,
        _: UInt,
        _: Arc<dyn Clock>,
    ) -> TrafficResult<ObservationStream> {
        let receiver = self
            .receiver
            .lock()
            .await
            .take()
            .ok_or_else(|| StoreError::Unavailable("source already opened".into()))?;
        let polls = self.polls.clone();
        self.opened.notify_one();
        Ok(Box::pin(futures_util::stream::unfold(
            receiver,
            move |mut receiver| {
                polls.fetch_add(1, Ordering::SeqCst);
                async move { receiver.recv().await.map(|frame| (Ok(frame), receiver)) }
            },
        )))
    }
}
#[tokio::test]
async fn actual_worker_waits_for_commit_ack_and_shutdown_joins() {
    let store = Arc::new(FaultStore::default());
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    let polls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cancellation = CancellationToken::new();
    let c = TrafficClient::start(TrafficActorArgs {
        host: HostId("contract-host".into()),
        source: Arc::new(FrameSource {
            receiver: tokio::sync::Mutex::new(Some(receiver)),
            polls: polls.clone(),
            opened: Notify::new(),
        }),
        store: store.clone(),
        clock: Arc::new(FakeClock::new(0, 0)),
        cancellation: cancellation.clone(),
    })
    .await
    .unwrap();
    let s = c.instance_started(new_session("instance")).await.unwrap();
    c.set_current_instance(Some("instance".into()))
        .await
        .unwrap();
    let detail_subscription = c.subscribe_details();
    drop(detail_subscription);
    store.pause.store(true, Ordering::SeqCst);
    c.controller_bound(SourceBinding {
        instance_id: "instance".into(),
        endpoint: SourceEndpoint::Http("http://unused".into()),
        secret: None,
    })
    .await
    .unwrap();
    let mut a = frame("instance", 1000, vec![sample("a", 10)]);
    a.generation = UInt(1);
    let mut b = frame("instance", 2000, vec![sample("a", 15)]);
    b.generation = UInt(1);
    sender.send(a).await.unwrap();
    sender.send(b).await.unwrap();
    store.entered.notified().await;
    assert_eq!(
        polls.load(Ordering::SeqCst),
        1,
        "second frame must not be polled before committed acknowledgement"
    );
    let mut summary = c.subscribe_summary();
    store.release.notify_one();
    loop {
        if summary
            .borrow_and_update()
            .as_ref()
            .is_some_and(|s| s.revision == UInt(2))
        {
            break;
        }
        summary.changed().await.unwrap();
    }
    assert_eq!(
        store
            .inner
            .session(s.id)
            .await
            .unwrap()
            .attributed_bytes
            .upload,
        UInt(15)
    );
    assert!(
        c.subscribe_details().borrow().is_none(),
        "no hidden details receiver should force cloning"
    );
    cancellation.cancel();
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn root_cancel_then_ordered_exit_is_sealed_before_shutdown_returns() {
    let store = Arc::new(FaultStore::default());
    let (c, root) = client(store.clone()).await;
    let s = c.instance_started(new_session("instance")).await.unwrap();
    c.observe(frame("instance", 1000, vec![sample("a", 10)]))
        .await
        .unwrap();
    root.cancel();
    c.notify_instance_exited("instance".into(), 2000).unwrap();
    assert!(
        c.observe(frame("instance", 3000, vec![sample("a", 20)]))
            .await
            .is_err()
    );
    c.shutdown().await.unwrap();
    assert_eq!(
        store.inner.session(s.id.clone()).await.unwrap().ended_at,
        Some(UInt(2000))
    );
    assert!(
        store
            .inner
            .recover(HostId("contract-host".into()))
            .await
            .unwrap()
            .sessions
            .is_empty()
    );
    let record = store
        .inner
        .connection(s.id, "a".into())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        record.status,
        ConnectionStatus::Closed {
            reason: CloseReason::CoreExited,
            ..
        }
    ));
}
#[tokio::test]
async fn zero_byte_connections_and_path_changes_are_visible_without_invented_bytes() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store).await;
    let s = c.instance_started(new_session("instance")).await.unwrap();
    c.observe(frame("instance", 1000, vec![sample("a", 0)]))
        .await
        .unwrap();
    let q = TopologyQuery {
        session_id: s.id.clone(),
        filter: ConnectionFilter::default(),
        scope: QueryScope::Live,
        limit: 100,
    };
    let first = c.query_topology(q.clone()).await.unwrap();
    assert_eq!(first.paths.len(), 1);
    assert_eq!(first.paths[0].bytes, Bytes::default());
    assert!(first.paths[0].current_rate.is_none());
    let mut changed = sample("a", 0);
    changed.chains = vec!["EXIT".into(), "new".into()];
    c.observe(frame("instance", 2000, vec![changed]))
        .await
        .unwrap();
    let live = c.query_topology(q).await.unwrap();
    assert!(
        live.paths
            .iter()
            .any(|p| p.dimensions.path == vec!["EXIT", "new"])
    );
    assert_eq!(
        c.session(s.id).await.unwrap().attributed_bytes,
        Bytes::default()
    );
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn owner_rejects_foreign_host_and_prunes_many_ended_drains() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store.clone()).await;
    let mut foreign = new_session("foreign");
    foreign.host = HostId("other-host".into());
    assert!(matches!(
        c.instance_started(foreign.clone()).await,
        Err(StoreError::InvalidData(_))
    ));
    assert!(store.inner.session(foreign.id()).await.is_err());
    let a = c.instance_started(new_session("a")).await.unwrap();
    let b = c.instance_started(new_session("b")).await.unwrap();
    let d = c.instance_started(new_session("c")).await.unwrap();
    c.set_current_instance(Some("c".into())).await.unwrap();
    for (s, t) in [(a.clone(), 1000), (b.clone(), 2000), (d.clone(), 3000)] {
        c.instance_exited(SessionEnd {
            session_id: s.id,
            detected_at: UInt(t),
            reason: CloseReason::CoreExited,
        })
        .await
        .unwrap();
    }
    assert!(store.inner.session(a.id).await.is_err());
    assert!(store.inner.session(b.id).await.is_err());
    assert_eq!(c.current_session().await.unwrap().unwrap().id, d.id);
    c.shutdown().await.unwrap();
}

#[tokio::test]
async fn status_clears_only_recovered_instance_errors() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store).await;
    c.instance_started(new_session("a")).await.unwrap();
    c.instance_started(new_session("b")).await.unwrap();
    c.observe(frame("a", 1000, vec![sample("a", 10)]))
        .await
        .unwrap();
    c.observe(frame("b", 1000, vec![sample("b", 10)]))
        .await
        .unwrap();
    c.source_disconnected("a".into(), UInt(0)).await.unwrap();
    c.source_disconnected("b".into(), UInt(0)).await.unwrap();
    assert!(c.subscribe_status().borrow().is_some());
    c.observe(frame("a", 2000, vec![sample("a", 15)]))
        .await
        .unwrap();
    assert!(
        c.subscribe_status().borrow().is_some(),
        "another instance remains degraded"
    );
    c.observe(frame("b", 2000, vec![sample("b", 15)]))
        .await
        .unwrap();
    assert!(c.subscribe_status().borrow().is_none());
    assert!(c.source_disconnected("b".into(), UInt(99)).await.is_err());
    assert!(
        c.subscribe_status().borrow().is_none(),
        "old source errors cannot contaminate current state"
    );
    c.shutdown().await.unwrap();
}
#[tokio::test]
async fn exit_keeps_terminal_event_when_uncertain_commit_retry_fails() {
    let store = Arc::new(FaultStore::default());
    let (c, _) = client(store.clone()).await;
    let s = c.instance_started(new_session("instance")).await.unwrap();
    store.commit_fault.store(3, Ordering::SeqCst);
    assert!(
        c.observe(frame("instance", 1000, vec![sample("a", 10)]))
            .await
            .is_err()
    );
    store.commit_fault.store(2, Ordering::SeqCst);
    assert!(
        c.instance_exited(SessionEnd {
            session_id: s.id.clone(),
            detected_at: UInt(2000),
            reason: CloseReason::CoreExited
        })
        .await
        .is_err()
    );
    assert!(c.subscribe_status().borrow().is_some());
    let ended = c.session(s.id.clone()).await.unwrap();
    assert_eq!(ended.ended_at, Some(UInt(2000)));
    assert_eq!(ended.attributed_bytes.upload, UInt(10));
    assert!(c.subscribe_status().borrow().is_none());
    c.shutdown().await.unwrap();
    let row = store
        .inner
        .connection(s.id, "a".into())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        row.status,
        ConnectionStatus::Closed {
            reason: CloseReason::CoreExited,
            ..
        }
    ));
}
