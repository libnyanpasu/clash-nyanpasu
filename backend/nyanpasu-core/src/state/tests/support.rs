//! Shared scaffolding for the persistence-settlement tests.

use std::sync::Arc;

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
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
