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
#[allow(unused_variables)]
pub(crate) trait StateChangedSubscriber<T: Clone + Send + Sync + 'static> {
    /// The name of the subscriber.
    fn name(&self) -> &str;

    /// Called when the state is changed, return a Error if the state change is failed.
    ///
    /// While state migrate is failed, the rollback will be called.
    ///
    /// When the prev_state is None, it means the state is not initialized.
    async fn migrate(&self, prev_state: Option<T>, new_state: T) -> Result<(), anyhow::Error>;

    /// Called when the state migrate is failed, return a Error if the state rollback is failed.
    ///
    /// If the migration do not affect the real system/service, you can use the default implementation,
    /// OR you MUST implement the rollback method.
    async fn rollback(&self, prev_state: Option<T>, new_state: T) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

pub trait StateSyncBuilder {
    type State: Clone + Send + Sync + 'static;

    fn build(&self) -> anyhow::Result<Self::State>;
}

pub trait StateAsyncBuilder {
    type State: Clone + Send + Sync + 'static;

    async fn build(&self) -> anyhow::Result<Self::State>;
}

impl<T, S> StateAsyncBuilder for S
where
    S: StateSyncBuilder<State = T>,
    T: Clone + Send + Sync + 'static,
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

pub struct StateCoordinator<T: Clone + Send + Sync + 'static> {
    current_state: RwLock<Option<T>>,
    subscribers: Vec<Box<dyn StateChangedSubscriber<T> + Send + Sync>>,
    // strategy: ConcurrencyStrategy,
}

impl<T: Clone + Send + Sync> StateCoordinator<T> {
    pub fn new() -> Self {
        Self {
            current_state: RwLock::new(None),
            subscribers: Vec::new(),
        }
    }

    async fn run_migration<S>(
        subscriber: &S,
        current_state: Option<&T>,
        new_state: &T,
    ) -> Result<(), StateChangedError>
    where
        S: StateChangedSubscriber<T> + Send + Sync + ?Sized,
    {
        if let Err(e) = subscriber
            .migrate(current_state.cloned(), new_state.clone())
            .await
        {
            let migrate_error = MigrateError {
                name: subscriber.name().to_string(),
                error: e,
            };
            tracing::error!("migrate error: {migrate_error:#?}");
            if let Err(e) = subscriber
                .rollback(current_state.cloned(), new_state.clone())
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
            Self::run_migration(subscriber.as_ref(), current_state.as_ref(), &new_state).await?;
        }

        *current_state = Some(new_state);
        Ok(())
    }
}
