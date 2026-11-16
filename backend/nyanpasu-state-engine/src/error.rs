//! Persistence and publication failures returned to source owners.

use crate::Version;

#[derive(thiserror::Error, Debug)]
pub enum StateChangedError {
    #[error(
        "state commit failed due to cas mismatch: expected version {expected}, but actual version is {actual}"
    )]
    StateCasMismatch { expected: Version, actual: Version },
}

#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("failed to read the config file")]
    ReadConfig(#[source] anyhow::Error),
    #[error("failed to deserialize the config file")]
    DeserializeConfig(#[source] anyhow::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum UpsertError {
    #[error("state changed error: {0}")]
    State(StateChangedError),
    #[error("write config error: {0}")]
    WriteConfig(anyhow::Error),
}
