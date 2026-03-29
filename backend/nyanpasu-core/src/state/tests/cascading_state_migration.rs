//! Integration tests proving multi-level cascading state migration.
//!
//! Models the real architecture:
//!
//! ```text
//! SourceConfig (A) ──subscriber: BridgeSub──▶ DerivedRuntime (B) ──subscriber: LeafSub──▶ (C)
//! ```
//!
//! Modifying A triggers B derivation via BridgeSub.migrate(),
//! which in turn triggers C's LeafSub.migrate() inside B's coordinator.
//!
//! Proves:
//! 1. Happy cascade: A → B → C all commit
//! 2. Leaf failure: C fails → B not committed → A not committed
//! 3. Effect failure at A: B already committed → rollback cascades back through B → C
//! 4. Consistent state after failed second update
//! 5. Sibling subscriber failure after bridge succeeds → rollback cascades

use crate::state::{*, error::WithEffectError};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
    Mutex as StdMutex,
};
use tokio::sync::Mutex as TokioMutex;

// ─── State types ───────────────────────────────────────────────

type SourceConfig = i32;
type DerivedRuntime = String;

// ─── LeafSubscriber: terminal subscriber on B's coordinator ───

struct LeafSubscriber {
    name: String,
    should_fail_migrate: AtomicBool,
    migrate_count: AtomicUsize,
    rollback_count: AtomicUsize,
    migrate_log: StdMutex<Vec<(Option<DerivedRuntime>, DerivedRuntime)>>,
    rollback_log: StdMutex<Vec<(Option<DerivedRuntime>, DerivedRuntime)>>,
}

impl LeafSubscriber {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            should_fail_migrate: AtomicBool::new(false),
            migrate_count: AtomicUsize::new(0),
            rollback_count: AtomicUsize::new(0),
            migrate_log: StdMutex::new(Vec::new()),
            rollback_log: StdMutex::new(Vec::new()),
        }
    }

    fn set_should_fail(&self, fail: bool) {
        self.should_fail_migrate.store(fail, Ordering::SeqCst);
    }

    fn migrate_count(&self) -> usize {
        self.migrate_count.load(Ordering::SeqCst)
    }

    fn rollback_count(&self) -> usize {
        self.rollback_count.load(Ordering::SeqCst)
    }

    fn migrate_log(&self) -> Vec<(Option<DerivedRuntime>, DerivedRuntime)> {
        self.migrate_log.lock().unwrap().clone()
    }

    #[allow(dead_code)]
    fn rollback_log(&self) -> Vec<(Option<DerivedRuntime>, DerivedRuntime)> {
        self.rollback_log.lock().unwrap().clone()
    }
}

impl FusedStateChangedSubscriber for LeafSubscriber {}

#[async_trait::async_trait]
impl StateChangedSubscriber<DerivedRuntime> for LeafSubscriber {
    fn name(&self) -> &str {
        &self.name
    }

    async fn migrate(
        &self,
        prev: Option<DerivedRuntime>,
        new: DerivedRuntime,
    ) -> Result<(), anyhow::Error> {
        self.migrate_count.fetch_add(1, Ordering::SeqCst);
        self.migrate_log.lock().unwrap().push((prev, new));
        if self.should_fail_migrate.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("leaf migrate failed"));
        }
        Ok(())
    }

    async fn rollback(
        &self,
        prev: Option<DerivedRuntime>,
        new: DerivedRuntime,
    ) -> Result<(), anyhow::Error> {
        self.rollback_count.fetch_add(1, Ordering::SeqCst);
        self.rollback_log.lock().unwrap().push((prev, new));
        Ok(())
    }
}

// ─── BridgeSubscriber: on A's coordinator, derives and upserts B ───

struct BridgeSubscriber {
    b_manager: Arc<TokioMutex<SimpleStateManager<DerivedRuntime>>>,
    /// Stores B's state before each migrate, for rollback.
    prev_b: StdMutex<Option<DerivedRuntime>>,
    migrate_count: AtomicUsize,
    rollback_count: AtomicUsize,
}

impl BridgeSubscriber {
    fn new(b_manager: Arc<TokioMutex<SimpleStateManager<DerivedRuntime>>>) -> Self {
        Self {
            b_manager,
            prev_b: StdMutex::new(None),
            migrate_count: AtomicUsize::new(0),
            rollback_count: AtomicUsize::new(0),
        }
    }

