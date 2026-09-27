//! Contract tests for the single-shot participant entry, the authoritative
//! decision handle, and the persistence settlement that surrounds them.
//!
//! These cover what the application workflow relies on when it joins a source
//! config transaction as a per-mutation participant:
//!
//! 1. every attempt gets its own participant object, so a late callback can
//!    only carry the identity of the attempt that created it;
//! 2. a committed transaction reads as committed before its `on_committed`
//!    notification lands;
//! 3. the caller's own local write sits between prepare and the commit, and a
//!    refusal after it completed is never reported as a clean rejection;
//! 4. a dropped transaction records the unknown on-disk outcome instead of
//!    assuming nothing was written.

use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::{Mutex, Notify};

use crate::state::{
    AbortResourceState, Ack, DecisionHandle, ReplaceIfVersionError, ReplaceIfVersionResult,
    RollbackReason, StateAckSubscriber, StateChange, StateChangeId, StateDecision,
    StateParticipant, SubscriberName, Version, error::StateChangedError,
};

use super::support::{StoreHijacker, TestState, config_path, manager_at, read_config};

/// A mutation whose only persistence is the config file the manager writes.
fn no_local_write() -> impl std::future::Future<Output = anyhow::Result<()>> {
    std::future::ready(Ok(()))
}

// -- participants and subscribers --

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prepare,
    Committed,
    RolledBack,
}

/// One attempt's participant. It captures the attempt identity and its decision
/// handle at construction, exactly like the application-side adapter will
/// capture its `OperationId`.
///
/// Every callback records what the authoritative decision said at that moment,
/// which is how the "abort before rollback notifications" ordering is checked.
struct RecordingParticipant {
    attempt: u64,
    decision: DecisionHandle,
    log: Arc<Mutex<Vec<(u64, Phase, StateDecision)>>>,
}

impl RecordingParticipant {
    async fn record(&self, phase: Phase) {
        self.log
            .lock()
            .await
            .push((self.attempt, phase, self.decision.decision()));
    }
}

#[async_trait::async_trait]
impl StateAckSubscriber<TestState> for RecordingParticipant {
    fn name(&self) -> SubscriberName<'_> {
        "mutation-participant".into()
    }

    async fn on_prepare(&self, _change: StateChange<TestState>) -> Ack {
        self.record(Phase::Prepare).await;
        Ack::Ok
    }

    async fn on_committed(&self, _change: StateChange<TestState>) -> Ack {
        self.record(Phase::Committed).await;
        Ack::Ok
    }

    async fn on_rolled_back(&self, _change: StateChange<TestState>, _reason: RollbackReason) {
        self.record(Phase::RolledBack).await;
    }
}

/// A participant that takes part without recording or vetoing anything.
struct SilentParticipant;

#[async_trait::async_trait]
impl StateAckSubscriber<TestState> for SilentParticipant {
    fn name(&self) -> SubscriberName<'_> {
        "silent-participant".into()
    }
}

/// A permanently registered subscriber that vetoes every prepare.
struct RejectingSubscriber;

#[async_trait::async_trait]
impl StateAckSubscriber<TestState> for RejectingSubscriber {
    fn name(&self) -> SubscriberName<'_> {
        "rejector".into()
    }

    async fn on_prepare(&self, _change: StateChange<TestState>) -> Ack {
        Ack::Rejected("not acceptable".to_string())
    }
}

// -- tests --

