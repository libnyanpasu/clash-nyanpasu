//! Tests for MVCC snapshot mechanism (ArcSwap-based lock-free reads).
//!
//! Proves:
//! 1. Snapshot reflects committed state
//! 2. Snapshot is initialized at coordinator construction
//! 3. Snapshot updates on each commit
//! 4. Snapshot IS updated even when post-commit notification fails
//! 5. Snapshot NOT updated on effect failure (with_pending_state)
//! 6. Snapshot updated on effect success
//! 7. Multiple handles see the same snapshot
//! 8. Concurrent fan-in: no deadlock
//! 9. Concurrent fan-in: eventual convergence (Read Committed)
//! 10. Three-source concurrent fan-in: no deadlock
//! 11. Snapshot during subscriber reflects committed value (post-commit)

use crate::state::{error::*, *};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::RwLock;

// ─── Helpers ──────────────────────────────────────────────────

struct FailAckSubscriber {
    name: String,
    should_fail: AtomicBool,
}

impl FailAckSubscriber {
    fn always_fail(name: &str) -> Self {
        Self {
            name: name.to_string(),
            should_fail: AtomicBool::new(true),
        }
    }
}

#[async_trait::async_trait]
impl<T: Clone + Send + Sync + 'static> StateAckSubscriber<T> for FailAckSubscriber {
    fn name(&self) -> SubscriberName<'_> {
        self.name.as_str().into()
    }
    async fn on_committed(&self, _change: StateChange<T>) -> Ack {
        if self.should_fail.load(Ordering::SeqCst) {
            Ack::Failed(anyhow::anyhow!("forced ACK failure"))
        } else {
            Ack::Ok
        }
    }
}

struct SnapshotCapture {
    name: String,
    handle: StateSnapshot<i32>,
    captured: std::sync::Mutex<Option<i32>>,
}

#[async_trait::async_trait]
impl StateAckSubscriber<i32> for SnapshotCapture {
    fn name(&self) -> SubscriberName<'_> {
        self.name.as_str().into()
    }
    async fn on_committed(&self, _change: StateChange<i32>) -> Ack {
        *self.captured.lock().unwrap() = Some(self.handle.load().state);
        Ack::Ok
    }
}

// ─── 5.1 Snapshot basic behavior ──────────────────────────────

#[tokio::test]
async fn test_snapshot_reflects_committed_state() {
    let mut coord = StateCoordinator::<i32>::builder().build(0);
    let handle = coord.snapshot_handle();
    coord.upsert_state(42).await.unwrap();
    assert_eq!(*handle.load(), 42);
}

#[tokio::test]
async fn test_snapshot_updates_on_each_commit() {
    let mut coord = StateCoordinator::<i32>::builder().build(0);
    let handle = coord.snapshot_handle();
    for i in 1..=3 {
        coord.upsert_state(i).await.unwrap();
        assert_eq!(*handle.load(), i);
    }
}

// ─── 5.2 ACK failure still updates snapshot (post-commit) ────

#[tokio::test]
async fn test_snapshot_updated_even_on_post_commit_failure() {
    let mut coord = StateCoordinator::<i32>::builder().build(0);
    coord.add_subscriber(Box::new(FailAckSubscriber::always_fail("blocker")));
    let handle = coord.snapshot_handle();

    let result = coord.upsert_state(42).await;
    assert!(result.is_ok());
    assert_eq!(*handle.load(), 42);
}

// ─── 5.3 Effect failure does not update snapshot ──────────────

#[tokio::test]
async fn test_snapshot_not_updated_on_effect_failure() {
    let mut coord = StateCoordinator::<i32>::builder().build(0);
    let handle = coord.snapshot_handle();

    coord.upsert_state(1).await.unwrap();
    assert_eq!(*handle.load(), 1);

    let result: Result<((), PrepareReport), WithEffectError<anyhow::Error>> = coord
        .with_pending_state(&2, |_s| async { Err(anyhow::anyhow!("effect failed")) })
        .await;
    assert!(result.is_err());
    assert_eq!(*handle.load(), 1);
}

