//! Integration tests proving multi-level cascading state with ACK model.
//!
//! Models the real architecture:
//!
//! ```text
//! SourceConfig (A) ──subscriber: BridgeAckSub──▶ DerivedRuntime (B) ──subscriber: LeafAckSub──▶ (C)
//! ```
//!
//! Modifying A triggers B derivation via BridgeAckSub.on_committed(),
//! which in turn triggers C's LeafAckSub.on_committed() inside B's coordinator.
//!
//! The current coordinator asks subscribers to prepare before commit, then sends
//! post-commit notifications for side effects that should not affect the commit.
//!
//! Proves:
//! 1. Happy cascade: A commits → B commits → C notified
//! 2. Effect failure at A: with_pending_state effect fails → nothing committed

use crate::state::{error::WithEffectError, *};
use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Mutex as TokioMutex;

// ─── State types ───────────────────────────────────────────────

type SourceConfig = i32;
type DerivedRuntime = String;

const INITIAL_SOURCE: SourceConfig = 0;
const INITIAL_DERIVED: &str = "";

// ─── LeafAckSubscriber: terminal subscriber on B's coordinator ───

struct LeafAckSubscriber {
    name: String,
    call_count: AtomicUsize,
    call_log: StdMutex<Vec<(Option<DerivedRuntime>, DerivedRuntime)>>,
}

impl LeafAckSubscriber {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            call_count: AtomicUsize::new(0),
            call_log: StdMutex::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }

    fn call_log(&self) -> Vec<(Option<DerivedRuntime>, DerivedRuntime)> {
        self.call_log.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl StateAckSubscriber<DerivedRuntime> for LeafAckSubscriber {
    fn name(&self) -> SubscriberName<'_> {
        self.name.as_str().into()
    }

    async fn on_committed(&self, change: StateChange<DerivedRuntime>) -> Ack {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        self.call_log
            .lock()
            .unwrap()
            .push((change.previous().cloned(), change.current().clone()));
        Ack::Ok
    }
}

// ─── BridgeAckSubscriber: on A's coordinator, derives and upserts B ───

struct BridgeAckSubscriber {
    b_manager: Arc<TokioMutex<SimpleStateManager<DerivedRuntime>>>,
    call_count: AtomicUsize,
}

impl BridgeAckSubscriber {
    fn new(b_manager: Arc<TokioMutex<SimpleStateManager<DerivedRuntime>>>) -> Self {
        Self {
            b_manager,
            call_count: AtomicUsize::new(0),
        }
    }

    fn derive(source: &SourceConfig) -> DerivedRuntime {
        format!("derived_from_{source}")
    }

    fn call_count(&self) -> usize {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl StateAckSubscriber<SourceConfig> for BridgeAckSubscriber {
    fn name(&self) -> SubscriberName<'_> {
        "bridge_a_to_b".into()
    }

    async fn on_committed(&self, change: StateChange<SourceConfig>) -> Ack {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        let new_b = Self::derive(change.current());
        let mut mgr = self.b_manager.lock().await;
        match mgr.upsert(new_b).await {
            Ok(_) => Ack::Ok,
            Err(e) => Ack::Failed(anyhow::anyhow!(e)),
        }
    }
}

// ─── Helper: build the A→B→C chain ───

struct TestChain {
    a: StateCoordinator<SourceConfig>,
    b: Arc<TokioMutex<SimpleStateManager<DerivedRuntime>>>,
    bridge: Arc<BridgeAckSubscriber>,
    leaf: Arc<LeafAckSubscriber>,
}

fn build_chain() -> TestChain {
    let leaf = Arc::new(LeafAckSubscriber::new("service_c"));
    let b_coord = StateCoordinator::builder()
        .with_subscriber(Box::new(leaf.clone()))
        .build(INITIAL_DERIVED.to_string());
    let b = Arc::new(TokioMutex::new(SimpleStateManager::from_coordinator(
        b_coord,
    )));

    let bridge = Arc::new(BridgeAckSubscriber::new(b.clone()));
    let a = StateCoordinator::builder()
        .with_subscriber(Box::new(bridge.clone()))
        .build(INITIAL_SOURCE);

    TestChain { a, b, bridge, leaf }
}

// ═══════════════════════════════════════════════════════════════
// Test 1: Happy path — A commits → B commits → C notified
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cascade_commit_a_to_b_to_c() {
    let mut chain = build_chain();

    let result = chain.a.upsert_state(42).await;

    assert!(result.is_ok());
    assert_eq!(*chain.a.snapshot_versioned(), 42);
    assert_eq!(&*chain.b.lock().await.snapshot(), "derived_from_42");

    assert_eq!(chain.leaf.call_count(), 1);
    let log = chain.leaf.call_log();
    assert_eq!(log[0].0, Some(INITIAL_DERIVED.to_string()));
    assert_eq!(log[0].1, "derived_from_42");
}

// ═══════════════════════════════════════════════════════════════
// Test 2: Effect failure at A — nothing committed
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cascade_no_commit_on_effect_failure() {
    let mut chain = build_chain();

    let result: Result<((), PrepareReport), WithEffectError<anyhow::Error>> = chain
        .a
        .with_pending_state(&42, |_state| async {
            Err::<(), _>(anyhow::anyhow!("effect failed"))
        })
        .await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), WithEffectError::Effect(_)));

    // A not committed (effect failed before commit)
    assert_eq!(*chain.a.snapshot_versioned(), INITIAL_SOURCE);

    // B not committed (bridge never called)
    assert_eq!(&*chain.b.lock().await.snapshot(), INITIAL_DERIVED);

    assert_eq!(chain.bridge.call_count(), 0);
    assert_eq!(chain.leaf.call_count(), 0);
}

// ═══════════════════════════════════════════════════════════════
// Test 3: Effect failure with previous state — nothing new committed
// ═══════════════════════════════════════════════════════════════

#[tokio::test]
async fn cascade_effect_failure_with_prev_state() {
    let mut chain = build_chain();

    // First: establish state
    chain.a.upsert_state(1).await.unwrap();
    assert_eq!(&*chain.b.lock().await.snapshot(), "derived_from_1");

    // Second: with_pending_state where effect fails
    let result: Result<((), PrepareReport), WithEffectError<anyhow::Error>> = chain
        .a
        .with_pending_state(&2, |_state| async {
            Err::<(), _>(anyhow::anyhow!("disk write failed"))
        })
        .await;

    assert!(result.is_err());

    // A stays at 1 (effect failed, no commit)
    assert_eq!(*chain.a.snapshot_versioned(), 1);

    // B stays at derived_from_1 (bridge never called)
    assert_eq!(&*chain.b.lock().await.snapshot(), "derived_from_1");

    // Only 1 call to bridge (from first upsert)
    assert_eq!(chain.bridge.call_count(), 1);

    // Only 1 call to leaf (from first upsert)
    assert_eq!(chain.leaf.call_count(), 1);
}
