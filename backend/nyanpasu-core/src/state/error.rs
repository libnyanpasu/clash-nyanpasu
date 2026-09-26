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
    Init(Box<ManagerInitError<Manager>>),
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
    pub coordinator: Box<super::coordinator::StateCoordinator<T>>,
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
        (*self.coordinator, self.report)
    }
}

pub struct ManagerInitError<Manager> {
    pub manager: Box<Manager>,
    pub report: PrepareReport,
}

impl<Manager> ManagerInitError<Manager> {
    pub fn new(manager: Manager, report: PrepareReport) -> Self {
        Self {
            manager: Box::new(manager),
            report,
        }
    }

    pub fn into_parts(self) -> (Manager, PrepareReport) {
        (*self.manager, self.report)
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

/// What a failed persistence recovery left behind.
///
/// A recovery only runs after the candidate was already written outside the
/// store and the commit was then refused, so when the recovery itself fails the
/// external side keeps a state the store never accepted.
#[derive(Debug, Clone)]
pub struct InconsistentPersistence {
    /// The config file that still holds the rejected candidate.
    pub config_path: camino::Utf8PathBuf,
    /// Whether the caller's own local write step (for example publishing a
    /// staged resource) had already completed, so it still needs compensating.
    pub local_write_completed: bool,
}

impl fmt::Display for InconsistentPersistence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "config file {} holds a rejected candidate",
            self.config_path
        )?;
        if self.local_write_completed {
            write!(f, " and a completed local write still needs compensation")?;
        }
        Ok(())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum UpsertError {
    #[error("state changed error: {0}")]
    State(StateChangedError),
    #[error("write config error: {0}")]
    WriteConfig(anyhow::Error),
    #[error("persistence failed ({cause}) and resource recovery failed: {recovery_error}")]
    ResourceRecovery {
        cause: anyhow::Error,
        recovery_error: anyhow::Error,
    },
    #[error(
        "state commit failed ({commit_error}) and restoring the persisted state failed ({recovery_error}): {inconsistent}"
    )]
    Recovery {
        commit_error: StateChangedError,
        #[source]
        recovery_error: anyhow::Error,
        inconsistent: InconsistentPersistence,
    },
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

    #[error(
        "effect failed ({effect_error}) and restoring local resources failed: {recovery_error}"
    )]
    EffectRecovery { effect_error: E, recovery_error: E },

    /// The commit was refused after the effect had already published the
    /// candidate, and putting the committed state back failed as well. This is
    /// never a clean rejection: something outside the store is now wrong.
    #[error(
        "state commit failed ({commit_error}) and restoring the committed state failed: {recovery_error}"
    )]
    Recovery {
        commit_error: StateChangedError,
        recovery_error: E,
    },
}