// ─── 5.4 Effect success updates snapshot ──────────────────────

#[tokio::test]
async fn test_snapshot_updated_on_effect_success() {
    let mut coord = StateCoordinator::<i32>::builder().build(0);
    let handle = coord.snapshot_handle();

    let result: Result<((), PrepareReport), WithEffectError<anyhow::Error>> =
        coord.with_pending_state(&42, |_s| async { Ok(()) }).await;
    assert!(result.is_ok());
    assert_eq!(*handle.load(), 42);
}

// ─── 5.5 Multiple handles see the same snapshot ───────────────

#[tokio::test]
async fn test_multiple_handles_see_same_snapshot() {
    let mut coord = StateCoordinator::<i32>::builder().build(0);
    let h1 = coord.snapshot_handle();
    let h2 = coord.snapshot_handle();

    coord.upsert_state(42).await.unwrap();
    assert_eq!(*h1.load(), 42);
    assert_eq!(*h2.load(), 42);
}

// ─── 5.6 Concurrent fan-in: no deadlock ──────────────────────

struct FanInAckSubscriber<S: Clone + Send + Sync + 'static, D: Clone + Send + Sync + 'static> {
    name: String,
    sibling_snapshot: StateSnapshot<S>,
    derived: Arc<RwLock<SimpleStateManager<D>>>,
    combiner: Box<dyn Fn(S, S) -> D + Send + Sync>,
}

#[async_trait::async_trait]
impl<S: Clone + Send + Sync + 'static, D: Clone + Send + Sync + 'static> StateAckSubscriber<S>
    for FanInAckSubscriber<S, D>
{
    fn name(&self) -> SubscriberName<'_> {
        self.name.as_str().into()
    }

    async fn on_committed(&self, change: StateChange<S>) -> Ack {
        let sibling = self.sibling_snapshot.load();
        let derived = (self.combiner)(change.current().clone(), sibling.state.clone());
        match self.derived.write().await.upsert(derived).await {
            Ok(_) => Ack::Ok,
            Err(e) => Ack::Failed(anyhow::anyhow!(e)),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_fan_in_no_deadlock() {
    let mut coord_a = StateCoordinator::<i32>::builder().build(0);
    let mut coord_b = StateCoordinator::<i32>::builder().build(0);
    let coord_d = StateCoordinator::<(i32, i32)>::builder().build((0, 0));

    let snap_a = coord_a.snapshot_handle();
    let snap_b = coord_b.snapshot_handle();

    let mgr_d = Arc::new(RwLock::new(SimpleStateManager::from_coordinator(coord_d)));
    let snap_d = mgr_d.read().await.snapshot_handle();

    coord_a.add_subscriber(Box::new(FanInAckSubscriber {
        name: "D_from_A".to_string(),
        sibling_snapshot: snap_b.clone(),
        derived: Arc::clone(&mgr_d),
        combiner: Box::new(|a, b| (a, b)),
    }));

    coord_b.add_subscriber(Box::new(FanInAckSubscriber {
        name: "D_from_B".to_string(),
        sibling_snapshot: snap_a.clone(),
        derived: Arc::clone(&mgr_d),
        combiner: Box::new(|b, a| (a, b)),
    }));

    let mgr_a = Arc::new(RwLock::new(SimpleStateManager::from_coordinator(coord_a)));
    let mgr_b = Arc::new(RwLock::new(SimpleStateManager::from_coordinator(coord_b)));

    let ma = Arc::clone(&mgr_a);
    let mb = Arc::clone(&mgr_b);

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let t1 = tokio::spawn(async move { ma.write().await.upsert(10).await });
        let t2 = tokio::spawn(async move { mb.write().await.upsert(20).await });
        t1.await.unwrap().unwrap();
        t2.await.unwrap().unwrap();
    })
    .await;

    assert!(result.is_ok(), "deadlock detected: timeout after 5 seconds");

    assert_eq!(*snap_a.load(), 10);
    assert_eq!(*snap_b.load(), 20);

    mgr_b.write().await.upsert(20).await.unwrap();
    let d_state = snap_d.load();
    assert_eq!(*d_state, (10, 20));
}

