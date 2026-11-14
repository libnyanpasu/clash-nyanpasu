//! Atomic document persistence and versioned snapshot publication owned by a source actor.

use std::{future::Future, io::Write, pin::Pin, sync::Arc};

use anyhow::Context as _;
use arc_swap::ArcSwap;
use atomicwrites::{AllowOverwrite, AtomicFile};
use bon::Builder;
use camino::Utf8PathBuf;
use fs_err::tokio as fs;
use nyanpasu_persistence::format::{Format, YamlFormat};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    StateSnapshot, Version, VersionedState,
    error::{LoadError, StateChangedError, UpsertError},
};

/// Loading inputs for an actor-owned persistent document.
#[derive(Builder)]
#[builder(finish_fn = assemble)]
pub struct PersistentStateManagerSetup<State, Formatter = YamlFormat>
where
    State: Clone + Send + Sync + 'static,
    Formatter: Default,
{
    config_path: Utf8PathBuf,
    config_prefix: Option<String>,
    #[builder(default)]
    formatter: Formatter,
    #[builder(skip)]
    state: std::marker::PhantomData<State>,
}

impl<State, Formatter> PersistentStateManagerSetup<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + Default + 'static,
    Formatter: Format + Default,
{
    /// Read and decode the document without changing it on disk.
    pub async fn load(self) -> Result<PersistentStateManager<State, Formatter>, LoadError> {
        let bytes = fs::read(&self.config_path)
            .await
            .map_err(|e| LoadError::ReadConfig(e.into()))?;
        let state = self
            .formatter
            .deserialize(bytes.as_slice())
            .map_err(LoadError::DeserializeConfig)?;
        Ok(self.from_state(state))
    }

    /// Read and decode the document, using the default only when it is absent.
    pub async fn load_or_default(
        self,
    ) -> Result<PersistentStateManager<State, Formatter>, LoadError> {
        let state = match fs::read(&self.config_path).await {
            Ok(bytes) => self
                .formatter
                .deserialize(bytes.as_slice())
                .map_err(LoadError::DeserializeConfig)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(
                    target: "app",
                    path = %self.config_path,
                    "config file not found, using default"
                );
                State::default()
            }
            Err(e) => return Err(LoadError::ReadConfig(e.into())),
        };
        Ok(self.from_state(state))
    }

    /// Initialize version zero from a supplied state without writing a file.
    pub fn from_state(self, state: State) -> PersistentStateManager<State, Formatter> {
        PersistentStateManager {
            config_path: self.config_path,
            config_prefix: self.config_prefix,
            formatter: self.formatter,
            current_state: Arc::new(ArcSwap::from_pointee(VersionedState {
                version: Version::default(),
                state,
            })),
        }
    }
}

/// Whether a conditional replacement committed or found a newer version.
#[derive(Debug)]
pub enum ReplaceIfVersionResult {
    Replaced,
    Conflict { actual_version: Version },
}

