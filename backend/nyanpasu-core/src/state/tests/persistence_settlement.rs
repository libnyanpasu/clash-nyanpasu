//! Every manager write settles the same way: if the commit is refused after the
//! candidate was already written to disk, the committed state is written back,
//! and a recovery that fails is reported as its own error naming what was left
//! inconsistent.

use crate::state::{
    ReplaceIfVersionError, Version,
    error::{InconsistentPersistence, StateChangedError, UpsertError},
};

use super::support::{
    REFUSED_NAME, RefuseNamedFormat, StoreHijacker, TestState, config_path, manager_at,
    manager_with_formatter, read_config,
};

#[tokio::test]
async fn a_failed_recovery_is_not_a_clean_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = config_path(&dir);
    let mut manager = manager_with_formatter::<RefuseNamedFormat>(&dir).await;
    // The winner is the state the recovery has to write back, and the formatter
    // refuses exactly that one.
    let hijacker = StoreHijacker {
        store: manager.state_store(),
        winner: TestState::new(REFUSED_NAME, 7),
        winner_version: Version::new(9),
    };
    manager.add_subscriber(Box::new(hijacker));

    let result = manager
        .replace_if_version(Version::new(0), TestState::new("candidate", 1))
        .await;

    match result {
        Err(ReplaceIfVersionError::Recovery {
            commit_error,
            recovery_error,
            inconsistent:
                InconsistentPersistence {
                    config_path: reported_path,
                    local_write_completed,
                },
        }) => {
            assert!(
                matches!(
                    commit_error,
                    StateChangedError::StateCasMismatch { expected, actual }
                        if expected == Version::new(0) && actual == Version::new(9)
                ),
                "got {commit_error:?}"
            );
            assert!(recovery_error.to_string().contains("refusing to serialize"));
            assert_eq!(reported_path, config_path);
            assert!(!local_write_completed);
        }
        other => panic!("expected a structured recovery failure, got {other:?}"),
    }

    // The store holds the winner while the file still holds the rejected
    // candidate: exactly the inconsistency the error names.
    assert_eq!(manager.snapshot().name, REFUSED_NAME);
    assert_eq!(read_config(&config_path).await.name, "candidate");
}

/// The unconditional `upsert` runs the same conditional-replacement
/// transaction, so a lost commit puts the committed state back on disk instead
/// of leaving the rejected candidate there.
#[tokio::test]
async fn upsert_restores_the_config_after_a_lost_commit() {
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

    let result = manager.upsert(TestState::new("candidate", 1)).await;

    assert!(
        matches!(
            result,
            Err(UpsertError::State(StateChangedError::StateCasMismatch {
                expected,
                actual
            })) if expected == Version::new(0) && actual == Version::new(9)
        ),
        "got {result:?}"
    );
    assert_eq!(&*manager.snapshot(), &winner);
    assert_eq!(
        read_config(&config_path).await,
        winner,
        "the config file must follow the committed state, not the rejected candidate"
    );
}
