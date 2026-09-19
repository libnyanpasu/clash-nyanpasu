use anyhow::Context;
use atomicwrites::{AllowOverwrite, AtomicFile};
use bon::Builder;
use camino::Utf8PathBuf;
use fs_err::tokio as fs;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    future::Future,
    io::Write,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{super::error::*, *};

use crate::{
    format::{Format, YamlFormat},
    state::{
        DecisionHandle, PrepareReport, StateParticipant, Version,
        coordinator::{ParticipantEntry, PendingOutcome},
    },
};

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
    state_coordinator: StateCoordinatorBuilder<State>,
    #[builder(default)]
    formatter: Formatter,
    #[builder(default)]
    force_build: bool,
}

impl<State, Formatter> PersistentStateManagerSetup<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + Default + 'static,
    Formatter: Format + Clone + Default,
{
    async fn build_manager(
        self,
        state: State,
    ) -> Result<
        PersistentStateManager<State, Formatter>,
        ManagerInitError<PersistentStateManager<State, Formatter>>,
    > {
        let Self {
            config_path,
            config_prefix,
            state_coordinator,
            formatter,
            force_build,
        } = self;

        let build_result = state_coordinator.build_initialized(state).await;
        let make_manager = |coordinator| PersistentStateManager {
            config_prefix,
            config_path,
            state_coordinator: coordinator,
            formatter,
        };

        match build_result {
            Ok(coordinator) => Ok(make_manager(coordinator)),
            Err(error) => {
                let (coordinator, report) = error.into_parts();
                let manager = make_manager(coordinator);
                if force_build {
                    Ok(manager)
                } else {
                    Err(ManagerInitError::new(manager, report))
                }
            }
        }
    }

    pub async fn load(
        self,
    ) -> Result<
        PersistentStateManager<State, Formatter>,
        LoadError<PersistentStateManager<State, Formatter>>,
    > {
        let bytes = fs::read(&self.config_path)
            .await
            .map_err(|e| LoadError::ReadConfig(e.into()))?;
        let state: State = self
            .formatter
            .deserialize(bytes.as_slice())
            .map_err(LoadError::DeserializeConfig)?;

        self.build_manager(state)
            .await
            .map_err(|e| LoadError::Init(Box::new(e)))
    }

    pub async fn load_or_default(
        self,
    ) -> Result<
        PersistentStateManager<State, Formatter>,
        LoadError<PersistentStateManager<State, Formatter>>,
    > {
        let state: State = match fs::read(&self.config_path).await {
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

        self.build_manager(state)
            .await
            .map_err(|e| LoadError::Init(Box::new(e)))
    }

    pub async fn from_state(
        self,
        state: State,
    ) -> Result<
        PersistentStateManager<State, Formatter>,
        LoadError<PersistentStateManager<State, Formatter>>,
    > {
        self.build_manager(state)
            .await
            .map_err(|e| LoadError::Init(Box::new(e)))
    }
}

#[derive(Debug)]
pub enum ReplaceIfVersionResult {
    Replaced,
    Conflict { actual_version: Version },
}

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
    #[error(
        "state commit failed ({commit_error}) and restoring the committed state failed \
         ({recovery_error}): {inconsistent}"
    )]
    Recovery {
        commit_error: StateChangedError,
        #[source]
        recovery_error: anyhow::Error,
        /// What the failed recovery left behind. This is what makes a recovery
        /// failure distinguishable from a clean rejection.
        inconsistent: InconsistentPersistence,
    },
}

/// Which write inside one conditional replacement failed.
///
/// The caller's local write step and the config write share one transaction, so
/// they share one error channel; this keeps them apart on the way out.
#[derive(Debug)]
enum ConditionalWriteError {
    LocalWrite(anyhow::Error),
    Config(anyhow::Error),
}

impl std::fmt::Display for ConditionalWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LocalWrite(error) => write!(f, "local write failed: {error}"),
            Self::Config(error) => write!(f, "config write failed: {error}"),
        }
    }
}

impl ConditionalWriteError {
    fn into_inner(self) -> anyhow::Error {
        match self {
            Self::LocalWrite(error) | Self::Config(error) => error,
        }
    }
}

