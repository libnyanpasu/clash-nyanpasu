//! Snapshot publication through the persistent owner's retained write APIs.

use crate::{ReplaceIfVersionError, ReplaceIfVersionResult, Version};

use super::support::{TestState, config_path, manager_at, read_config};

#[tokio::test]
async fn test_snapshot_reflects_committed_state() {
    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir);
    let handle = manager.snapshot_handle();
    let previous = handle.load();
    assert_eq!(previous.version, Version::new(0));
    assert_eq!(previous.state, TestState::default());

    manager
        .upsert(TestState::new("committed", 42))
        .await
        .unwrap();

    assert_eq!(handle.load().state, TestState::new("committed", 42));
    assert_eq!(handle.load().version, Version::new(1));
    assert_eq!(previous.state, TestState::default());
    assert_eq!(previous.version, Version::new(0));
}

#[tokio::test]
async fn test_snapshot_updates_on_each_commit() {
    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir);
    let handle = manager.snapshot_handle();
    for value in 1..=3 {
        manager
            .upsert(TestState::new("committed", value))
            .await
            .unwrap();
        assert_eq!(handle.load().value, value);
        assert_eq!(handle.load().version, Version::new(value as u64));
    }
}

#[tokio::test]
async fn test_snapshot_not_updated_on_effect_failure() {
    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir);
    manager
        .upsert(TestState::new("committed", 1))
        .await
        .unwrap();
    let handle = manager.snapshot_handle();
    let previous = handle.load();

    let result = manager
        .replace_if_version_with_local_write(
            Version::new(1),
            TestState::new("candidate", 2),
            || async { Err(anyhow::anyhow!("effect failed")) },
            || async { Ok(()) },
        )
        .await;

    assert!(matches!(result, Err(ReplaceIfVersionError::LocalWrite(_))));
    assert_eq!(handle.load().state, previous.state);
    assert_eq!(handle.load().version, previous.version);
    assert_eq!(read_config(&config_path(&dir)).await, previous.state);
}

#[tokio::test]
async fn test_snapshot_updated_on_effect_success() {
    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir);
    let handle = manager.snapshot_handle();
    let candidate = TestState::new("committed", 42);

    let result = manager
        .replace_if_version_with_local_write(
            Version::new(0),
            candidate.clone(),
            || async { Ok(()) },
            || async { Ok(()) },
        )
        .await
        .unwrap();

    assert!(matches!(result, ReplaceIfVersionResult::Replaced));
    assert_eq!(handle.load().state, candidate);
    assert_eq!(handle.load().version, Version::new(1));
}

#[tokio::test]
async fn test_multiple_handles_see_same_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir);
    let first = manager.snapshot_handle();
    let second = first.clone();
    let third = manager.snapshot_handle();

    manager
        .upsert(TestState::new("committed", 42))
        .await
        .unwrap();

    for handle in [first, second, third] {
        assert_eq!(handle.load().state, TestState::new("committed", 42));
        assert_eq!(handle.load().version, Version::new(1));
    }
}

#[tokio::test]
async fn each_write_publishes_the_snapshot_before_returning() {
    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir);
    let handle = manager.snapshot_handle();

    manager.upsert(TestState::new("upsert", 1)).await.unwrap();
    assert_eq!(handle.load().name, "upsert");
    assert_eq!(handle.load().version, Version::new(1));

    let result = manager
        .replace_if_version(Version::new(1), TestState::new("replace", 2))
        .await
        .unwrap();
    assert!(matches!(result, ReplaceIfVersionResult::Replaced));
    assert_eq!(handle.load().name, "replace");
    assert_eq!(handle.load().version, Version::new(2));

    let resource = dir.path().join("managed.yaml");
    let result = manager
        .replace_if_version_with_local_write(
            Version::new(2),
            TestState::new("local", 3),
            || async {
                tokio::fs::write(&resource, "new content").await?;
                Ok(())
            },
            || async { panic!("successful persistence needs no recovery") },
        )
        .await
        .unwrap();
    assert!(matches!(result, ReplaceIfVersionResult::Replaced));
    assert_eq!(handle.load().name, "local");
    assert_eq!(handle.load().version, Version::new(3));
    assert_eq!(read_config(&config_path(&dir)).await, handle.load().state);
    assert_eq!(
        tokio::fs::read_to_string(resource).await.unwrap(),
        "new content"
    );
}
