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
}

#[derive(thiserror::Error, Debug)]
pub enum WriteError {
    #[error("failed to write the config file: {0}")]
    WriteConfig(anyhow::Error),
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

    #[error("effect failed and rollback also failed: effect={effect}, rollback={rollback}")]
    EffectAndRollback { effect: E, rollback: StateChangedError },
}
