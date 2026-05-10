use super::{ack::PrepareReport, version::Version};
use std::{fmt, time::Duration};
#[derive(thiserror::Error, Debug)]
#[error("state prepared but required subscriber ACK failed")]
pub struct PrepareAckError {
    pub report: PrepareReport,
}

#[derive(thiserror::Error, Debug)]
pub enum StateChangedError {
    #[error("builder validation error: {0}")]
    Validation(anyhow::Error),

    #[error("state pre-commit but required subscriber ACK failed: {0}")]
    PrepareAck(PrepareAckError),
    /// This error indicates that the state has been updated optimistically, but the commit failed due to required subscriber ACK failures. The caller should check the current state and decide whether to retry or not.
    #[error(
        "state commit failed due to cas mismatch: expected version {expected}, but actual version is {actual}"
    )]
    StateCasMismatch { expected: Version, actual: Version },
}

impl StateChangedError {
    pub fn is_precommit(&self) -> bool {
        matches!(self, StateChangedError::PrepareAck(_))
    }
}

#[derive(thiserror::Error)]
pub enum LoadError<Manager = ()> {
    #[error("failed to read the config file: {0}")]
    ReadConfig(anyhow::Error),
    #[error("failed to upsert the state: {0}")]
    Upsert(StateChangedError),
    #[error("failed to deserialize the config file: {0}")]
    DeserializeConfig(anyhow::Error),
    #[error("state manager initialization ACK failed: {0}")]
    Init(ManagerInitError<Manager>),
}

impl<Manager> fmt::Debug for LoadError<Manager> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadConfig(error) => f.debug_tuple("ReadConfig").field(error).finish(),
            Self::Upsert(error) => f.debug_tuple("Upsert").field(error).finish(),
            Self::DeserializeConfig(error) => {
                f.debug_tuple("DeserializeConfig").field(error).finish()
            }
            Self::Init(error) => f.debug_tuple("Init").field(error).finish(),
        }
    }
}

#[derive(thiserror::Error)]
#[error("state prepared but required subscriber ACK failed during initialization")]
pub struct InitAckError<T: Clone + Send + Sync + 'static> {
    pub coordinator: super::coordinator::StateCoordinator<T>,
    pub report: PrepareReport,
}

impl<T: Clone + Send + Sync + 'static> std::fmt::Debug for InitAckError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InitAckError")
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

impl<T: Clone + Send + Sync + 'static> InitAckError<T> {
    pub fn into_parts(self) -> (super::coordinator::StateCoordinator<T>, PrepareReport) {
        (self.coordinator, self.report)
    }
}

pub struct ManagerInitError<Manager> {
    pub manager: Manager,
    pub report: PrepareReport,
}

impl<Manager> ManagerInitError<Manager> {
    pub fn new(manager: Manager, report: PrepareReport) -> Self {
        Self { manager, report }
    }

    pub fn into_parts(self) -> (Manager, PrepareReport) {
        (self.manager, self.report)
    }
}

impl<Manager> fmt::Debug for ManagerInitError<Manager> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ManagerInitError")
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

impl<Manager> fmt::Display for ManagerInitError<Manager> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "state manager initialized but required subscriber ACK failed"
        )
    }
}

impl<Manager> std::error::Error for ManagerInitError<Manager> {}

#[derive(thiserror::Error, Debug)]
pub enum UpsertError {
    #[error("state changed error: {0}")]
    State(StateChangedError),
    #[error("write config error: {0}")]
    WriteConfig(anyhow::Error),
}

impl UpsertError {
    pub fn is_precommit(&self) -> bool {
        matches!(self, UpsertError::State(s) if s.is_precommit())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum WithEffectError<E> {
    #[error("state commit failed: {0}")]
    State(StateChangedError),

    #[error("effect failed: {0}")]
    Effect(E),

    #[error("effect timed out after {0:?}")]
    EffectTimedOut(Duration),
}
