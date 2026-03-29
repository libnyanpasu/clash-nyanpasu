//! Tests for MVCC snapshot mechanism (ArcSwap-based lock-free reads).
//!
//! Proves:
//! 1. Snapshot reflects committed state
//! 2. Snapshot is None before first commit
//! 3. Snapshot updates on each commit
//! 4. Snapshot NOT updated on migration failure
//! 5. Snapshot NOT updated on effect failure (with_pending_state)
//! 6. Snapshot updated on effect success
//! 7. Multiple handles see the same snapshot
//! 8. Concurrent fan-in: no deadlock
//! 9. Concurrent fan-in: eventual convergence (Read Committed)
//! 10. Three-source concurrent fan-in: no deadlock
//! 11. Snapshot during subscriber reflects pre-commit value

use crate::state::{error::*, *};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::RwLock;

// ─── Helpers ──────────────────────────────────────────────────

struct FailSubscriber {
    name: String,
    should_fail: AtomicBool,
}

impl FailSubscriber {
    fn always_fail(name: &str) -> Self {
        Self {
            name: name.to_string(),
            should_fail: AtomicBool::new(true),
        }
    }
}

impl FusedStateChangedSubscriber for FailSubscriber {}

#[async_trait::async_trait]
impl<T: Clone + Send + Sync + 'static> StateChangedSubscriber<T> for FailSubscriber {
    fn name(&self) -> &str {
        &self.name
    }
    async fn migrate(&self, _prev: Option<T>, _new: T) -> Result<(), anyhow::Error> {
        if self.should_fail.load(Ordering::SeqCst) {
            Err(anyhow::anyhow!("forced migration failure"))
        } else {
            Ok(())
        }
    }
}

/// Subscriber that captures the snapshot value during migration.
struct SnapshotCapture {
    name: String,
    handle: StateSnapshot<i32>,
    captured: std::sync::Mutex<Option<Option<Arc<i32>>>>,
}

impl FusedStateChangedSubscriber for SnapshotCapture {}

#[async_trait::async_trait]
impl StateChangedSubscriber<i32> for SnapshotCapture {
    fn name(&self) -> &str {
        &self.name
    }
    async fn migrate(&self, _prev: Option<i32>, _new: i32) -> Result<(), anyhow::Error> {
        let snap = self.handle.load();
        *self.captured.lock().unwrap() = Some(snap);
        Ok(())
    }
}

// ─── 5.1 Snapshot basic behavior ──────────────────────────────

#[tokio::test]
async fn test_snapshot_reflects_committed_state() {
    let mut coord = StateCoordinator::<i32>::new();
    let handle = coord.snapshot_handle();
    coord.upsert_state(42).await.unwrap();
    assert_eq!(handle.load().as_deref(), Some(&42));
}

#[tokio::test]
async fn test_snapshot_is_none_before_first_commit() {
    let coord = StateCoordinator::<i32>::new();
    let handle = coord.snapshot_handle();
    assert_eq!(handle.load(), None);
}

#[tokio::test]
async fn test_snapshot_updates_on_each_commit() {
    let mut coord = StateCoordinator::<i32>::new();
    let handle = coord.snapshot_handle();
    for i in 1..=3 {
        coord.upsert_state(i).await.unwrap();
        assert_eq!(handle.load().as_deref(), Some(&i));
    }
}

// ─── 5.2 Migration failure does not update snapshot ───────────

#[tokio::test]
async fn test_snapshot_not_updated_on_migration_failure() {
    let mut coord = StateCoordinator::<i32>::new();
    coord.add_subscriber(Box::new(FailSubscriber::always_fail("blocker")));
    let handle = coord.snapshot_handle();

    let result = coord.upsert_state(42).await;
    assert!(result.is_err());
    assert_eq!(handle.load(), None);
}

// ─── 5.3 Effect failure does not update snapshot ──────────────

#[tokio::test]
async fn test_snapshot_not_updated_on_effect_failure() {
    let mut coord = StateCoordinator::<i32>::new();
    let handle = coord.snapshot_handle();

    coord.upsert_state(1).await.unwrap();
    assert_eq!(handle.load().as_deref(), Some(&1));

    let result: Result<(), WithEffectError<anyhow::Error>> = coord
        .with_pending_state(&2, |_s| async { Err(anyhow::anyhow!("effect failed")) })
        .await;
    assert!(result.is_err());
    assert_eq!(handle.load().as_deref(), Some(&1));
}