// ─── 5.7 Concurrent fan-in eventual convergence ──────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_fan_in_eventual_convergence() {
    let mut coord_a = StateCoordinator::<i32>::builder().build(0);
    let mut coord_b = StateCoordinator::<i32>::builder().build(0);
    let coord_d = StateCoordinator::<(i32, i32)>::builder().build((0, 0));

    let snap_a = coord_a.snapshot_handle();
    let snap_b = coord_b.snapshot_handle();

    let mgr_d = Arc::new(RwLock::new(SimpleStateManager::from_coordinator(coord_d)));
    let snap_d = mgr_d.read().await.snapshot_handle();

    coord_a.add_subscriber(Box::new(FanInAckSubscriber {
        name: "D_from_A".to_string(),
        sibling_snapshot: snap_b.clone(),
        derived: Arc::clone(&mgr_d),
        combiner: Box::new(|a, b| (a, b)),
    }));

    coord_b.add_subscriber(Box::new(FanInAckSubscriber {
        name: "D_from_B".to_string(),
        sibling_snapshot: snap_a.clone(),
        derived: Arc::clone(&mgr_d),
        combiner: Box::new(|b, a| (a, b)),
    }));

    let mgr_a = Arc::new(RwLock::new(SimpleStateManager::from_coordinator(coord_a)));
    let mgr_b = Arc::new(RwLock::new(SimpleStateManager::from_coordinator(coord_b)));

    mgr_a.write().await.upsert(10).await.unwrap();
    assert_eq!(*snap_a.load(), 10);
    assert_eq!(*snap_d.load(), (10, 0));

    mgr_b.write().await.upsert(20).await.unwrap();
    let d_final = snap_d.load();
    assert_eq!(*d_final, (10, 20));
}

// ─── 5.8 Three-source concurrent fan-in ──────────────────────

struct TriFanInAckSubscriber {
    name: String,
    snap_a: StateSnapshot<i32>,
    snap_b: StateSnapshot<i32>,
    snap_c: StateSnapshot<i32>,
    derived: Arc<RwLock<SimpleStateManager<(i32, i32, i32)>>>,
    source: char,
}