/// A persistence, recovery or snapshot-publication failure.
#[derive(Debug, thiserror::Error)]
pub enum ReplaceIfVersionError {
    #[error("state change error: {0}")]
    State(#[from] StateChangedError),
    #[error("write config error: {0}")]
    WriteConfig(#[source] anyhow::Error),
    #[error("local write step failed before commit: {0}")]
    LocalWrite(#[source] anyhow::Error),
    #[error("persistence failed ({cause}) and resource recovery failed: {recovery_error}")]
    ResourceRecovery {
        cause: anyhow::Error,
        recovery_error: anyhow::Error,
    },
}

/// The local resource write and document write share a recovery step, but keep
/// their distinct error categories for the caller.
#[derive(Debug)]
enum ConditionalWriteError {
    LocalWrite(anyhow::Error),
    Config(anyhow::Error),
}

impl ConditionalWriteError {
    fn into_cause(self) -> anyhow::Error {
        match self {
            Self::LocalWrite(error) => error.context("local write failed"),
            Self::Config(error) => error.context("config write failed"),
        }
    }
}

type ConfigWrite = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

#[derive(Debug, Clone, Copy)]
enum ConfigWriteMode {
    Inline,
    Offloaded,
}

/// A source owner's document and read-only, versioned snapshots.
///
/// Writes publish their snapshot before returning success. Conditional writes
/// run on the owner's task; unconditional writes offload the atomic file write.
pub struct PersistentStateManager<State, Formatter = YamlFormat>
where
    State: Clone + Send + Sync + 'static,
{
    config_prefix: Option<String>,
    pub(crate) config_path: Utf8PathBuf,
    current_state: Arc<ArcSwap<VersionedState<State>>>,
    formatter: Formatter,
}

impl<State, Formatter> PersistentStateManager<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + Default + 'static,
    Formatter: Format,
{
    /// Clone the most recently committed value.
    pub fn snapshot(&self) -> Arc<State> {
        Arc::new(self.current_state.load().state.clone())
    }

    /// Return a read-only handle that observes future committed versions.
    pub fn snapshot_handle(&self) -> StateSnapshot<State> {
        StateSnapshot::from_store(Arc::clone(&self.current_state))
    }

    /// Persist an unconditional replacement, then publish its snapshot.
    pub async fn upsert(&mut self, state: State) -> Result<(), UpsertError>
    where
        Formatter: Clone,
    {
        let previous = self.current_state.load_full();
        let version = previous.version.next();
        self.write_config_step(ConfigWriteMode::Offloaded)(&state)
            .await
            .map_err(UpsertError::WriteConfig)?;
        self.publish(previous, version, state)
            .map_err(UpsertError::State)
    }

    fn write_config_step(
        &self,
        mode: ConfigWriteMode,
    ) -> impl FnOnce(&State) -> ConfigWrite + use<State, Formatter>
    where
        Formatter: Clone,
    {
        let formatter = self.formatter.clone();
        let config_path = self.config_path.clone();
        let config_prefix = self.config_prefix.clone();
        move |state| {
            let mut buf = Vec::with_capacity(4096);
            let serialized = formatter.serialize(&mut buf, state, config_prefix.as_deref());
            Box::pin(async move {
                serialized?;
                let file = AtomicFile::new(&config_path, AllowOverwrite);
                let written = match mode {
                    ConfigWriteMode::Inline => file.write(|f| f.write_all(&buf)),
                    ConfigWriteMode::Offloaded => {
                        tokio::task::spawn_blocking(move || file.write(|f| f.write_all(&buf)))
                            .await
                            .map_err(|error| match error.try_into_panic() {
                                Ok(panic) => std::panic::resume_unwind(panic),
                                Err(error) => error,
                            })?
                    }
                };
                written.with_context(|| format!("failed to write config: {config_path}"))?;
                Ok(())
            })
        }
    }

    /// Persist and publish the value only if the expected version is current.
    pub async fn replace_if_version(
        &mut self,
        expected_version: Version,
        next_state: State,
    ) -> Result<ReplaceIfVersionResult, ReplaceIfVersionError>
    where
        Formatter: Clone,
    {
        self.replace_if_version_with_local_write(
            expected_version,
            next_state,
            || async { Ok(()) },
            || async { Ok(()) },
        )
        .await
    }

    /// Persist local resources and the document only at `expected_version`.
    ///
    /// A write failure awaits local recovery before returning. A CAS failure
    /// after persistence preserves the winning snapshot and does not undo files.
    pub async fn replace_if_version_with_local_write<W, WFut, R, RFut>(
        &mut self,
        expected_version: Version,
        next_state: State,
        local_write: W,
        local_recovery: R,
    ) -> Result<ReplaceIfVersionResult, ReplaceIfVersionError>
    where
        Formatter: Clone,
        W: FnOnce() -> WFut,
        WFut: Future<Output = anyhow::Result<()>>,
        R: FnOnce() -> RFut,
        RFut: Future<Output = anyhow::Result<()>>,
    {
        let previous = self.current_state.load_full();
        if previous.version != expected_version {
            return Ok(ReplaceIfVersionResult::Conflict {
                actual_version: previous.version,
            });
        }

        let version = previous.version.next();
        let effect = self.write_config_step(ConfigWriteMode::Inline);
        let written = async {
            local_write()
                .await
                .map_err(ConditionalWriteError::LocalWrite)?;
            effect(&next_state)
                .await
                .map_err(ConditionalWriteError::Config)
        }
        .await;
        if let Err(error) = written {
            if let Err(recovery_error) = local_recovery().await {
                return Err(ReplaceIfVersionError::ResourceRecovery {
                    cause: error.into_cause(),
                    recovery_error,
                });
            }
            return Err(match error {
                ConditionalWriteError::LocalWrite(error) => {
                    ReplaceIfVersionError::LocalWrite(error)
                }
                ConditionalWriteError::Config(error) => ReplaceIfVersionError::WriteConfig(error),
            });
        }

        self.publish(previous, version, next_state)?;
        Ok(ReplaceIfVersionResult::Replaced)
    }

    fn publish(
        &mut self,
        previous: Arc<VersionedState<State>>,
        version: Version,
        state: State,
    ) -> Result<(), StateChangedError> {
        let next = Arc::new(VersionedState { version, state });
        let actual = self.current_state.compare_and_swap(&previous, next);
        if !Arc::ptr_eq(&actual, &previous) {
            return Err(StateChangedError::StateCasMismatch {
                expected: previous.version,
                actual: actual.version,
            });
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn state_store(&self) -> Arc<ArcSwap<VersionedState<State>>> {
        Arc::clone(&self.current_state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tempfile::tempdir;
    use tokio::fs;

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    struct TestState {
        name: String,
        value: i32,
    }

    impl TestState {
        fn new(name: String, value: i32) -> Self {
            Self { name, value }
        }
    }

    #[derive(Clone, Default)]
    struct FailingSerializeFormat;

    impl Format for FailingSerializeFormat {
        fn serialize<W: Write, T: Serialize>(
            &self,
            mut writer: W,
            _value: &T,
            _prefix: Option<&str>,
        ) -> anyhow::Result<()> {
            writer.write_all(b"partial document")?;
            Err(std::io::Error::other("serialization failed").into())
        }

        fn deserialize<R: std::io::Read, T: DeserializeOwned>(
            &self,
            reader: R,
        ) -> anyhow::Result<T> {
            YamlFormat.deserialize(reader)
        }
    }

    async fn create_temp_config_file(
        state: &TestState,
    ) -> anyhow::Result<(Utf8PathBuf, tempfile::TempDir)> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("test_config.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let yaml = yaml_serde::to_string(state)?;
        fs::write(&config_path, yaml).await?;
        Ok((config_path, temp_dir))
    }

    async fn read_yaml<T: DeserializeOwned>(path: &Utf8PathBuf) -> anyhow::Result<T> {
        let content = fs::read_to_string(path).await?;
        let value = yaml_serde::from_str(&content)?;
        Ok(value)
    }

    #[tokio::test]
    async fn test_setup_load_success() {
        let state = TestState::new("test".to_string(), 42);
        let (config_path, _temp_dir) = create_temp_config_file(&state).await.unwrap();

        let manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .load()
            .await
            .unwrap();

        let loaded = manager.snapshot();
        assert_eq!(loaded.name, "test");
        assert_eq!(loaded.value, 42);
    }

    #[tokio::test]
    async fn test_setup_load_file_not_exist() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let result = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .load()
            .await;

        assert!(result.is_err());
        let error_msg = result.err().unwrap().to_string();
        assert!(error_msg.contains("failed to read the config file"));
    }

    #[tokio::test]
    async fn test_setup_load_or_default_success() {
        let state = TestState::new("default_test".to_string(), 100);
        let (config_path, _temp_dir) = create_temp_config_file(&state).await.unwrap();

        let manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .load_or_default()
            .await
            .unwrap();

        let loaded = manager.snapshot();
        assert_eq!(loaded.name, "default_test");
        assert_eq!(loaded.value, 100);
    }

    #[tokio::test]
    async fn test_setup_load_or_default_file_not_exist() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .load_or_default()
            .await
            .unwrap();

        let loaded = manager.snapshot();
        assert_eq!(loaded.name, "");
        assert_eq!(loaded.value, 0);
    }

    #[tokio::test]
    async fn load_or_default_rejects_malformed_documents_without_changing_them() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("config.yaml")).unwrap();
        let bytes = b"name: [unterminated\n";
        fs::write(&path, bytes).await.unwrap();

        let result = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(path.clone())
            .assemble()
            .load_or_default()
            .await;

        let Err(LoadError::DeserializeConfig(error)) = result else {
            panic!("a malformed document must report a deserialize error");
        };
        assert!(error.downcast_ref::<yaml_serde::Error>().is_some());
        assert_eq!(fs::read(&path).await.unwrap(), bytes);
    }

    #[tokio::test]
    async fn load_or_default_rejects_a_directory_in_place_of_the_document() {
        let dir = tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("config.yaml")).unwrap();
        fs::create_dir(&path).await.unwrap();

        let result = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(path.clone())
            .assemble()
            .load_or_default()
            .await;

        let Err(LoadError::ReadConfig(error)) = result else {
            panic!("a directory must report a read error instead of a default value");
        };
        let source = error.downcast_ref::<std::io::Error>().unwrap();
        assert_ne!(source.kind(), std::io::ErrorKind::NotFound);
        assert!(path.is_dir());
    }

    #[tokio::test]
    async fn failed_local_write_and_recovery_report_both_causes() {
        let initial = TestState::new("committed".into(), 7);
        let (path, _dir) = create_temp_config_file(&initial).await.unwrap();
        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(path)
            .assemble()
            .from_state(initial.clone());
        let handle = manager.snapshot_handle();

        let result = manager
            .replace_if_version_with_local_write(
                Version::new(0),
                TestState::new("candidate".into(), 8),
                || async {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "local write denied",
                    )
                    .into())
                },
                || async { Err(std::io::Error::other("recovery failed").into()) },
            )
            .await;

        let Err(ReplaceIfVersionError::ResourceRecovery {
            cause,
            recovery_error,
        }) = result
        else {
            panic!("expected both persistence and recovery causes: {result:?}");
        };
        let write = cause.root_cause().downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(write.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(write.to_string(), "local write denied");
        let recovery = recovery_error.downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(recovery.to_string(), "recovery failed");
        assert_eq!(handle.load().version, Version::new(0));
        assert_eq!(handle.load().state, initial);
    }

    #[tokio::test]
    async fn serialization_failure_recovers_changed_resources_without_publishing() {
        let initial = TestState::new("committed".into(), 7);
        let (path, dir) = create_temp_config_file(&initial).await.unwrap();
        let mut manager =
            PersistentStateManagerSetup::<TestState, FailingSerializeFormat>::builder()
                .config_path(path)
                .assemble()
                .from_state(initial.clone());
        let handle = manager.snapshot_handle();
        let resource = dir.path().join("resource");
        fs::write(&resource, b"old").await.unwrap();

        let result = manager
            .replace_if_version_with_local_write(
                Version::new(0),
                TestState::new("candidate".into(), 8),
                || async {
                    fs::write(&resource, b"new").await?;
                    Ok(())
                },
                || async {
                    assert_eq!(fs::read(&resource).await.unwrap(), b"new");
                    fs::write(&resource, b"old").await?;
                    Ok(())
                },
            )
            .await;

        let Err(ReplaceIfVersionError::WriteConfig(error)) = result else {
            panic!("expected a serialization failure: {result:?}");
        };
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().to_string(),
            "serialization failed"
        );
        assert_eq!(fs::read(&resource).await.unwrap(), b"old");
        assert_eq!(handle.load().version, Version::new(0));
        assert_eq!(handle.load().state, initial);
    }

