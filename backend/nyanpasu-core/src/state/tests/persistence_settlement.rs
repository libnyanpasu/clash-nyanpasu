//! A manager write that loses its commit after the candidate was written
//! returns the mismatch. The store keeps what the winner committed, and the
//! config file is not written back: a failed commit is not a disk rollback.

use crate::state::{
    Version,
    error::{StateChangedError, UpsertError},
};

use super::support::{StoreHijacker, TestState, config_path, manager_at, read_config};

/// The unconditional `upsert` runs the same conditional-replacement
/// transaction, so a lost commit is reported as the mismatch it is.
#[tokio::test]
async fn upsert_reports_a_lost_commit_without_writing_the_config_back() {
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
        read_config(&config_path).await.name,
        "candidate",
        "the config file is not written back"
    );
}