/// The deferred half of a config write: the bytes are already serialized, only
/// the file write is left.
type ConfigWrite = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

/// Where the config file write runs.
///
/// These are the two behaviours this manager has always had, and they are kept
/// as they were: `upsert` offloads the write, conditional replacement runs it on
/// the calling task. Offloading conditional replacement too would change when a
/// commit becomes observable on a path this change was not asked to touch.
///
/// Within one entry point the effect and its recovery always use the same mode.
#[derive(Debug, Clone, Copy)]
enum ConfigWriteMode {
    /// Write on the current task.
    Inline,
    /// Write on a blocking thread, so a multi-syscall file write never stalls
    /// the runtime. The work runs to completion even if the caller is dropped.
    Offloaded,
}

pub struct PersistentStateManager<State, Formatter = YamlFormat>
where
    State: Clone + Send + Sync + 'static,
{
    config_prefix: Option<String>,
    pub(crate) config_path: Utf8PathBuf,
    state_coordinator: StateCoordinator<State>,
    formatter: Formatter,
}

impl<State, Formatter> PersistentStateManager<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + Default + 'static,
    Formatter: Format,
{
    super::impl_state_manager_delegates!(State);

    /// Replace the state unconditionally and persist it.
    ///
    /// This is the same conditional-replacement transaction as
    /// [`PersistentStateManager::replace_if_version`] with no version
    /// precondition, so a commit that loses a race still rewrites the config
    /// file back to the state the store actually holds.
    pub async fn upsert(&mut self, state: State) -> Result<PrepareReport, UpsertError>
    where
        Formatter: Clone,
    {
        let config_path = self.config_path.clone();
        let (effect, recovery) = self.config_write_steps(ConfigWriteMode::Offloaded);
        self.state_coordinator
            .with_pending_state(&state, effect, |committed| async move {
                recovery(&committed).await
            })
            .await
            .map(|((), report)| report)
            .map_err(|error| match error {
                WithEffectError::State(error) => UpsertError::State(error),
                WithEffectError::Effect(error) => UpsertError::WriteConfig(error),
                WithEffectError::EffectTimedOut(timeout) => UpsertError::WriteConfig(
                    anyhow::anyhow!("write config timed out after {timeout:?}"),
                ),
                WithEffectError::EffectRecovery {
                    effect_error,
                    recovery_error,
                } => UpsertError::ResourceRecovery {
                    cause: anyhow::anyhow!("{effect_error}"),
                    recovery_error,
                },
                WithEffectError::Recovery {
                    commit_error,
                    recovery_error,
                } => UpsertError::Recovery {
                    commit_error,
                    recovery_error,
                    inconsistent: InconsistentPersistence {
                        config_path,
                        local_write_completed: false,
                    },
                },
            })
    }

    /// A self-contained "write the config file" step, detached from `&self` so
    /// it can be moved into the transaction's effect and recovery closures.
    ///
    /// Serialization always happens on the calling task because it is in-memory
    /// and cheap; `mode` only decides where the file write itself runs.
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
                            .await?
                    }
                };
                written.with_context(|| format!("failed to write config: {config_path}"))?;
                Ok(())
            })
        }
    }

    fn config_write_steps(
        &self,
        mode: ConfigWriteMode,
    ) -> (
        impl FnOnce(&State) -> ConfigWrite + use<State, Formatter>,
        impl FnOnce(&State) -> ConfigWrite + use<State, Formatter>,
    )
    where
        Formatter: Clone,
    {
        let effect = self.write_config_step(mode);
        let recovery = self.write_config_step(mode);
        let written = Arc::new(AtomicBool::new(false));
        let completed = written.clone();
        (
            move |state| {
                let write = effect(state);
                Box::pin(async move {
                    write.await?;
                    completed.store(true, Ordering::Release);
                    Ok(())
                }) as ConfigWrite
            },
            move |state| {
                if written.load(Ordering::Acquire) {
                    recovery(state)
                } else {
                    Box::pin(async { Ok(()) }) as ConfigWrite
                }
            },
        )
    }

    pub async fn replace_if_version(
        &mut self,
        expected_version: Version,
        next_state: State,
    ) -> Result<ReplaceIfVersionResult, ReplaceIfVersionError>
    where
        Formatter: Clone,
    {
        let config_path = self.config_path.clone();
        let (effect, recovery) = self.config_write_steps(ConfigWriteMode::Inline);
        let outcome = self
            .state_coordinator
            .with_pending_state_if_version(
                expected_version,
                &next_state,
                |state| async move { effect(state).await.map_err(ConditionalWriteError::Config) },
                |committed| async move {
                    recovery(&committed)
                        .await
                        .map_err(ConditionalWriteError::Config)
                },
            )
            .await;
        Self::map_conditional_outcome(outcome, config_path, false)
    }

    /// Replace the state only if the store still holds `expected_version`, with
    /// one extra participant taking part in this transaction.
    ///
    /// `participant` is built from the transaction's own read-only
    /// [`DecisionHandle`], so it can still tell what the transaction decided
    /// after its `on_committed` notification was dropped or timed out. It joins
    /// the permanently registered subscribers of the same transaction: same
    /// prepare fan-out, same required-failure rollback, same notifications. It
    /// is never registered on the coordinator, so a cancelled replacement
    /// cannot leave a subscription behind.
    ///
    /// Necessary local writes and their recovery are owned by this transaction.
    /// Recovery must settle partial writes as well as completed publications;
    /// the authoritative abort is published only after it returns.
    pub async fn replace_if_version_with_participant<P, W, WFut, R, RFut>(
        &mut self,
        expected_version: Version,
        next_state: State,
        participant: P,
        local_write: W,
        local_recovery: R,
    ) -> Result<ReplaceIfVersionResult, ReplaceIfVersionError>
    where
        Formatter: Clone + Send + 'static,
        P: FnOnce(DecisionHandle) -> StateParticipant<State>,
        W: FnOnce() -> WFut + Send + 'static,
        WFut: Future<Output = anyhow::Result<()>> + Send + 'static,
        R: FnOnce() -> RFut + Send + 'static,
        RFut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let config_path = self.config_path.clone();
        let effect = self.write_config_step(ConfigWriteMode::Inline);
        let recovery = self.write_config_step(ConfigWriteMode::Inline);
        let local_write_completed = Arc::new(AtomicBool::new(false));
        let local_write_flag = Arc::clone(&local_write_completed);
        let config_written = Arc::new(AtomicBool::new(false));
        let config_written_effect = config_written.clone();
        let mut owner = self.state_coordinator.persistence_owner();
        let participant = ParticipantEntry::new(participant);
        let (completion, result) = tokio::sync::oneshot::channel();
        let (_caller_lifetime, caller) = tokio::sync::watch::channel(());
        let mut cancelled = caller.clone();
        let started = Arc::new(AtomicBool::new(false));
        let started_effect = started.clone();
        let decision = participant.decision_writer();
        // The source owner outlives its caller: dropping a waiter cannot drop an
        // in-flight write, release its permit, or race its compensation.
        tokio::spawn(async move {
            let operation = owner.with_pending_state_if_version_with_participant(
                expected_version,
                &next_state,
                participant,
                |state| async move {
                    if caller.has_changed().is_err() {
                        return Err(ConditionalWriteError::LocalWrite(anyhow::anyhow!(
                            "caller cancelled before persistence"
                        )));
                    }
                    started_effect.store(true, Ordering::Release);
                    local_write()
                        .await
                        .map_err(ConditionalWriteError::LocalWrite)?;
                    local_write_flag.store(true, Ordering::SeqCst);
                    if caller.has_changed().is_err() {
                        return Err(ConditionalWriteError::LocalWrite(anyhow::anyhow!(
                            "caller cancelled during persistence"
                        )));
                    }
                    effect(state).await.map_err(ConditionalWriteError::Config)?;
                    config_written_effect.store(true, Ordering::Release);
                    Ok(())
                },
                |committed| async move {
                    // Try every necessary recovery even when another fails.
                    let local = local_recovery().await;
                    let config = if config_written.load(Ordering::Acquire) {
                        recovery(&committed).await
                    } else {
                        Ok(())
                    };
                    match (local, config) {
                        (Ok(()), Ok(())) => Ok(()),
                        (local, config) => Err(ConditionalWriteError::LocalWrite(anyhow::anyhow!(
                            "local recovery: {local:?}; config recovery: {config:?}"
                        ))),
                    }
                },
            );
            let mut operation = Box::pin(operation);
            let outcome = tokio::select! {
                biased;
                outcome = &mut operation => outcome,
                _ = cancelled.changed() => {
                    if !started.load(Ordering::Acquire) {
                        drop(operation);
                        decision.abort(crate::state::AbortResourceState::Restored);
                        return;
                    }
                    // Finish the write, then the effect's cancellation check sends
                    // it through resource recovery before any abort is published.
                    operation.await
                }
            };
            let outcome = Self::map_conditional_outcome(
                outcome,
                config_path,
                local_write_completed.load(Ordering::SeqCst),
            );
            let _ = completion.send(outcome);
        });
        result.await.map_err(|error| {
            ReplaceIfVersionError::LocalWrite(anyhow::anyhow!(
                "source persistence owner failed: {error}"
            ))
        })?
    }

    fn map_conditional_outcome(
        outcome: Result<PendingOutcome<()>, WithEffectError<ConditionalWriteError>>,
        config_path: Utf8PathBuf,
        local_write_completed: bool,
    ) -> Result<ReplaceIfVersionResult, ReplaceIfVersionError> {
        match outcome {
            Ok(PendingOutcome::Committed { .. }) => Ok(ReplaceIfVersionResult::Replaced),
            Ok(PendingOutcome::Conflict { actual }) => Ok(ReplaceIfVersionResult::Conflict {
                actual_version: actual,
            }),
            Err(WithEffectError::State(error)) => Err(ReplaceIfVersionError::State(error)),
            Err(WithEffectError::Effect(ConditionalWriteError::LocalWrite(error))) => {
                Err(ReplaceIfVersionError::LocalWrite(error))
            }
            Err(WithEffectError::Effect(ConditionalWriteError::Config(error))) => {
                Err(ReplaceIfVersionError::WriteConfig(error))
            }
            Err(WithEffectError::EffectTimedOut(timeout)) => {
                Err(ReplaceIfVersionError::WriteConfig(anyhow::anyhow!(
                    "write timed out after {timeout:?}"
                )))
            }
            Err(WithEffectError::EffectRecovery {
                effect_error,
                recovery_error,
            }) => Err(ReplaceIfVersionError::ResourceRecovery {
                cause: anyhow::anyhow!("{effect_error}"),
                recovery_error: recovery_error.into_inner(),
            }),
            Err(WithEffectError::Recovery {
                commit_error,
                recovery_error,
            }) => Err(ReplaceIfVersionError::Recovery {
                commit_error,
                recovery_error: recovery_error.into_inner(),
                inconsistent: InconsistentPersistence {
                    config_path,
                    local_write_completed,
                },
            }),
        }
    }

    /// The raw store behind this manager, so tests can simulate a writer that
    /// bypassed the coordinator and force a CAS mismatch.
    #[cfg(test)]
    pub(crate) fn state_store(&self) -> crate::state::coordinator::StateStore<State> {
        self.state_coordinator.state_store()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Ack, StateAckSubscriber, StateChange, SubscriberName};
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

    async fn create_temp_config_file(
        state: &TestState,
    ) -> anyhow::Result<(Utf8PathBuf, tempfile::TempDir)> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("test_config.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let yaml = serde_yaml_ng::to_string(state)?;
        fs::write(&config_path, yaml).await?;
        Ok((config_path, temp_dir))
    }

    async fn read_yaml<T: DeserializeOwned>(path: &Utf8PathBuf) -> anyhow::Result<T> {
        let content = fs::read_to_string(path).await?;
        let value = serde_yaml_ng::from_str(&content)?;
        Ok(value)
    }

    struct FailingInitSubscriber {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<TestState> for FailingInitSubscriber {
        fn name(&self) -> SubscriberName<'_> {
            "failing_init".into()
        }

        async fn on_prepare(&self, _change: StateChange<TestState>) -> Ack {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ack::Failed(anyhow::anyhow!("init ACK failed"))
        }
    }

    fn failing_builder(calls: Arc<AtomicUsize>) -> StateCoordinatorBuilder<TestState> {
        StateCoordinatorBuilder::default()
            .with_subscriber(Box::new(FailingInitSubscriber { calls }))
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
    async fn test_setup_from_state() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("from_state.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let state = TestState::new("from_state".to_string(), 99);
        let manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(state.clone())
            .await
            .unwrap();

        assert_eq!(&*manager.snapshot(), &state);
    }

    #[tokio::test]
    async fn test_from_state_ack_failure_returns_recoverable_manager() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("from_state_ack_fail.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let state = TestState::new("committed".to_string(), 99);

        let result = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .state_coordinator(failing_builder(Arc::clone(&calls)))
            .assemble()
            .from_state(state.clone())
            .await;

        match result {
            Err(LoadError::Init(error)) => {
                let (manager, report) = error.into_parts();
                assert!(report.has_required_failures());
                assert_eq!(&*manager.snapshot(), &state);
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            }
            Err(error) => panic!("expected init ACK error, got {error}"),
            Ok(_) => panic!("expected recoverable init ACK error"),
        }
    }

    #[tokio::test]
    async fn test_force_build_returns_manager_after_ack_failure() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("force_build_ack_fail.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let state = TestState::new("forced".to_string(), 123);

        let manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .state_coordinator(failing_builder(Arc::clone(&calls)))
            .force_build(true)
            .assemble()
            .from_state(state.clone())
            .await
            .unwrap();

        assert_eq!(&*manager.snapshot(), &state);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
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
            .from_state(TestState::default())
            .await
            .unwrap();

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
            .from_state(TestState::default())
            .await
            .unwrap();

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
            .from_state(TestState::new("initial".to_string(), 100))
            .await
            .unwrap();

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
            .from_state(TestState::default())
            .await
            .unwrap();

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
            .from_state(TestState::default())
            .await
            .unwrap();

        let result = manager
            .replace_if_version(Version::new(0), TestState::new("next".into(), 1))
            .await
            .unwrap();
        assert!(matches!(result, ReplaceIfVersionResult::Replaced));
        assert_eq!(
            manager.state_coordinator.snapshot_versioned().version,
            Version::new(1)
        );
        assert_eq!(
            read_yaml::<TestState>(&config_path).await.unwrap().name,
            "next"
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
            .from_state(TestState::default())
            .await
            .unwrap();
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
            .from_state(TestState::new("initial".into(), 1))
            .await
            .unwrap();
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
        assert_eq!(
            manager.state_coordinator.snapshot_versioned().version,
            Version::new(0)
        );
    }

    #[tokio::test]
    async fn test_replace_if_version_versions_are_monotonic() {
        let temp_dir = tempdir().unwrap();
        let config_path =
            Utf8PathBuf::from_path_buf(temp_dir.path().join("replace_monotonic.yaml")).unwrap();
        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(TestState::default())
            .await
            .unwrap();

        let first = manager
            .replace_if_version(Version::new(0), TestState::new("one".into(), 1))
            .await
            .unwrap();
        assert!(matches!(first, ReplaceIfVersionResult::Replaced));
        assert_eq!(
            manager.state_coordinator.snapshot_versioned().version,
            Version::new(1)
        );

        let second = manager
            .replace_if_version(Version::new(1), TestState::new("two".into(), 2))
            .await
            .unwrap();
        assert!(matches!(second, ReplaceIfVersionResult::Replaced));
        assert_eq!(
            manager.state_coordinator.snapshot_versioned().version,
            Version::new(2)
        );
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
            .from_state(TestState::default())
            .await
            .unwrap();

        let state = TestState::new("prefix_test".to_string(), 500);
        manager.upsert(state).await.unwrap();

        let file_content = fs::read_to_string(&config_path).await.unwrap();
        assert!(file_content.starts_with("# This is a test config"));
        assert!(file_content.contains("# Do not edit manually"));
        assert!(file_content.contains("name: prefix_test"));
    }
}