#[tokio::test]
async fn two_failed_attempts_keep_separate_participant_identities() {
    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir).await;
    manager.add_subscriber(Box::new(RejectingSubscriber));

    let log = Arc::new(Mutex::new(Vec::new()));
    let participants: Arc<StdMutex<Vec<StateParticipant<TestState>>>> =
        Arc::new(StdMutex::new(Vec::new()));

    for attempt in [1_u64, 2] {
        // Both attempts target the same state version: a vetoed prepare must
        // not consume it.
        let result = manager
            .replace_if_version_with_participant(
                Version::new(0),
                TestState::new("candidate", attempt as i32),
                |decision| {
                    let participant = Arc::new(RecordingParticipant {
                        attempt,
                        decision,
                        log: Arc::clone(&log),
                    });
                    participants
                        .lock()
                        .unwrap()
                        .push(Arc::clone(&participant) as StateParticipant<TestState>);
                    participant
                },
                no_local_write,
                || async { Ok(()) },
            )
            .await;

        assert!(
            matches!(
                result,
                Err(ReplaceIfVersionError::State(StateChangedError::PrepareAck(
                    _
                )))
            ),
            "attempt {attempt} should be vetoed, got {result:?}"
        );
    }

    let participants = participants.lock().unwrap().clone();
    assert_eq!(participants.len(), 2);
    assert!(
        !Arc::ptr_eq(&participants[0], &participants[1]),
        "each attempt must build its own participant object"
    );

    // Every callback carries the identity of the attempt that built the object
    // it reached, and the abort is already recorded when the rollback
    // notification arrives.
    assert_eq!(
        *log.lock().await,
        vec![
            (1, Phase::Prepare, StateDecision::Undecided),
            (
                1,
                Phase::RolledBack,
                StateDecision::Aborted {
                    resources: AbortResourceState::Restored
                }
            ),
            (2, Phase::Prepare, StateDecision::Undecided),
            (
                2,
                Phase::RolledBack,
                StateDecision::Aborted {
                    resources: AbortResourceState::Restored
                }
            ),
        ]
    );

    // A callback arriving late is delivered to the object that ran the attempt,
    // so it is still tagged with that attempt and reads that attempt's decision.
    let change = StateChange {
        id: StateChangeId::new(1),
        previous: None,
        current: Arc::new(TestState::new("candidate", 1)),
    };
    participants[0]
        .on_rolled_back(
            change,
            RollbackReason::CoordinatorError(Arc::new(anyhow::anyhow!("late rollback"))),
        )
        .await;
    assert_eq!(
        log.lock().await.last().cloned(),
        Some((
            1,
            Phase::RolledBack,
            StateDecision::Aborted {
                resources: AbortResourceState::Restored
            }
        ))
    );

    assert_eq!(manager.snapshot().name, "");
}