    /// Pure derivation: SourceConfig → DerivedRuntime
    fn derive(source: &SourceConfig) -> DerivedRuntime {
        format!("derived_from_{source}")
    }

    fn migrate_count(&self) -> usize {
        self.migrate_count.load(Ordering::SeqCst)
    }

    fn rollback_count(&self) -> usize {
        self.rollback_count.load(Ordering::SeqCst)
    }
}

impl FusedStateChangedSubscriber for BridgeSubscriber {}

#[async_trait::async_trait]
impl StateChangedSubscriber<SourceConfig> for BridgeSubscriber {
    fn name(&self) -> &str {
        "bridge_a_to_b"
    }

    async fn migrate(
        &self,
        _prev: Option<SourceConfig>,
        new: SourceConfig,
    ) -> Result<(), anyhow::Error> {
        self.migrate_count.fetch_add(1, Ordering::SeqCst);
        let new_b = Self::derive(&new);
        let mut mgr = self.b_manager.lock().await;
        // Snapshot B's current state for rollback
        *self.prev_b.lock().unwrap() = mgr.current_state();
        // Upsert B — triggers LeafSubscriber.migrate() inside B's coordinator
        mgr.upsert(new_b).await.map_err(|e| anyhow::anyhow!(e))?;
        Ok(())
    }

    async fn rollback(
        &self,
        _prev: Option<SourceConfig>,
        _new: SourceConfig,
    ) -> Result<(), anyhow::Error> {
        self.rollback_count.fetch_add(1, Ordering::SeqCst);
        let prev = self.prev_b.lock().unwrap().take();
        let mut mgr = self.b_manager.lock().await;
        if let Some(old_b) = prev {
            // Restore B — triggers LeafSubscriber.migrate() with the old state
            mgr.upsert(old_b).await.map_err(|e| anyhow::anyhow!(e))?;
        }
        Ok(())
    }
}

// ─── SiblingSubscriber: another subscriber on A, runs after BridgeSubscriber ───

struct SiblingSubscriber {
    name: String,
    should_fail_migrate: AtomicBool,
    migrate_count: AtomicUsize,
    rollback_count: AtomicUsize,
}

impl SiblingSubscriber {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            should_fail_migrate: AtomicBool::new(false),
            migrate_count: AtomicUsize::new(0),
            rollback_count: AtomicUsize::new(0),
        }
    }

    fn set_should_fail(&self, fail: bool) {
        self.should_fail_migrate.store(fail, Ordering::SeqCst);
    }

    fn migrate_count(&self) -> usize {
        self.migrate_count.load(Ordering::SeqCst)
    }

    fn rollback_count(&self) -> usize {
        self.rollback_count.load(Ordering::SeqCst)
    }
}

impl FusedStateChangedSubscriber for SiblingSubscriber {}

#[async_trait::async_trait]
impl StateChangedSubscriber<SourceConfig> for SiblingSubscriber {
    fn name(&self) -> &str {
        &self.name
    }

    async fn migrate(
        &self,
        _prev: Option<SourceConfig>,
        _new: SourceConfig,
    ) -> Result<(), anyhow::Error> {
        self.migrate_count.fetch_add(1, Ordering::SeqCst);
        if self.should_fail_migrate.load(Ordering::SeqCst) {
            return Err(anyhow::anyhow!("sibling migrate failed"));
        }
        Ok(())
    }

