//! Persistence failure settlement and snapshot CAS conflicts.

use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
};

use arc_swap::ArcSwap;
use nyanpasu_persistence::format::{Format, YamlFormat};
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::Notify;

use crate::{
    PersistentStateManagerSetup, ReplaceIfVersionError, Version, VersionedState,
    error::{StateChangedError, UpsertError},
};

use super::support::{TestState, config_path, manager_at, read_config};

type TestStore = Arc<ArcSwap<VersionedState<TestState>>>;

/// Inject a competing publication during serialization, after the manager has
/// captured its expected snapshot and before it attempts to publish.
#[derive(Clone, Default)]
struct HijackingFormat {
    store: Arc<Mutex<Option<TestStore>>>,
}

impl Format for HijackingFormat {
    fn serialize<W: Write, T: Serialize>(
        &self,
        writer: W,
        value: &T,
        prefix: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(store) = self.store.lock().unwrap().take() {
            store.store(Arc::new(VersionedState {
                version: Version::new(9),
                state: TestState::new("winner", 7),
            }));
        }
        YamlFormat.serialize(writer, value, prefix)
    }

    fn deserialize<R: Read, T: DeserializeOwned>(&self, reader: R) -> anyhow::Result<T> {
        YamlFormat.deserialize(reader)
    }
}

#[tokio::test]
async fn upsert_reports_a_lost_commit_without_writing_the_config_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = config_path(&dir);
    let formatter = HijackingFormat::default();
    let mut manager = PersistentStateManagerSetup::<TestState, HijackingFormat>::builder()
        .config_path(path.clone())
        .formatter(formatter.clone())
        .assemble()
        .from_state(TestState::default());
    *formatter.store.lock().unwrap() = Some(manager.state_store());

    let result = manager.upsert(TestState::new("candidate", 1)).await;

    assert!(
        matches!(result, Err(UpsertError::State(StateChangedError::StateCasMismatch {
        expected, actual
    })) if expected == Version::new(0) && actual == Version::new(9)),
        "got {result:?}"
    );
    assert_eq!(&*manager.snapshot(), &TestState::new("winner", 7));
    assert_eq!(
        read_config(&path).await.name,
        "candidate",
        "the config file is not written back"
    );

    manager.upsert(TestState::new("next", 10)).await.unwrap();
    assert_eq!(manager.snapshot_handle().load().version, Version::new(10));
}

#[tokio::test]
async fn a_lost_commit_after_a_completed_local_write_is_not_a_clean_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let mut manager = manager_at(&dir);
    let store = manager.state_store();
    let resource = dir.path().join("managed.yaml");

    let result = manager
        .replace_if_version_with_local_write(
            Version::new(0),
            TestState::new("candidate", 1),
            || async {
                tokio::fs::write(&resource, "candidate").await?;
                store.store(Arc::new(VersionedState {
                    version: Version::new(9),
                    state: TestState::new("winner", 7),
                }));
                Ok(())
            },
            || async { panic!("a CAS conflict after persistence does not recover files") },
        )
        .await;

    assert!(
        matches!(result, Err(ReplaceIfVersionError::State(StateChangedError::StateCasMismatch {
        expected, actual
    })) if expected == Version::new(0) && actual == Version::new(9)),
        "got {result:?}"
    );
    assert_eq!(manager.snapshot_handle().load().version, Version::new(9));
    assert_eq!(&*manager.snapshot(), &TestState::new("winner", 7));
    assert_eq!(read_config(&config_path(&dir)).await.name, "candidate");
    assert_eq!(
        tokio::fs::read_to_string(&resource).await.unwrap(),
        "candidate"
    );
}

#[tokio::test]
async fn yaml_failure_waits_for_local_recovery_and_publishes_its_result() {
    enum Recovery {
        Restored,
        Failed,
    }

    for recovery in [Recovery::Restored, Recovery::Failed] {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = manager_at(&dir);
        let resource = dir.path().join("managed.yaml");
        tokio::fs::write(&resource, "A").await.unwrap();
        let config = config_path(&dir);
        tokio::fs::create_dir(&config).await.unwrap();
        let handle = manager.snapshot_handle();
        let recovering = Notify::new();
        let release = Notify::new();
        let mut mutation = Box::pin(manager.replace_if_version_with_local_write(
            Version::new(0),
            TestState::new("candidate", 1),
            || async {
                tokio::fs::write(&resource, "B").await?;
                Ok(())
            },
            || async {
                recovering.notify_one();
                release.notified().await;
                anyhow::ensure!(
                    matches!(recovery, Recovery::Restored),
                    "managed file could not be restored"
                );
                tokio::fs::write(&resource, "A").await?;
                Ok(())
            },
        ));
        tokio::select! {
            result = &mut mutation => panic!("recovery should be pending: {result:?}"),
            _ = recovering.notified() => {}
        }
        assert_eq!(handle.load().version, Version::new(0));
        assert_eq!(handle.load().state, TestState::default());
        assert_eq!(tokio::fs::read_to_string(&resource).await.unwrap(), "B");
        release.notify_one();
        let result = mutation.await;
        match recovery {
            Recovery::Failed => {
                let Err(ReplaceIfVersionError::ResourceRecovery {
                    cause,
                    recovery_error,
                }) = result
                else {
                    panic!("expected a resource recovery failure: {result:?}");
                };
                assert!(
                    cause
                        .root_cause()
                        .downcast_ref::<atomicwrites::Error<std::io::Error>>()
                        .is_some(),
                    "{cause:?}"
                );
                assert_eq!(
                    recovery_error.to_string(),
                    "managed file could not be restored"
                );
                assert_eq!(tokio::fs::read_to_string(&resource).await.unwrap(), "B");
            }
            Recovery::Restored => {
                assert!(matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))));
                assert_eq!(tokio::fs::read_to_string(&resource).await.unwrap(), "A");
            }
        }
        assert_eq!(handle.load().version, Version::new(0));
        assert_eq!(handle.load().state, TestState::default());
    }
}