/// The participant's `on_committed` is held still, so the notification has not
/// landed. The decision handle already proves the commit.
#[tokio::test]
async fn a_committed_decision_is_readable_before_the_commit_notification_lands() {
    struct HeldParticipant {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<TestState> for HeldParticipant {
        fn name(&self) -> SubscriberName<'_> {
            "held-participant".into()
        }

        async fn on_committed(&self, _change: StateChange<TestState>) -> Ack {
            self.entered.notify_one();
            self.release.notified().await;
            Ack::Ok
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir).await;
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let handle: Arc<StdMutex<Option<DecisionHandle>>> = Arc::new(StdMutex::new(None));

    let mut replace = Box::pin(manager.replace_if_version_with_participant(
        Version::new(0),
        TestState::new("next", 1),
        |decision| {
            *handle.lock().unwrap() = Some(decision);
            Arc::new(HeldParticipant {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
            })
        },
        no_local_write,
        || async { Ok(()) },
    ));
    tokio::select! {
        result = &mut replace => panic!("the commit notification is held, got {result:?}"),
        _ = entered.notified() => {}
    }

    let handle = handle.lock().unwrap().clone().expect("handle was built");
    assert_eq!(
        handle.decision(),
        StateDecision::Committed {
            version: Version::new(1)
        }
    );

    release.notify_one();
    let result = replace.await.unwrap();
    assert!(matches!(result, ReplaceIfVersionResult::Replaced));
}

#[tokio::test]
async fn the_local_write_runs_between_prepare_and_the_commit() {
    struct OrderRecorder {
        who: &'static str,
        log: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<TestState> for OrderRecorder {
        fn name(&self) -> SubscriberName<'_> {
            self.who.into()
        }

        async fn on_prepare(&self, _change: StateChange<TestState>) -> Ack {
            self.log.lock().await.push(format!("prepare:{}", self.who));
            Ack::Ok
        }

        async fn on_committed(&self, _change: StateChange<TestState>) -> Ack {
            self.log
                .lock()
                .await
                .push(format!("committed:{}", self.who));
            Ack::Ok
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let config_path = config_path(&dir);
    let mut manager = manager_at(&dir).await;
    let log = Arc::new(Mutex::new(Vec::new()));
    manager.add_subscriber(Box::new(OrderRecorder {
        who: "registered",
        log: Arc::clone(&log),
    }));

    let participant_log = Arc::clone(&log);
    let local_log = Arc::clone(&log);
    let result = manager
        .replace_if_version_with_participant(
            Version::new(0),
            TestState::new("promoted", 1),
            |_decision| {
                Arc::new(OrderRecorder {
                    who: "participant",
                    log: participant_log,
                })
            },
            move || async move {
                local_log.lock().await.push("local write".to_string());
                Ok(())
            },
            || async { Ok(()) },
        )
        .await
        .unwrap();

    assert!(matches!(result, ReplaceIfVersionResult::Replaced));
    let log = log.lock().await.clone();
    let local_write = log
        .iter()
        .position(|entry| entry == "local write")
        .expect("the local write must run");
    assert!(
        log[..local_write].iter().all(|e| e.starts_with("prepare:")),
        "everything before the local write is a prepare: {log:?}"
    );
    assert_eq!(
        log[..local_write].len(),
        2,
        "both the registered subscriber and the participant prepare first: {log:?}"
    );
    assert!(
        log[local_write + 1..]
            .iter()
            .all(|e| e.starts_with("committed:")),
        "everything after the local write is a commit notification: {log:?}"
    );
    assert_eq!(read_config(&config_path).await.name, "promoted");
}

#[tokio::test]
async fn a_failed_local_write_rolls_the_transaction_back() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = config_path(&dir);
    let mut manager = manager_at(&dir).await;
    manager.upsert(TestState::new("initial", 1)).await.unwrap();

    let handle: Arc<StdMutex<Option<DecisionHandle>>> = Arc::new(StdMutex::new(None));
    let log = Arc::new(Mutex::new(Vec::new()));
    let result = manager
        .replace_if_version_with_participant(
            Version::new(1),
            TestState::new("candidate", 2),
            |decision| {
                *handle.lock().unwrap() = Some(decision.clone());
                Arc::new(RecordingParticipant {
                    attempt: 1,
                    decision,
                    log: Arc::clone(&log),
                })
            },
            || async { Err(anyhow::anyhow!("promotion failed")) },
            || async { Ok(()) },
        )
        .await;

    match result {
        Err(ReplaceIfVersionError::LocalWrite(error)) => {
            assert!(error.to_string().contains("promotion failed"));
        }
        other => panic!("expected a local write failure, got {other:?}"),
    }

    assert_eq!(manager.snapshot().name, "initial");
    assert_eq!(read_config(&config_path).await.name, "initial");
    assert_eq!(
        *log.lock().await,
        vec![
            (1, Phase::Prepare, StateDecision::Undecided),
            (
                1,
                Phase::RolledBack,
                StateDecision::Aborted {
                    resources: AbortResourceState::Restored
                }
            ),
        ]
    );
    let handle = handle.lock().unwrap().clone().unwrap();
    assert_eq!(
        handle.decision(),
        StateDecision::Aborted {
            resources: AbortResourceState::Restored
        }
    );
}

/// Dropping the mutation while its local write is outstanding leaves the
/// on-disk outcome unknown. The abort the transaction publishes on drop must say
/// so instead of inferring "nothing was written" from "the store was not
/// swapped".
#[tokio::test]
async fn dropping_a_local_write_publishes_an_abort_that_needs_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir).await;
    let handle = Arc::new(StdMutex::new(None));
    let started = Arc::new(Notify::new());
    let writing = started.clone();
    let mut pending = Box::pin(manager.replace_if_version_with_participant(
        Version::new(0),
        TestState::new("candidate", 1),
        |decision| {
            *handle.lock().unwrap() = Some(decision);
            Arc::new(SilentParticipant) as StateParticipant<TestState>
        },
        move || async move {
            writing.notify_one();
            std::future::pending::<()>().await;
            Ok(())
        },
        || async { Ok(()) },
    ));
    tokio::select! {
        result = &mut pending => panic!("write should be pending: {result:?}"),
        _ = started.notified() => {}
    }
    drop(pending);

    let handle = handle.lock().unwrap().clone().unwrap();
    assert!(matches!(
        handle.decision(),
        StateDecision::Aborted {
            resources: AbortResourceState::NeedsRecovery(_)
        }
    ));
    assert_eq!(manager.snapshot().name, "");
}