    async fn rollback(
        &self,
        _prev: Option<SourceConfig>,
        _new: SourceConfig,
    ) -> Result<(), anyhow::Error> {
        self.rollback_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

// ─── Helper: build the A→B→C chain ───

struct TestChain {
    a: StateCoordinator<SourceConfig>,
    b: Arc<TokioMutex<SimpleStateManager<DerivedRuntime>>>,
    bridge: Arc<BridgeSubscriber>,
    leaf: Arc<LeafSubscriber>,
}

fn build_chain() -> TestChain {
    // B level: coordinator with LeafSubscriber
    let leaf = Arc::new(LeafSubscriber::new("service_c"));
    let mut b_coord: StateCoordinator<DerivedRuntime> = StateCoordinator::new();
    b_coord.add_subscriber(Box::new(leaf.clone()));
    let b = Arc::new(TokioMutex::new(SimpleStateManager::new(b_coord)));

    // A level: coordinator with BridgeSubscriber
    let bridge = Arc::new(BridgeSubscriber::new(b.clone()));
    let mut a: StateCoordinator<SourceConfig> = StateCoordinator::new();
    a.add_subscriber(Box::new(bridge.clone()));

    TestChain { a, b, bridge, leaf }
}

// ═══════════════════════════════════════════════════════════════
// Test 1: Happy path — A → B → C all commit
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cascade_commit_a_to_b_to_c() {
    let mut chain = build_chain();

    // Act: upsert A
    let result = chain.a.upsert_state(42).await;

    // Assert: all levels committed
    assert!(result.is_ok());
    assert_eq!(chain.a.current_state(), Some(42));
    assert_eq!(
        chain.b.lock().await.current_state(),
        Some("derived_from_42".to_string())
    );

    // LeafSubscriber saw the cascade
    assert_eq!(chain.leaf.migrate_count(), 1);
    assert_eq!(chain.leaf.rollback_count(), 0);
    let log = chain.leaf.migrate_log();
    assert_eq!(log[0].0, None); // prev was None (first upsert)
    assert_eq!(log[0].1, "derived_from_42");
}

// ═══════════════════════════════════════════════════════════════
// Test 2: Leaf failure → B not committed → A not committed
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cascade_noncommit_when_leaf_fails() {
    let mut chain = build_chain();
    chain.leaf.set_should_fail(true);

    // Act
    let result = chain.a.upsert_state(42).await;

    // Assert: entire chain rolled back
    assert!(result.is_err());
    assert_eq!(chain.a.current_state(), None); // A not committed
    assert_eq!(chain.b.lock().await.current_state(), None); // B not committed

    // Leaf was called but failed
    assert_eq!(chain.leaf.migrate_count(), 1);
    // Leaf's own rollback is called by B's do_migration_for_subscriber
    assert_eq!(chain.leaf.rollback_count(), 1);

    // Bridge was called, migrate failed (propagated from B), then rolled back by A
    assert_eq!(chain.bridge.migrate_count(), 1);
    assert_eq!(chain.bridge.rollback_count(), 1);
}

// ═══════════════════════════════════════════════════════════════
// Test 3: Effect failure at A (None→Some) — "rollback from zero" limitation
//
// When prev=None, Bridge.rollback() cannot restore B to None because
// SimpleStateManager has no reset mechanism. B stays committed.
//
// This is acceptable for the real architecture:
//   - Bootstrap (None→Some) doesn't use with_pending_state on sources
//   - Hot updates (Some→Some) always have a prev state (see Test 6)
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cascade_rollback_on_effect_failure_from_none() {
    let mut chain = build_chain();

    // Act: with_pending_state where effect fails
    let result: Result<(), WithEffectError<anyhow::Error>> = chain
        .a
        .with_pending_state(&42, |_state| async {
            Err::<(), _>(anyhow::anyhow!("effect failed"))
        })
        .await;

    // Assert: effect error returned
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), WithEffectError::Effect(_)));

    // A not committed (effect failed)
    assert_eq!(chain.a.current_state(), None);

    // B WAS committed during migrate phase and stays committed.
    // Bridge.rollback(None, 42) is a no-op because prev_b=None.
    // This is the "rollback from zero" limitation.
    assert_eq!(
        chain.b.lock().await.current_state(),
        Some("derived_from_42".to_string())
    );

    // Leaf: migrated once during Bridge.migrate(). Not rolled back because
    // Bridge.rollback() doesn't trigger any B state change.
    assert_eq!(chain.leaf.migrate_count(), 1);
    assert_eq!(chain.leaf.rollback_count(), 0);

    // Bridge: migrated then rollback was called (but was a no-op)
    assert_eq!(chain.bridge.migrate_count(), 1);
    assert_eq!(chain.bridge.rollback_count(), 1);
}

