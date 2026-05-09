pub trait StateSyncBuilder: Default + Clone {
    type State: Clone + Send + Sync + 'static;

    fn build(&self) -> anyhow::Result<Self::State>;
}

pub trait StateAsyncBuilder: Default + Clone {
    type State: Clone + Send + Sync + 'static;

    fn build(&self) -> impl Future<Output = anyhow::Result<Self::State>> + Send;
}

impl<T, S> StateAsyncBuilder for S
where
    S: StateSyncBuilder<State = T> + Send + Sync,
    T: Clone + Send + Sync + 'static,
{
    type State = T;
    async fn build(&self) -> anyhow::Result<Self::State> {
        StateSyncBuilder::build(self)
    }
}