/// The other half of the same rule. A transaction dropped *before* its local
/// write started wrote nothing anywhere, so the abort says everything there is
/// to say. Flagging it would send the caller into the most expensive state in
/// the system for an ordinary dropped prepare.
#[tokio::test]
async fn dropping_before_the_local_write_starts_leaves_the_flag_clear() {
    struct ParkingPrepare {
        entered: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<TestState> for ParkingPrepare {
        fn name(&self) -> SubscriberName<'_> {
            "parking-prepare".into()
        }

        async fn on_prepare(&self, _change: StateChange<TestState>) -> Ack {
            self.entered.notify_one();
            std::future::pending::<()>().await;
            Ack::Ok
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir).await;
    let handle: Arc<StdMutex<Option<DecisionHandle>>> = Arc::new(StdMutex::new(None));
    let entered = Arc::new(Notify::new());

    {
        let entered_for_prepare = Arc::clone(&entered);
        let mut pending = Box::pin(manager.replace_if_version_with_participant(
            Version::new(0),
            TestState::new("candidate", 1),
            |decision| {
                *handle.lock().unwrap() = Some(decision);
                Arc::new(ParkingPrepare {
                    entered: entered_for_prepare,
                })
            },
            no_local_write,
            || async { Ok(()) },
        ));

        tokio::select! {
            result = &mut pending => panic!("the prepare should stay pending, got {result:?}"),
            _ = entered.notified() => {}
        }

        drop(pending);
    }

    let handle = handle.lock().unwrap().clone().unwrap();
    assert_eq!(
        handle.wait().await,
        StateDecision::Aborted {
            resources: AbortResourceState::Restored
        }
    );
    assert!(
        !matches!(
            handle.decision(),
            StateDecision::Aborted {
                resources: AbortResourceState::NeedsRecovery(_)
            }
        ),
        "nothing was written, so there is nothing to recover"
    );
    assert_eq!(manager.snapshot().name, "");
}

/// A commit refused after the local write completed leaves a published resource
/// behind. The config file is put back, that resource is not, so the caller has
/// to be told — a plain CAS mismatch would read as "nothing happened".
#[tokio::test]
async fn a_lost_commit_after_a_completed_local_write_is_not_a_clean_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = config_path(&dir);
    let mut manager = manager_at(&dir).await;
    let winner = TestState::new("winner", 7);
    let hijacker = StoreHijacker {
        store: manager.state_store(),
        winner: winner.clone(),
        winner_version: Version::new(9),
    };
    manager.add_subscriber(Box::new(hijacker));

    let promoted = Arc::new(AtomicBool::new(false));
    let promoted_flag = Arc::clone(&promoted);
    let result = manager
        .replace_if_version_with_participant(
            Version::new(0),
            TestState::new("candidate", 1),
            |_decision| Arc::new(SilentParticipant) as StateParticipant<TestState>,
            move || async move {
                promoted_flag.store(true, Ordering::SeqCst);
                Ok(())
            },
            || async { Ok(()) },
        )
        .await;

    assert!(promoted.load(Ordering::SeqCst), "the local write must run");
    match result {
        Err(ReplaceIfVersionError::State(commit_error)) => {
            assert!(
                matches!(
                    commit_error,
                    StateChangedError::StateCasMismatch { expected, actual }
                        if expected == Version::new(0) && actual == Version::new(9)
                ),
                "got {commit_error:?}"
            );
        }
        other => panic!("expected an orphaned local write, got {other:?}"),
    }

    // The config recovery itself succeeded: only the caller's publication is
    // left over, which is exactly what the variant says.
    assert_eq!(&*manager.snapshot(), &winner);
    assert_eq!(read_config(&config_path).await, winner);
}

