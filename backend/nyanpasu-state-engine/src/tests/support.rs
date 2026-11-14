//! Shared scaffolding for the persistence-settlement tests.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use crate::{PersistentStateManager, PersistentStateManagerSetup};

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

pub(super) fn manager_at(dir: &TempDir) -> PersistentStateManager<TestState> {
    PersistentStateManagerSetup::<TestState>::builder()
        .config_path(config_path(dir))
        .assemble()
        .from_state(TestState::default())
}

pub(super) async fn read_config(path: &Utf8PathBuf) -> TestState {
    let content = tokio::fs::read_to_string(path).await.unwrap();
    yaml_serde::from_str(&content).unwrap()
}