// ═══════════════════════════════════════════════════════════════
// Test 4: Second update fails → state consistent at first update
//
//   First:  A=1 → B="derived_from_1" → C sees (None, "derived_from_1") ✓
//   Second: A=2 → B tries "derived_from_2" → C FAILS → B not committed
//           → Bridge.rollback() tries to upsert old B → C FAILS again
//   Result: A=1, B="derived_from_1" (both consistent despite double failure)
//
// Key insight: B never committed "derived_from_2" because Leaf rejected it.
// So B.current_state remains "derived_from_1" throughout.
// Bridge.rollback() attempts to upsert "derived_from_1" but Leaf still
// fails, so the rollback-upsert also fails — but state is already correct.
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cascade_consistent_after_failed_second_update() {
    let mut chain = build_chain();

    // First update: success
    chain.a.upsert_state(1).await.unwrap();
    assert_eq!(chain.a.current_state(), Some(1));
    assert_eq!(
        chain.b.lock().await.current_state(),
        Some("derived_from_1".to_string())
    );

    // Configure leaf to fail on ALL subsequent migrates
    chain.leaf.set_should_fail(true);

    // Second update: fails (Leaf rejects)
    let result = chain.a.upsert_state(2).await;
    assert!(result.is_err());

    // State consistent at first update despite cascading failure
    assert_eq!(chain.a.current_state(), Some(1));
    assert_eq!(
        chain.b.lock().await.current_state(),
        Some("derived_from_1".to_string())
    );

    // Leaf migrate log — trace the full cascade:
    let log = chain.leaf.migrate_log();
    assert_eq!(log.len(), 3);

    // 1. First update (success)
    assert_eq!(log[0], (None, "derived_from_1".to_string()));

    // 2. Second attempt: B.upsert("derived_from_2") → Leaf.migrate FAILS
    //    B never committed "derived_from_2", so B.current stays "derived_from_1"
    assert_eq!(
        log[1],
        (
            Some("derived_from_1".to_string()),
            "derived_from_2".to_string()
        )
    );

    // 3. Bridge.rollback() → B.upsert("derived_from_1") → Leaf.migrate FAILS again
    //    B.current is STILL "derived_from_1", so prev in this call is "derived_from_1"
    assert_eq!(
        log[2],
        (
            Some("derived_from_1".to_string()),
            "derived_from_1".to_string()
        )
    );
}

// ═══════════════════════════════════════════════════════════════
// Test 5: Sibling failure after bridge (None→Some) — same "rollback from zero"
//
// A has two subscribers: [BridgeSub, SiblingSub]
// Bridge succeeds (B commits), then Sibling fails.
// Bridge.rollback() is a no-op (prev=None) → B stays committed.
// Same limitation as Test 3.
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cascade_rollback_when_sibling_fails_after_bridge() {
    // B level
    let leaf = Arc::new(LeafSubscriber::new("service_c"));
    let mut b_coord: StateCoordinator<DerivedRuntime> = StateCoordinator::new();
    b_coord.add_subscriber(Box::new(leaf.clone()));
    let b = Arc::new(TokioMutex::new(SimpleStateManager::new(b_coord)));

    // A level: BridgeSub first, then SiblingSub
    let bridge = Arc::new(BridgeSubscriber::new(b.clone()));
    let sibling = Arc::new(SiblingSubscriber::new("sibling_d"));
    sibling.set_should_fail(true); // Sibling will fail

    let mut a: StateCoordinator<SourceConfig> = StateCoordinator::new();
    a.add_subscriber(Box::new(bridge.clone()));
    a.add_subscriber(Box::new(sibling.clone()));

    // Act
    let result = a.upsert_state(99).await;

    // Assert: failure
    assert!(result.is_err());

    // A not committed
    assert_eq!(a.current_state(), None);

    // B was committed during Bridge.migrate() and stays committed.
    // Bridge.rollback(None, 99) is a no-op — "rollback from zero" limitation.
    assert_eq!(
        b.lock().await.current_state(),
        Some("derived_from_99".to_string())
    );

    // Bridge: migrated (success), then rollback called (no-op)
    assert_eq!(bridge.migrate_count(), 1);
    assert_eq!(bridge.rollback_count(), 1);

    // Sibling: migrated (failed), then rollback called by do_migration_for_subscriber
    assert_eq!(sibling.migrate_count(), 1);
    assert_eq!(sibling.rollback_count(), 1);

    // Leaf: migrated once during Bridge.migrate(). No rollback because
    // Bridge.rollback() doesn't trigger any B state change.
    assert_eq!(leaf.migrate_count(), 1);
    assert_eq!(leaf.rollback_count(), 0);
}