/// A conflict never starts a transaction, but the participant already exists.
/// Its decision must still be settled, or a workflow polling the handle waits
/// for a decision that is never coming.
#[tokio::test]
async fn a_version_conflict_settles_the_participant_decision() {
    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir).await;
    manager.upsert(TestState::new("current", 1)).await.unwrap();

    let handle: Arc<StdMutex<Option<DecisionHandle>>> = Arc::new(StdMutex::new(None));
    let log = Arc::new(Mutex::new(Vec::new()));
    let result = manager
        .replace_if_version_with_participant(
            Version::new(0),
            TestState::new("stale", 2),
            |decision| {
                *handle.lock().unwrap() = Some(decision.clone());
                Arc::new(RecordingParticipant {
                    attempt: 1,
                    decision,
                    log: Arc::clone(&log),
                })
            },
            no_local_write,
            || async { Ok(()) },
        )
        .await
        .unwrap();

    assert!(matches!(
        result,
        ReplaceIfVersionResult::Conflict { actual_version } if actual_version == Version::new(1)
    ));
    assert!(
        log.lock().await.is_empty(),
        "a conflict never runs the transaction, so the participant sees no callbacks"
    );
    let handle = handle.lock().unwrap().clone().unwrap();
    assert_eq!(
        handle.decision(),
        StateDecision::Aborted {
            resources: AbortResourceState::Restored
        },
        "a conflict is a decision, not an absence of one"
    );
}

/// A CAS mismatch whose storage recovery fails leaves the persisted state
/// unknown, and the participant is the one reader that has to act on it: an
/// `Aborted` decision on its own reads as a clean cancellation it may undo its
/// own external work for (v2 §4.2).
///
/// Driven through the coordinator rather than the manager because the recovery
/// closure is what this test has to hold still: the participant must not be
/// able to settle while it is running, and must see the flag once it fails.
#[tokio::test]
async fn a_failed_persistence_recovery_qualifies_the_participants_abort() {
    use crate::state::{
        StateCoordinator, coordinator::ParticipantEntry, error::WithEffectError,
        transaction::NotifyStrategy,
    };

    let mut coordinator = StateCoordinator::<TestState>::builder()
        .with_notify_strategy(NotifyStrategy::Parallel)
        .build(TestState::new("initial", 0));
    let winner = TestState::new("winner", 7);
    coordinator.add_subscriber(Box::new(StoreHijacker {
        store: coordinator.state_store(),
        winner: winner.clone(),
        winner_version: Version::new(9),
    }));

    let handle: Arc<StdMutex<Option<DecisionHandle>>> = Arc::new(StdMutex::new(None));
    let log = Arc::new(Mutex::new(Vec::new()));
    let recovering = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let participant = {
        let handle = Arc::clone(&handle);
        let log = Arc::clone(&log);
        ParticipantEntry::new(move |decision: DecisionHandle| {
            *handle.lock().unwrap() = Some(decision.clone());
            Arc::new(RecordingParticipant {
                attempt: 1,
                decision,
                log,
            }) as StateParticipant<TestState>
        })
    };

    let candidate = TestState::new("candidate", 1);
    let recovering_for_fn = Arc::clone(&recovering);
    let release_for_fn = Arc::clone(&release);
    let mut transaction = Box::pin(coordinator.with_pending_state_if_version_with_participant(
        Version::new(0),
        &candidate,
        participant,
        |_state| async move { Ok::<(), anyhow::Error>(()) },
        move |_committed| async move {
            recovering_for_fn.notify_one();
            release_for_fn.notified().await;
            Err(anyhow::anyhow!(
                "the committed state could not be written back"
            ))
        },
    ));

    tokio::select! {
        result = &mut transaction => panic!("the recovery should stay pending: it {}", if result.is_ok() { "succeeded" } else { "failed" }),
        _ = recovering.notified() => {}
    }

    {
        let handle = handle.lock().unwrap().clone().unwrap();
        assert_eq!(
            handle.decision(),
            StateDecision::Undecided,
            "a participant polling the decision must not settle while the persistence \
             recovery it would act on is still running"
        );
    }

    release.notify_one();
    let outcome = transaction.await;
    assert!(
        matches!(outcome, Err(WithEffectError::Recovery { .. })),
        "the caller is told too, but it is not the only reader"
    );

    let handle = handle.lock().unwrap().clone().unwrap();
    assert!(
        matches!(
            handle.decision(),
            StateDecision::Aborted {
                resources: AbortResourceState::NeedsRecovery(_)
            }
        ),
        "an unknown persistence outcome must be recorded as needing recovery"
    );
    assert_eq!(
        *log.lock().await,
        vec![
            (1, Phase::Prepare, StateDecision::Undecided),
            (1, Phase::RolledBack, handle.decision()),
        ],
        "the rollback notification carries the abort the flag qualifies"
    );
}