// ─── 5.4 Effect success updates snapshot ──────────────────────

#[tokio::test]
async fn test_snapshot_updated_on_effect_success() {
    let mut coord = StateCoordinator::<i32>::new();
    let handle = coord.snapshot_handle();

    let result: Result<(), WithEffectError<anyhow::Error>> = coord
        .with_pending_state(&42, |_s| async { Ok(()) })
        .await;
    assert!(result.is_ok());
    assert_eq!(handle.load().as_deref(), Some(&42));
}

// ─── 5.5 Multiple handles see the same snapshot ───────────────

#[tokio::test]
async fn test_multiple_handles_see_same_snapshot() {
    let mut coord = StateCoordinator::<i32>::new();
    let h1 = coord.snapshot_handle();
    let h2 = coord.snapshot_handle();

    coord.upsert_state(42).await.unwrap();
    assert_eq!(h1.load().as_deref(), Some(&42));
    assert_eq!(h2.load().as_deref(), Some(&42));
}

// ─── 5.6 Concurrent fan-in: no deadlock ──────────────────────

/// Fan-in subscriber: reads a sibling snapshot and writes to derived state.
struct FanInSubscriber<S: Clone + Send + Sync + 'static, D: Clone + Send + Sync + 'static> {
    name: String,
    sibling_snapshot: StateSnapshot<S>,
    derived: Arc<RwLock<SimpleStateManager<D>>>,
    combiner: Box<dyn Fn(S, S) -> D + Send + Sync>,
}

impl<S: Clone + Send + Sync + 'static, D: Clone + Send + Sync + 'static>
    FusedStateChangedSubscriber for FanInSubscriber<S, D>
{
}

