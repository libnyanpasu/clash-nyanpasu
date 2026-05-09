use super::ack::CommitReport;

#[derive(thiserror::Error, Debug)]
#[error("state committed but required subscriber ACK failed")]
pub struct CommitAckError {
    pub report: CommitReport,
}

#[derive(thiserror::Error, Debug)]
pub enum StateChangedError {
    #[error("builder validation error: {0}")]
    Validation(anyhow::Error),

    #[error("state committed but required subscriber ACK failed: {0}")]
    CommitAck(CommitAckError),
}

impl StateChangedError {
    pub fn is_post_commit(&self) -> bool {
        matches!(self, StateChangedError::CommitAck(_))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("failed to read the config file: {0}")]
    ReadConfig(anyhow::Error),
    #[error("failed to upsert the state: {0}")]
    Upsert(StateChangedError),
    #[error("failed to deserialize the config file: {0}")]
    DeserializeConfig(anyhow::Error),
}

#[derive(thiserror::Error)]
#[error("state committed but required subscriber ACK failed during initialization")]
pub struct InitAckError<T: Clone + Send + Sync + 'static> {
    pub coordinator: super::coordinator::StateCoordinator<T>,
    pub report: CommitReport,
}

impl<T: Clone + Send + Sync + 'static> std::fmt::Debug for InitAckError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InitAckError")
            .field("report", &self.report)
            .finish_non_exhaustive()
    }
}

impl<T: Clone + Send + Sync + 'static> InitAckError<T> {
    pub fn into_parts(self) -> (super::coordinator::StateCoordinator<T>, CommitReport) {
        (self.coordinator, self.report)
    }
}

impl<T: Clone + Send + Sync + 'static> From<InitAckError<T>> for StateChangedError {
    fn from(e: InitAckError<T>) -> Self {
        StateChangedError::CommitAck(CommitAckError { report: e.report })
    }
}

impl<T: Clone + Send + Sync + 'static> From<InitAckError<T>> for LoadError {
    fn from(e: InitAckError<T>) -> Self {
        LoadError::Upsert(StateChangedError::from(e))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum UpsertError {
    #[error("state changed error: {0}")]
    State(StateChangedError),
    #[error("write config error: {0}")]
    WriteConfig(anyhow::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum WithEffectError<E> {
    #[error("state migration failed: {0}")]
    State(StateChangedError),

    #[error("effect failed: {0}")]
    Effect(E),
}
