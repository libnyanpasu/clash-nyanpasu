use tokio::sync::RwLock;

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

#[async_trait::async_trait]
pub(crate) trait StateChangedSubscriber<T: Clone + Send + Sync> {
    /// The name of the subscriber.
    fn name(&self) -> &str;

    /// Called when the state is changed, return a Error if the state change is failed.
    ///
    /// While state migrate is failed, the rollback will be called.
    async fn migrate(&self, prev_state: T, new_state: T) -> Result<(), anyhow::Error>;
    /// Called when the state migrate is failed, return a Error if the state rollback is failed.
    async fn rollback(&self, prev_state: T, new_state: T) -> Result<(), anyhow::Error>;
}

pub trait StateSyncBuilder {
    type State: Clone + Send + Sync;

    fn build(&self) -> anyhow::Result<Self::State>;
}

pub trait StateAsyncBuilder {
    type State: Clone + Send + Sync;

    async fn build(&self) -> anyhow::Result<Self::State>;
}

impl<T, S> StateAsyncBuilder for S
where
    S: StateSyncBuilder<State = T>,
    T: Clone + Send + Sync,
{
    type State = T;
    async fn build(&self) -> anyhow::Result<Self::State> {
        self.build()
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConcurrencyStrategy {
    #[default]
    Sequential,
    Concurrent,
    Limited(usize),
}

pub struct ServiceCoordinator<T: Clone + Send + Sync> {
    current_state: RwLock<T>,
    subscribers: Vec<Box<dyn StateChangedSubscriber<T> + Send + Sync>>,
    // strategy: ConcurrencyStrategy,
}

impl<T: Clone + Send + Sync> ServiceCoordinator<T> {
    pub fn new(state: T) -> Self {
        Self {
            current_state: RwLock::new(state),
            subscribers: Vec::new(),
        }
    }

    async fn run_migration<S>(
        subscriber: &S,
        current_state: &T,
        new_state: &T,
    ) -> Result<(), StateChangedError>
    where
        S: StateChangedSubscriber<T> + Send + Sync + ?Sized,
    {
        if let Err(e) = subscriber
            .migrate(current_state.clone(), new_state.clone())
            .await
        {
            let migrate_error = MigrateError {
                name: subscriber.name().to_string(),
                error: e,
            };
            tracing::error!("migrate error: {migrate_error:#?}");
            if let Err(e) = subscriber
                .rollback(current_state.clone(), new_state.clone())
                .await
            {
                tracing::error!("rollback error: {e:#?}");
                return Err(StateChangedError::MigrateAndRollback(
                    migrate_error,
                    RollbackError {
                        name: subscriber.name().to_string(),
                        error: e,
                    },
                ));
            }
            return Err(StateChangedError::Migrate(migrate_error));
        }
        Ok(())
    }

    pub async fn upsert(
        &mut self,
        builder: impl StateAsyncBuilder<State = T>,
    ) -> Result<(), StateChangedError> {
        let mut current_state = self.current_state.write().await;
        let new_state = builder
            .build()
            .await
            .map_err(StateChangedError::Validation)?;

        for subscriber in self.subscribers.iter() {
            Self::run_migration(subscriber.as_ref(), &current_state, &new_state).await?;
        }

        *current_state = new_state;
        Ok(())
    }
}