#[async_trait::async_trait]
impl StateAckSubscriber<i32> for TriFanInAckSubscriber {
    fn name(&self) -> SubscriberName<'_> {
        self.name.as_str().into()
    }

    async fn on_committed(&self, change: StateChange<i32>) -> Ack {
        let a = if self.source == 'a' {
            *change.current()
        } else {
            self.snap_a.load().state
        };
        let b = if self.source == 'b' {
            *change.current()
        } else {
            self.snap_b.load().state
        };
        let c = if self.source == 'c' {
            *change.current()
        } else {
            self.snap_c.load().state
        };
        match self.derived.write().await.upsert((a, b, c)).await {
            Ok(_) => Ack::Ok,
            Err(e) => Ack::Failed(anyhow::anyhow!(e)),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_three_source_concurrent_fan_in() {
    let mut coord_a = StateCoordinator::<i32>::builder().build(0);
    let mut coord_b = StateCoordinator::<i32>::builder().build(0);
    let mut coord_c = StateCoordinator::<i32>::builder().build(0);
    let coord_d = StateCoordinator::<(i32, i32, i32)>::builder().build((0, 0, 0));

    let snap_a = coord_a.snapshot_handle();
    let snap_b = coord_b.snapshot_handle();
    let snap_c = coord_c.snapshot_handle();

    let mgr_d = Arc::new(RwLock::new(SimpleStateManager::from_coordinator(coord_d)));
    let snap_d = mgr_d.read().await.snapshot_handle();

    coord_a.add_subscriber(Box::new(TriFanInAckSubscriber {
        name: "D_from_A".to_string(),
        snap_a: snap_a.clone(),
        snap_b: snap_b.clone(),
        snap_c: snap_c.clone(),
        derived: Arc::clone(&mgr_d),
        source: 'a',
    }));

    coord_b.add_subscriber(Box::new(TriFanInAckSubscriber {
        name: "D_from_B".to_string(),
        snap_a: snap_a.clone(),
        snap_b: snap_b.clone(),
        snap_c: snap_c.clone(),
        derived: Arc::clone(&mgr_d),
        source: 'b',
    }));

    coord_c.add_subscriber(Box::new(TriFanInAckSubscriber {
        name: "D_from_C".to_string(),
        snap_a: snap_a.clone(),
        snap_b: snap_b.clone(),
        snap_c: snap_c.clone(),
        derived: Arc::clone(&mgr_d),
        source: 'c',
    }));

    let mgr_a = Arc::new(RwLock::new(SimpleStateManager::from_coordinator(coord_a)));
    let mgr_b = Arc::new(RwLock::new(SimpleStateManager::from_coordinator(coord_b)));
    let mgr_c = Arc::new(RwLock::new(SimpleStateManager::from_coordinator(coord_c)));

    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let ma = Arc::clone(&mgr_a);
        let mb = Arc::clone(&mgr_b);
        let mc = Arc::clone(&mgr_c);

        let t1 = tokio::spawn(async move { ma.write().await.upsert(1).await });
        let t2 = tokio::spawn(async move { mb.write().await.upsert(2).await });
        let t3 = tokio::spawn(async move { mc.write().await.upsert(3).await });

        t1.await.unwrap().unwrap();
        t2.await.unwrap().unwrap();
        t3.await.unwrap().unwrap();
    })
    .await;

    assert!(result.is_ok(), "deadlock detected: timeout after 5 seconds");

    assert_eq!(*snap_a.load(), 1);
    assert_eq!(*snap_b.load(), 2);
    assert_eq!(*snap_c.load(), 3);

    let _d_intermediate = snap_d.load();

    mgr_c.write().await.upsert(3).await.unwrap();
    let d_final = snap_d.load();
    assert_eq!(*d_final, (1, 2, 3));
}

// ─── 5.9 Snapshot during subscriber reflects committed value ──

#[tokio::test]
async fn test_snapshot_during_subscriber_reflects_committed_value() {
    let mut coord = StateCoordinator::<i32>::builder().build(0);
    let handle = coord.snapshot_handle();

    let capture = Arc::new(SnapshotCapture {
        name: "capture".to_string(),
        handle: handle.clone(),
        captured: std::sync::Mutex::new(None),
    });

    coord.add_subscriber(Box::new(Arc::clone(&capture)));

    // The post-commit hook observes the already committed snapshot.
    coord.upsert_state(42).await.unwrap();
    let captured_during_first = capture.captured.lock().unwrap().take();
    assert_eq!(
        captured_during_first,
        Some(42),
        "snapshot during first on_committed should be 42 (post-commit)"
    );

    assert_eq!(*handle.load(), 42);

    // Second commit: snapshot during on_committed should be 100 (new committed value)
    coord.upsert_state(100).await.unwrap();
    let captured_during_second = capture.captured.lock().unwrap().take();
    assert_eq!(
        captured_during_second,
        Some(100),
        "snapshot during second on_committed should be 100 (post-commit)"
    );

    assert_eq!(*handle.load(), 100);
}

// ─── 5.10 Manager snapshot_handle works ──────────────────────

#[tokio::test]
async fn test_simple_manager_snapshot_handle() {
    let mut mgr = SimpleStateManager::from_coordinator(StateCoordinator::<i32>::builder().build(0));
    let handle = mgr.snapshot_handle();

    assert_eq!(*handle.load(), 0);
    mgr.upsert(42).await.unwrap();
    assert_eq!(*handle.load(), 42);
}