#[async_trait::async_trait]
impl<S: Clone + Send + Sync + 'static, D: Clone + Send + Sync + 'static>
    StateChangedSubscriber<S> for FanInSubscriber<S, D>
{
    fn name(&self) -> &str {
        &self.name
    }

    async fn migrate(&self, _prev: Option<S>, new_state: S) -> Result<(), anyhow::Error> {
        if let Some(sibling) = self.sibling_snapshot.load() {
            let derived = (self.combiner)(new_state, (*sibling).clone());
            self.derived.write().await.upsert(derived).await?;
        }
        // If sibling hasn't committed yet, skip — the sibling's own write will
        // trigger a later migration that converges to the correct value.
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_fan_in_no_deadlock() {
    // Source A, Source B → Derived D = (A, B)
    let mut coord_a = StateCoordinator::<i32>::new();
    let mut coord_b = StateCoordinator::<i32>::new();
    let coord_d = StateCoordinator::<(i32, i32)>::new();

    let snap_a = coord_a.snapshot_handle();
    let snap_b = coord_b.snapshot_handle();

    let mgr_d = Arc::new(RwLock::new(SimpleStateManager::new(coord_d)));
    let snap_d = mgr_d.read().await.snapshot_handle();

    // Register fan-in subscribers
    coord_a.add_subscriber(Box::new(FanInSubscriber {
        name: "D_from_A".to_string(),
        sibling_snapshot: snap_b.clone(),
        derived: Arc::clone(&mgr_d),
        combiner: Box::new(|a, b| (a, b)),
    }));

    coord_b.add_subscriber(Box::new(FanInSubscriber {
        name: "D_from_B".to_string(),
        sibling_snapshot: snap_a.clone(),
        derived: Arc::clone(&mgr_d),
        combiner: Box::new(|b, a| (a, b)),
    }));

    let mgr_a = Arc::new(RwLock::new(SimpleStateManager::new(coord_a)));
    let mgr_b = Arc::new(RwLock::new(SimpleStateManager::new(coord_b)));

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

    // Both sources committed
    assert_eq!(snap_a.load().as_deref(), Some(&10));
    assert_eq!(snap_b.load().as_deref(), Some(&20));

    // Force convergence: re-trigger B so its subscriber reads committed A snapshot
    mgr_b.write().await.upsert(20).await.unwrap();
    let d_state = snap_d.load().expect("D should converge");
    assert_eq!(*d_state, (10, 20));
}

// ─── 5.7 Concurrent fan-in eventual convergence ──────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_concurrent_fan_in_eventual_convergence() {
    let mut coord_a = StateCoordinator::<i32>::new();
    let mut coord_b = StateCoordinator::<i32>::new();
    let coord_d = StateCoordinator::<(i32, i32)>::new();

    let snap_a = coord_a.snapshot_handle();
    let snap_b = coord_b.snapshot_handle();

    let mgr_d = Arc::new(RwLock::new(SimpleStateManager::new(coord_d)));
    let snap_d = mgr_d.read().await.snapshot_handle();

    coord_a.add_subscriber(Box::new(FanInSubscriber {
        name: "D_from_A".to_string(),
        sibling_snapshot: snap_b.clone(),
        derived: Arc::clone(&mgr_d),
        combiner: Box::new(|a, b| (a, b)),
    }));

    coord_b.add_subscriber(Box::new(FanInSubscriber {
        name: "D_from_B".to_string(),
        sibling_snapshot: snap_a.clone(),
        derived: Arc::clone(&mgr_d),
        combiner: Box::new(|b, a| (a, b)),
    }));

    let mgr_a = Arc::new(RwLock::new(SimpleStateManager::new(coord_a)));
    let mgr_b = Arc::new(RwLock::new(SimpleStateManager::new(coord_b)));

    // Phase 1: A commits first. B hasn't committed yet, so A's subscriber
    // skips (no sibling snapshot available). D is still None.
    mgr_a.write().await.upsert(10).await.unwrap();
    assert_eq!(snap_a.load().as_deref(), Some(&10));
    assert!(snap_d.load().is_none(), "D should be None — B not yet committed");

    // Phase 2: B commits. A's snapshot is now 10, so B's subscriber
    // derives D = (10, 20) — convergence achieved.
    mgr_b.write().await.upsert(20).await.unwrap();
    let d_final = snap_d.load().expect("D should converge after B upsert");
    assert_eq!(*d_final, (10, 20));
}

// ─── 5.8 Three-source concurrent fan-in ──────────────────────

struct TriFanInSubscriber {
    name: String,
    snap_a: StateSnapshot<i32>,
    snap_b: StateSnapshot<i32>,
    snap_c: StateSnapshot<i32>,
    derived: Arc<RwLock<SimpleStateManager<(i32, i32, i32)>>>,
    /// Which source this subscriber is for: 'a', 'b', or 'c'
    source: char,
}

impl FusedStateChangedSubscriber for TriFanInSubscriber {}

#[async_trait::async_trait]
impl StateChangedSubscriber<i32> for TriFanInSubscriber {
    fn name(&self) -> &str {
        &self.name
    }

    async fn migrate(&self, _prev: Option<i32>, new_state: i32) -> Result<(), anyhow::Error> {
        let a = if self.source == 'a' {
            new_state
        } else {
            self.snap_a.load().map(|v| *v).unwrap_or(0)
        };
        let b = if self.source == 'b' {
            new_state
        } else {
            self.snap_b.load().map(|v| *v).unwrap_or(0)
        };
        let c = if self.source == 'c' {
            new_state
        } else {
            self.snap_c.load().map(|v| *v).unwrap_or(0)
        };
        self.derived.write().await.upsert((a, b, c)).await?;
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_three_source_concurrent_fan_in() {
    let mut coord_a = StateCoordinator::<i32>::new();
    let mut coord_b = StateCoordinator::<i32>::new();
    let mut coord_c = StateCoordinator::<i32>::new();
    let coord_d = StateCoordinator::<(i32, i32, i32)>::new();

    let snap_a = coord_a.snapshot_handle();
    let snap_b = coord_b.snapshot_handle();
    let snap_c = coord_c.snapshot_handle();

    let mgr_d = Arc::new(RwLock::new(SimpleStateManager::new(coord_d)));
    let snap_d = mgr_d.read().await.snapshot_handle();

    coord_a.add_subscriber(Box::new(TriFanInSubscriber {
        name: "D_from_A".to_string(),
        snap_a: snap_a.clone(),
        snap_b: snap_b.clone(),
        snap_c: snap_c.clone(),
        derived: Arc::clone(&mgr_d),
        source: 'a',
    }));

    coord_b.add_subscriber(Box::new(TriFanInSubscriber {
        name: "D_from_B".to_string(),
        snap_a: snap_a.clone(),
        snap_b: snap_b.clone(),
        snap_c: snap_c.clone(),
        derived: Arc::clone(&mgr_d),
        source: 'b',
    }));

    coord_c.add_subscriber(Box::new(TriFanInSubscriber {
        name: "D_from_C".to_string(),
        snap_a: snap_a.clone(),
        snap_b: snap_b.clone(),
        snap_c: snap_c.clone(),
        derived: Arc::clone(&mgr_d),
        source: 'c',
    }));

    let mgr_a = Arc::new(RwLock::new(SimpleStateManager::new(coord_a)));
    let mgr_b = Arc::new(RwLock::new(SimpleStateManager::new(coord_b)));
    let mgr_c = Arc::new(RwLock::new(SimpleStateManager::new(coord_c)));

    // Concurrent upserts prove no deadlock
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

    // All sources have committed
    assert_eq!(snap_a.load().as_deref(), Some(&1));
    assert_eq!(snap_b.load().as_deref(), Some(&2));
    assert_eq!(snap_c.load().as_deref(), Some(&3));

    // D has some value (no panic during fan-in)
    let _d_intermediate = snap_d.load();

    // With concurrent writes, D may have a stale view (Read Committed semantics).
    // Trigger one more write on C to force convergence — C's subscriber reads
    // committed snapshots of A and B.
    mgr_c.write().await.upsert(3).await.unwrap();
    let d_final = snap_d.load().expect("D should converge");
    assert_eq!(*d_final, (1, 2, 3));
}

// ─── 5.9 Snapshot during subscriber reflects pre-commit ──────

#[tokio::test]
async fn test_snapshot_during_subscriber_reflects_pre_commit() {
    let mut coord = StateCoordinator::<i32>::new();
    let handle = coord.snapshot_handle();

    let capture = Arc::new(SnapshotCapture {
        name: "capture".to_string(),
        handle: handle.clone(),
        captured: std::sync::Mutex::new(None),
    });

    coord.add_subscriber(Box::new(Arc::clone(&capture)));

    // First commit: snapshot is None during migration
    coord.upsert_state(42).await.unwrap();
    let captured_during_first = capture.captured.lock().unwrap().take();
    assert_eq!(
        captured_during_first,
        Some(None),
        "snapshot during first migration should be None (pre-commit)"
    );

    // After commit: snapshot is now 42
    assert_eq!(handle.load().as_deref(), Some(&42));

    // Second commit: snapshot during migration should be 42 (previous committed value)
    coord.upsert_state(100).await.unwrap();
    let captured_during_second = capture
        .captured
        .lock()
        .unwrap()
        .take()
        .map(|opt| opt.map(|arc| *arc));
    assert_eq!(
        captured_during_second,
        Some(Some(42)),
        "snapshot during second migration should be 42 (previous committed value)"
    );

    assert_eq!(handle.load().as_deref(), Some(&100));
}

// ─── 5.10 upsert_with_context updates snapshot ───────────────

#[tokio::test]
async fn test_upsert_with_context_updates_snapshot() {
    let mut coord = StateCoordinator::<i32>::new();
    let handle = coord.snapshot_handle();

    coord.upsert_state_with_context(42).await.unwrap();
    assert_eq!(handle.load().as_deref(), Some(&42));
}

// ─── 5.11 with_pending_state_in_context updates snapshot ─────

#[tokio::test]
async fn test_with_pending_state_in_context_updates_snapshot() {
    let mut coord = StateCoordinator::<i32>::new();
    let handle = coord.snapshot_handle();

    let result: Result<(), WithEffectError<anyhow::Error>> = coord
        .with_pending_state_in_context(&42, |_s| async { Ok(()) })
        .await;
    assert!(result.is_ok());
    assert_eq!(handle.load().as_deref(), Some(&42));
}

// ─── 5.12 Manager snapshot_handle works ──────────────────────

#[tokio::test]
async fn test_simple_manager_snapshot_handle() {
    let mut mgr = SimpleStateManager::new(StateCoordinator::<i32>::new());
    let handle = mgr.snapshot_handle();

    assert_eq!(handle.load(), None);
    mgr.upsert(42).await.unwrap();
    assert_eq!(handle.load().as_deref(), Some(&42));
}
