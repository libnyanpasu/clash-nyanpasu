#[derive(thiserror::Error, Debug)]
#[error("state migrate error: {name}: {error:#?}")]
pub struct MigrateError {
    pub name: String,
    pub error: anyhow::Error,
}

#[derive(thiserror::Error, Debug)]
#[error("state rollback error: {name}: {error:#?}")]
pub struct RollbackError {
    pub name: String,
    pub error: anyhow::Error,
}

#[derive(thiserror::Error, Debug)]
pub enum StateChangedError {
    #[error("builder validation error: {0}")]
    Validation(anyhow::Error),
    #[error("state migrate error: {0:#?}")]
    Migrate(#[from] MigrateError),

    #[error("state migrate and rollback error: migrate {0:#?}, rollback {1:#?}")]
    MigrateAndRollback(MigrateError, RollbackError),
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
