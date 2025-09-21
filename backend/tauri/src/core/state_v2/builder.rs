pub trait StateSyncBuilder: Default + Clone {
    type State: Clone + Send + Sync + 'static;

    fn build(&self) -> anyhow::Result<Self::State>;
}

pub trait StateAsyncBuilder: Default + Clone {
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