    #[tokio::test]
    async fn failed_replacement_preserves_existing_document_bytes() {
        let initial = TestState::new("committed".into(), 7);
        let (path, _dir) = create_temp_config_file(&initial).await.unwrap();
        let bytes = b"# preserve this comment\nname: committed\nvalue: 7\n";
        fs::write(&path, bytes).await.unwrap();
        let mut manager =
            PersistentStateManagerSetup::<TestState, FailingSerializeFormat>::builder()
                .config_path(path.clone())
                .assemble()
                .from_state(initial.clone());

        let result = manager
            .replace_if_version(Version::new(0), TestState::new("candidate".into(), 8))
            .await;

        let Err(ReplaceIfVersionError::WriteConfig(error)) = result else {
            panic!("expected a failed replacement: {result:?}");
        };
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().to_string(),
            "serialization failed"
        );
        assert_eq!(fs::read(&path).await.unwrap(), bytes);
        assert_eq!(manager.snapshot_handle().load().version, Version::new(0));
        assert_eq!(&*manager.snapshot(), &initial);
    }

    #[test]
    fn test_setup_from_state() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("from_state.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let state = TestState::new("from_state".to_string(), 99);
        let manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(state.clone());

        assert_eq!(&*manager.snapshot(), &state);
    }

    #[tokio::test]
    async fn test_upsert_success() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("upsert_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .config_prefix("# upsert test".to_string())
            .assemble()
            .from_state(TestState::default());

        let state = TestState::new("upsert".to_string(), 200);
        let result = manager.upsert(state).await;
        assert!(result.is_ok());

        let loaded = manager.snapshot();
        assert_eq!(loaded.name, "upsert");
        assert_eq!(loaded.value, 200);

        assert!(config_path.exists());
        let saved: TestState = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved.name, "upsert");
        assert_eq!(saved.value, 200);
    }

    #[tokio::test]
    async fn test_upsert_write_config_error_without_previous() {
        let config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(TestState::default());

        let state = TestState::new("write_fail".to_string(), 300);
        let result = manager.upsert(state).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            UpsertError::WriteConfig(_) => {}
            other => panic!("Expected UpsertError::WriteConfig, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_upsert_write_config_error_no_commit() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("rollback_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(TestState::new("initial".to_string(), 100));

        assert_eq!(manager.snapshot().name, "initial");

        manager.config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let new_state = TestState::new("new_value".to_string(), 200);
        let result = manager.upsert(new_state).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            UpsertError::WriteConfig(_) => {}
            other => panic!("Expected UpsertError::WriteConfig, got: {:?}", other),
        }

        let state = manager.snapshot();
        assert_eq!(state.name, "initial");
        assert_eq!(state.value, 100);
    }

    #[tokio::test]
    async fn test_multiple_upserts() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("multiple_upserts_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .config_prefix("# multiple upserts".to_string())
            .assemble()
            .from_state(TestState::default());

        let state1 = TestState::new("first".to_string(), 1);
        manager.upsert(state1).await.unwrap();
        let loaded1 = manager.snapshot();
        assert_eq!(loaded1.name, "first");
        assert_eq!(loaded1.value, 1);

        let state2 = TestState::new("second".to_string(), 2);
        manager.upsert(state2).await.unwrap();
        let loaded2 = manager.snapshot();
        assert_eq!(loaded2.name, "second");
        assert_eq!(loaded2.value, 2);

        let saved: TestState = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved.name, "second");
        assert_eq!(saved.value, 2);
    }

    #[tokio::test]
    async fn test_replace_if_version_success_advances_snapshot_version() {
        let temp_dir = tempdir().unwrap();
        let config_path = Utf8PathBuf::from_path_buf(temp_dir.path().join("replace.yaml")).unwrap();
        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .assemble()
            .from_state(TestState::default());

        let result = manager
            .replace_if_version(Version::new(0), TestState::new("next".into(), 1))
            .await
            .unwrap();
        assert!(matches!(result, ReplaceIfVersionResult::Replaced));
        assert_eq!(manager.snapshot_handle().load().version, Version::new(1));
        assert_eq!(
            read_yaml::<TestState>(&config_path).await.unwrap().name,
            "next"
        );
    }

    #[tokio::test]
    async fn test_replace_if_version_with_local_write_owns_the_write_and_its_recovery() {
        let temp_dir = tempdir().unwrap();
        let config_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("local_write.yaml")).unwrap();
        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .assemble()
            .from_state(TestState::default());
        let writes = Arc::new(AtomicUsize::new(0));
        let recoveries = Arc::new(AtomicUsize::new(0));

        let (wrote, recovered) = (writes.clone(), recoveries.clone());
        let result = manager
            .replace_if_version_with_local_write(
                Version::new(0),
                TestState::new("next".into(), 1),
                move || async move {
                    wrote.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
                move || async move {
                    recovered.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await
            .unwrap();
        assert!(matches!(result, ReplaceIfVersionResult::Replaced));
        assert_eq!(
            read_yaml::<TestState>(&config_path).await.unwrap().name,
            "next"
        );
        assert_eq!(
            (
                writes.load(Ordering::SeqCst),
                recoveries.load(Ordering::SeqCst)
            ),
            (1, 0)
        );

        let recovered = recoveries.clone();
        let result = manager
            .replace_if_version_with_local_write(
                Version::new(1),
                TestState::new("failed".into(), 2),
                || async { Err(anyhow::anyhow!("scripted local write failure")) },
                move || async move {
                    recovered.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await;
        assert!(matches!(result, Err(ReplaceIfVersionError::LocalWrite(_))));
        assert_eq!(recoveries.load(Ordering::SeqCst), 1);
        assert_eq!(manager.snapshot().name, "next");

        let wrote = writes.clone();
        let result = manager
            .replace_if_version_with_local_write(
                Version::new(0),
                TestState::new("stale".into(), 3),
                move || async move {
                    wrote.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
                || async { Ok(()) },
            )
            .await
            .unwrap();
        assert!(matches!(result, ReplaceIfVersionResult::Conflict { .. }));
        assert_eq!(
            writes.load(Ordering::SeqCst),
            1,
            "a conflict writes nothing"
        );
    }

    #[tokio::test]
    async fn test_replace_if_version_stale_version_returns_conflict() {
        let temp_dir = tempdir().unwrap();
        let config_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("replace_conflict.yaml")).unwrap();
        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .assemble()
            .from_state(TestState::default());
        manager
            .upsert(TestState::new("current".into(), 1))
            .await
            .unwrap();

        let result = manager
            .replace_if_version(Version::new(0), TestState::new("stale".into(), 2))
            .await
            .unwrap();
        assert!(matches!(
            result,
            ReplaceIfVersionResult::Conflict { actual_version }
                if actual_version == Version::new(1)
        ));
        assert_eq!(manager.snapshot().name, "current");
        assert_eq!(
            read_yaml::<TestState>(&config_path).await.unwrap().name,
            "current"
        );
    }

    #[tokio::test]
    async fn test_replace_if_version_persistence_failure_keeps_state_and_version() {
        let temp_dir = tempdir().unwrap();
        let config_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("replace_failure.yaml")).unwrap();
        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(TestState::new("initial".into(), 1));
        let missing_dir = tempdir().unwrap();
        let missing_path =
            Utf8PathBuf::from_path_buf(missing_dir.path().join("replace.yaml")).unwrap();
        missing_dir.close().unwrap();
        manager.config_path = missing_path;

        let result = manager
            .replace_if_version(Version::new(0), TestState::new("failed".into(), 2))
            .await;
        assert!(matches!(result, Err(ReplaceIfVersionError::WriteConfig(_))));
        assert_eq!(manager.snapshot().name, "initial");
        assert_eq!(manager.snapshot_handle().load().version, Version::new(0));
    }

    #[tokio::test]
    async fn test_replace_if_version_versions_are_monotonic() {
        let temp_dir = tempdir().unwrap();
        let config_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("replace_monotonic.yaml")).unwrap();
        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(TestState::default());

        let first = manager
            .replace_if_version(Version::new(0), TestState::new("one".into(), 1))
            .await
            .unwrap();
        assert!(matches!(first, ReplaceIfVersionResult::Replaced));
        assert_eq!(manager.snapshot_handle().load().version, Version::new(1));

        let second = manager
            .replace_if_version(Version::new(1), TestState::new("two".into(), 2))
            .await
            .unwrap();
        assert!(matches!(second, ReplaceIfVersionResult::Replaced));
        assert_eq!(manager.snapshot_handle().load().version, Version::new(2));
    }

    #[tokio::test]
    async fn test_config_prefix_in_saved_file() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("prefix_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let prefix = "# This is a test config\n# Do not edit manually";
        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .config_prefix(prefix.to_string())
            .assemble()
            .from_state(TestState::default());

        let state = TestState::new("prefix_test".to_string(), 500);
        manager.upsert(state).await.unwrap();

        let file_content = fs::read_to_string(&config_path).await.unwrap();
        assert!(file_content.starts_with("# This is a test config"));
        assert!(file_content.contains("# Do not edit manually"));
        assert!(file_content.contains("name: prefix_test"));
    }
}