/// The same mismatch with a recovery that succeeds is an ordinary refusal: the
/// config file holds the committed state, so nothing needs recovering and the
/// participant may cancel cleanly.
#[tokio::test]
async fn a_successful_persistence_recovery_leaves_the_abort_unqualified() {
    use crate::state::{
        StateCoordinator, coordinator::ParticipantEntry, error::WithEffectError,
        transaction::NotifyStrategy,
    };

    let mut coordinator = StateCoordinator::<TestState>::builder()
        .with_notify_strategy(NotifyStrategy::Parallel)
        .build(TestState::new("initial", 0));
    coordinator.add_subscriber(Box::new(StoreHijacker {
        store: coordinator.state_store(),
        winner: TestState::new("winner", 7),
        winner_version: Version::new(9),
    }));

    let handle: Arc<StdMutex<Option<DecisionHandle>>> = Arc::new(StdMutex::new(None));
    let participant = {
        let handle = Arc::clone(&handle);
        ParticipantEntry::new(move |decision: DecisionHandle| {
            *handle.lock().unwrap() = Some(decision);
            Arc::new(SilentParticipant) as StateParticipant<TestState>
        })
    };

    let candidate = TestState::new("candidate", 1);
    let outcome = coordinator
        .with_pending_state_if_version_with_participant(
            Version::new(0),
            &candidate,
            participant,
            |_state| async move { Ok::<(), anyhow::Error>(()) },
            |_committed| async move { Ok(()) },
        )
        .await;

    assert!(matches!(
        outcome,
        Err(WithEffectError::State(
            StateChangedError::StateCasMismatch { .. }
        ))
    ));
    let handle = handle.lock().unwrap().clone().unwrap();
    assert_eq!(
        handle.decision(),
        StateDecision::Aborted {
            resources: AbortResourceState::Restored
        }
    );
    assert!(!matches!(
        handle.decision(),
        StateDecision::Aborted {
            resources: AbortResourceState::NeedsRecovery(_)
        }
    ));
}

#[tokio::test]
async fn yaml_failure_waits_for_local_recovery_and_publishes_its_result() {
    for fail_recovery in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = manager_at(&dir).await;
        let resource = dir.path().join("managed.yaml");
        std::fs::write(&resource, "A").unwrap();
        let config = config_path(&dir);
        if config.exists() {
            std::fs::remove_file(&config).unwrap();
        }
        std::fs::create_dir(&config).unwrap();
        let handle = Arc::new(StdMutex::new(None));
        let recovering = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let write_path = resource.clone();
        let restore_path = resource.clone();
        let recovery_started = recovering.clone();
        let recovery_release = release.clone();
        let mut mutation = Box::pin(manager.replace_if_version_with_participant(
            Version::new(0),
            TestState::new("candidate", 1),
            |decision| {
                *handle.lock().unwrap() = Some(decision);
                Arc::new(SilentParticipant) as StateParticipant<TestState>
            },
            move || async move {
                std::fs::write(write_path, "B")?;
                Ok(())
            },
            move || async move {
                recovery_started.notify_one();
                recovery_release.notified().await;
                anyhow::ensure!(!fail_recovery, "managed file could not be restored");
                std::fs::write(restore_path, "A")?;
                Ok(())
            },
        ));
        tokio::select! {
            result = &mut mutation => panic!("recovery should be pending: {result:?}"),
            _ = recovering.notified() => {}
        }
        let decision = handle.lock().unwrap().clone().unwrap();
        assert_eq!(decision.decision(), StateDecision::Undecided);
        assert_eq!(std::fs::read_to_string(&resource).unwrap(), "B");
        release.notify_one();
        let result = mutation.await;
        let outcome = decision.wait().await;
        if fail_recovery {
            assert!(matches!(
                result,
                Err(ReplaceIfVersionError::ResourceRecovery { .. })
            ));
            assert!(matches!(
                outcome,
                StateDecision::Aborted {
                    resources: AbortResourceState::NeedsRecovery(_)
                }
            ));
        } else {
            assert!(matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))));
            assert_eq!(
                outcome,
                StateDecision::Aborted {
                    resources: AbortResourceState::Restored
                }
            );
            assert_eq!(std::fs::read_to_string(&resource).unwrap(), "A");
        }
        assert_eq!(manager.snapshot_handle().load().version, Version::new(0));
    }
}