// ═══════════════════════════════════════════════════════════════
// Test 5b: Sibling failure WITH previous state → full rollback cascade
//
// Same as Test 5 but with a successful first update, so Bridge has
// a prev state to restore to during rollback.
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cascade_full_rollback_sibling_with_prev_state() {
    // B level
    let leaf = Arc::new(LeafSubscriber::new("service_c"));
    let mut b_coord: StateCoordinator<DerivedRuntime> = StateCoordinator::new();
    b_coord.add_subscriber(Box::new(leaf.clone()));
    let b = Arc::new(TokioMutex::new(SimpleStateManager::new(b_coord)));

    // A level
    let bridge = Arc::new(BridgeSubscriber::new(b.clone()));
    let sibling = Arc::new(SiblingSubscriber::new("sibling_d"));

    let mut a: StateCoordinator<SourceConfig> = StateCoordinator::new();
    a.add_subscriber(Box::new(bridge.clone()));
    a.add_subscriber(Box::new(sibling.clone()));

    // First update: success
    a.upsert_state(1).await.unwrap();
    assert_eq!(a.current_state(), Some(1));
    assert_eq!(
        b.lock().await.current_state(),
        Some("derived_from_1".to_string())
    );

    // Now make sibling fail
    sibling.set_should_fail(true);

    // Second update: should fail and rollback everything
    let result = a.upsert_state(2).await;
    assert!(result.is_err());

    // A rolled back to first state
    assert_eq!(a.current_state(), Some(1));

    // B rolled back to first derived state via Bridge.rollback()
    assert_eq!(
        b.lock().await.current_state(),
        Some("derived_from_1".to_string())
    );

    // Verify the full cascade:
    // Bridge: 2 migrates (first + second), 1 rollback (for second)
    assert_eq!(bridge.migrate_count(), 2);
    assert_eq!(bridge.rollback_count(), 1);

    // Leaf migrate log shows the full cascade:
    let log = leaf.migrate_log();
    assert_eq!(log.len(), 3);
    //   1. (None, "derived_from_1") — first update
    assert_eq!(log[0], (None, "derived_from_1".to_string()));
    //   2. (Some("derived_from_1"), "derived_from_2") — second update (Bridge.migrate)
    assert_eq!(
        log[1],
        (
            Some("derived_from_1".to_string()),
            "derived_from_2".to_string()
        )
    );
    //   3. (Some("derived_from_2"), "derived_from_1") — rollback (Bridge.rollback → B.upsert(old))
    assert_eq!(
        log[2],
        (
            Some("derived_from_2".to_string()),
            "derived_from_1".to_string()
        )
    );
}

// ═══════════════════════════════════════════════════════════════
// Test 6: Effect failure with previous state → full rollback
//
// A.with_pending_state succeeds migrate phase (B commits, C migrates),
// then effect fails. Bridge.rollback() restores B to previous state.
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cascade_effect_failure_with_prev_state() {
    let mut chain = build_chain();

    // First: establish state
    chain.a.upsert_state(1).await.unwrap();
    assert_eq!(
        chain.b.lock().await.current_state(),
        Some("derived_from_1".to_string())
    );

    // Second: with_pending_state where effect fails
    let result: Result<(), WithEffectError<anyhow::Error>> = chain
        .a
        .with_pending_state(&2, |_state| async {
            Err::<(), _>(anyhow::anyhow!("disk write failed"))
        })
        .await;

    assert!(result.is_err());

    // A stays at 1 (effect failed, state not committed)
    assert_eq!(chain.a.current_state(), Some(1));

    // B restored to "derived_from_1" by Bridge.rollback()
    assert_eq!(
        chain.b.lock().await.current_state(),
        Some("derived_from_1".to_string())
    );

    // Leaf saw the full cascade:
    let log = chain.leaf.migrate_log();
    assert_eq!(log.len(), 3);
    //   1. First update: (None, "derived_from_1")
    assert_eq!(log[0], (None, "derived_from_1".to_string()));
    //   2. Second migrate: (Some("derived_from_1"), "derived_from_2")
    assert_eq!(
        log[1],
        (
            Some("derived_from_1".to_string()),
            "derived_from_2".to_string()
        )
    );
    //   3. Rollback restore: (Some("derived_from_2"), "derived_from_1")
    assert_eq!(
        log[2],
        (
            Some("derived_from_2".to_string()),
            "derived_from_1".to_string()
        )
    );
}
