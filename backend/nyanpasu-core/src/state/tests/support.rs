//! Shared scaffolding for the persistence-settlement tests.

use std::{io::Write, sync::Arc};

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tempfile::TempDir;

use crate::{
    format::Format,
    state::{
        Ack, PersistentStateManager, PersistentStateManagerSetup, StateAckSubscriber, StateChange,
        SubscriberName, Version, VersionedState, coordinator::StateStore,
    },
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct TestState {
    pub(super) name: String,
    pub(super) value: i32,
}

impl TestState {
    pub(super) fn new(name: &str, value: i32) -> Self {
        Self {
            name: name.to_string(),
            value,
        }
    }
}

pub(super) const REFUSED_NAME: &str = "refused";

/// YAML, except that it refuses to write the state named `refused`.
///
/// Fails one specific write (the recovery) while letting the other (the
/// candidate) through, without touching the filesystem in between.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct RefuseNamedFormat;

impl Format for RefuseNamedFormat {
    fn serialize<W: Write, T: Serialize>(
        &self,
        mut writer: W,
        value: &T,
        prefix: Option<&str>,
    ) -> anyhow::Result<()> {
        let body = serde_yaml_ng::to_string(value)?;
        if body.contains(REFUSED_NAME) {
            anyhow::bail!("refusing to serialize the `{REFUSED_NAME}` state");
        }
        if let Some(prefix) = prefix {
            writeln!(writer, "{prefix}")?;
        }
        writer.write_all(body.as_bytes())?;
        Ok(())
    }

    fn deserialize<R: std::io::Read, T: DeserializeOwned>(&self, reader: R) -> anyhow::Result<T> {
        Ok(serde_yaml_ng::from_reader(reader)?)
    }
}

pub(super) fn config_path(dir: &TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(dir.path().join("state.yaml")).unwrap()
}

pub(super) async fn manager_with_formatter<F>(dir: &TempDir) -> PersistentStateManager<TestState, F>
where
    F: Format + Clone + Default,
{
    match PersistentStateManagerSetup::<TestState, F>::builder()
        .config_path(config_path(dir))
        .assemble()
        .from_state(TestState::default())
        .await
    {
        Ok(manager) => manager,
        Err(error) => panic!("the initial state must load: {error:?}"),
    }
}

pub(super) async fn manager_at(dir: &TempDir) -> PersistentStateManager<TestState> {
    manager_with_formatter(dir).await
}

pub(super) async fn read_config(path: &Utf8PathBuf) -> TestState {
    let content = tokio::fs::read_to_string(path).await.unwrap();
    serde_yaml_ng::from_str(&content).unwrap()
}

/// A registered subscriber that swaps the store from outside the coordinator
/// during prepare, so the transaction's compare-and-swap loses.
pub(super) struct StoreHijacker {
    pub(super) store: StateStore<TestState>,
    pub(super) winner: TestState,
    pub(super) winner_version: Version,
}

#[async_trait::async_trait]
impl StateAckSubscriber<TestState> for StoreHijacker {
    fn name(&self) -> SubscriberName<'_> {
        "hijacker".into()
    }

    async fn on_prepare(&self, _change: StateChange<TestState>) -> Ack {
        self.store.store(Arc::new(VersionedState {
            version: self.winner_version,
            state: self.winner.clone(),
        }));
        Ack::Ok
    }
}
