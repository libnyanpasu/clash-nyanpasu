//! Owner bookkeeping belongs to the installed incarnation, including during failed attempts.

use super::*;
use crate::version::LifecycleState;

/// Retirement drains old budgets; only installation replaces budgets and resets owner state.
#[test]
fn recovery_resets_owner_bookkeeping_only_when_the_candidate_is_installed() {
    let disk = MemoryDisk::new();
    let mut store = open(&disk);
    write_simple(&mut store, 1, Durability::Durable);
    write_simple(&mut store, 2, Durability::Consistent);
    store.shared.registry.set_behind(true);
    let old_head = store.head_budget.clone();
    let old_staging = store.staging_budget.clone();
    let shared = store.shared.clone();
    let bookkeeping = (store.last_durable, store.since_durable, store.behind());
    assert!(bookkeeping.0.is_some());
    assert!(bookkeeping.1 > 0);
    let before = store.next_event_seq();
    let snapshot = store.reader().capture().unwrap();
    assert!(old_head.reserved() > 0);
    assert!(matches!(
        store
            .recover_with::<()>(
                || panic!("reopened before draining"),
                |_| panic!("restored before draining"),
            )
            .unwrap(),
        RecoveryProgress::WaitingForReaders
    ));
    assert_eq!(shared.registry.state(), LifecycleState::WaitingForReaders);
    assert_eq!(
        (store.last_durable, store.since_durable, store.behind()),
        bookkeeping
    );
    assert_eq!(
        store.next_event_seq(),
        EventSeq {
            incarnation: before.incarnation,
            seq: before.seq + 1,
        }
    );
    assert_eq!(old_head.reserved(), 0);
    assert_eq!(old_staging.reserved(), 0);
    assert!(shared.query_budget.reserved() > 0);
    drop(snapshot);
    assert_eq!(shared.query_budget.reserved(), 0);

    let error = store
        .recover_with::<()>(
            || Err(StorageError::Closed),
            |_| panic!("restored without a backend"),
        )
        .unwrap_err();
    assert!(matches!(error, RecoveryError::Reopen { .. }));
    assert_eq!(shared.registry.state(), LifecycleState::Unavailable);
    assert_eq!(
        (store.last_durable, store.since_durable, store.behind()),
        bookkeeping
    );
    assert_eq!(
        store.next_event_seq(),
        EventSeq {
            incarnation: before.incarnation,
            seq: before.seq + 2,
        }
    );

    let mut failed_incarnation = 0;
    let error = store
        .recover_with::<()>(
            || disk.open(),
            |view| {
                failed_incarnation = view.incarnation();
                Err(RecoveryError::Restore {
                    kind: ErrorKind::Local,
                    source: Box::new(std::io::Error::other("host declined")),
                })
            },
        )
        .unwrap_err();
    assert!(matches!(error, RecoveryError::Restore { .. }));
    assert_eq!(shared.registry.state(), LifecycleState::Unavailable);
    assert_eq!(
        (store.last_durable, store.since_durable, store.behind()),
        bookkeeping
    );
    assert_eq!(
        store.next_event_seq(),
        EventSeq {
            incarnation: before.incarnation,
            seq: before.seq + 3,
        }
    );
    assert_eq!(old_head.reserved(), 0);
    assert_eq!(old_staging.reserved(), 0);
    assert_eq!(shared.query_budget.reserved(), 0);

    let progress = store
        .recover_with(
            || {
                assert_eq!(old_head.reserved(), 0);
                assert_eq!(old_staging.reserved(), 0);
                assert_eq!(shared.query_budget.reserved(), 0);
                disk.open()
            },
            |view| Ok(view.incarnation()),
        )
        .unwrap();
    let RecoveryProgress::Recovered {
        restored,
        incarnation,
        ..
    } = progress
    else {
        panic!("no old reader remains");
    };
    assert_eq!(restored, incarnation);
    assert!(incarnation > failed_incarnation);
    assert_eq!(
        (store.last_durable, store.since_durable, store.behind()),
        (None, 0, false)
    );
    assert_eq!(
        store.next_event_seq(),
        EventSeq {
            incarnation,
            seq: 0
        }
    );
    assert!(Arc::ptr_eq(&shared, &store.shared));
    assert_eq!(old_head.reserved(), 0);
    assert_eq!(old_staging.reserved(), 0);
    assert!(store.head_budget.reserved() > 0);
    assert_eq!(store.staging_budget.reserved(), 0);
    let snapshot = store.reader().capture().unwrap();
    assert_eq!(read_state(snapshot.storage()).unwrap().last_batch, Some(2));
    drop(snapshot);
    assert_eq!(shared.query_budget.reserved(), 0);
}
