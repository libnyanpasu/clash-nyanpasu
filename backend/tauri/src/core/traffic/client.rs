use super::actor::{Message, StartArgs, TrafficActor, TrafficActorArgs};
use nyanpasu_traffic::{model::*, ports::SourceBinding};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::sync::watch;
#[derive(Clone)]
pub struct TrafficClient {
    actor: ActorRef<Message>,
    summary: watch::Receiver<Option<TrafficSummary>>,
    details: watch::Sender<Option<TrafficDetails>>,
    stopped: watch::Receiver<Option<TrafficResult<()>>>,
    status: watch::Receiver<Option<StoreError>>,
    // This lock owns only the actor task handle, never actor domain state.
    join: std::sync::Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}
pub(crate) async fn call<T: Send + 'static>(
    actor: &ActorRef<Message>,
    message: impl FnOnce(RpcReplyPort<TrafficResult<T>>) -> Message,
) -> TrafficResult<T> {
    match actor
        .call(message, None)
        .await
        .map_err(|e| StoreError::Unavailable(e.to_string()))?
    {
        CallResult::Success(result) => result,
        _ => Err(StoreError::Unavailable(
            "traffic actor stopped before replying".into(),
        )),
    }
}
impl TrafficClient {
    pub async fn start(args: TrafficActorArgs) -> TrafficResult<Self> {
        let (summary_tx, summary) = watch::channel(None);
        let (details, details_rx) = watch::channel(None);
        drop(details_rx);
        let (stopped_tx, stopped) = watch::channel(None);
        let (status_tx, status) = watch::channel(None);
        let (actor, join) = Actor::spawn(
            None,
            TrafficActor,
            StartArgs {
                args,
                summary: summary_tx,
                details: details.clone(),
                stopped: stopped_tx,
                status: status_tx,
            },
        )
        .await
        .map_err(|e| StoreError::Unavailable(e.to_string()))?;
        Ok(Self {
            actor,
            summary,
            details,
            stopped,
            status,
            join: std::sync::Arc::new(tokio::sync::Mutex::new(Some(join))),
        })
    }
    pub async fn shutdown(&self) -> TrafficResult<()> {
        // Even an already-stopped cell must join its task before reporting cleanup.
        let _ = self.actor.drain_and_wait(None).await;
        let mut join = self.join.lock().await;
        if let Some(handle) = join.as_mut() {
            if let Err(error) = handle.await {
                if error.is_panic() {
                    std::panic::resume_unwind(error.into_panic())
                }
                return Err(StoreError::Unavailable(error.to_string()));
            }
            *join = None;
        }
        self.stopped.borrow().clone().unwrap_or_else(|| {
            Err(StoreError::Unavailable(
                "traffic actor stopped without completing cleanup".into(),
            ))
        })
    }
    pub fn subscribe_status(&self) -> watch::Receiver<Option<StoreError>> {
        self.status.clone()
    }
    pub fn notify_current_instance(&self, id: Option<String>) -> TrafficResult<()> {
        self.actor
            .cast(Message::Select(id, None))
            .map_err(|e| StoreError::Unavailable(e.to_string()))
    }
    pub async fn set_current_instance(&self, id: Option<String>) -> TrafficResult<()> {
        call(&self.actor, |p| Message::Select(id, Some(p))).await
    }
    pub fn notify_instance_started(
        &self,
        new: NewSession,
        binding: Option<SourceBinding>,
    ) -> TrafficResult<()> {
        self.actor
            .cast(Message::Start(new, binding, None))
            .map_err(|e| StoreError::Unavailable(e.to_string()))
    }
    pub fn notify_controller_bound(&self, binding: SourceBinding) -> TrafficResult<()> {
        self.actor
            .cast(Message::Bind(binding, None))
            .map_err(|e| StoreError::Unavailable(e.to_string()))
    }
    /// Stop collecting without asserting that the observed process exited.
    pub fn notify_detached(&self, id: String) -> TrafficResult<()> {
        self.actor
            .cast(Message::Detach(id, None))
            .map_err(|e| StoreError::Unavailable(e.to_string()))
    }
    pub async fn detach(&self, id: String) -> TrafficResult<()> {
        call(&self.actor, |p| Message::Detach(id, Some(p))).await
    }
    pub fn notify_instance_exited(&self, id: String, time: i64) -> TrafficResult<()> {
        let time = UInt(
            time.try_into()
                .map_err(|_| StoreError::InvalidData("negative exit time".into()))?,
        );
        self.actor
            .cast(Message::Exit(id, time, None))
            .map_err(|e| StoreError::Unavailable(e.to_string()))
    }
    pub async fn instance_started(&self, new: NewSession) -> TrafficResult<SessionRecord> {
        call(&self.actor, |p| Message::Start(new, None, Some(p))).await
    }
    pub async fn controller_bound(&self, binding: SourceBinding) -> TrafficResult<()> {
        call(&self.actor, |p| Message::Bind(binding, Some(p))).await
    }
    pub async fn instance_exited(&self, end: SessionEnd) -> TrafficResult<CommitReceipt> {
        let session = self.session(end.session_id).await?;
        call(&self.actor, |p| {
            Message::Exit(session.instance_id, end.detected_at, Some(p))
        })
        .await
    }
    pub async fn update_context(
        &self,
        id: String,
        context: Option<ConfigContext>,
    ) -> TrafficResult<()> {
        call(&self.actor, |p| Message::Context(id, context, p)).await
    }
    pub async fn observe(&self, observation: Observation) -> TrafficResult<CommitReceipt> {
        call(&self.actor, |p| Message::Observe(observation, p)).await
    }
    pub async fn source_disconnected(&self, id: String, generation: UInt) -> TrafficResult<()> {
        call(&self.actor, |p| {
            Message::Disconnected(id, generation, None, p)
        })
        .await
    }
    pub async fn current_session(&self) -> TrafficResult<Option<SessionRecord>> {
        call(&self.actor, Message::CurrentSession).await
    }
    pub async fn session(&self, id: SessionId) -> TrafficResult<SessionRecord> {
        call(&self.actor, |p| Message::Session(id, p)).await
    }
    pub async fn query_connections(&self, q: ConnectionsQuery) -> TrafficResult<ConnectionPage> {
        call(&self.actor, |p| Message::Connections(q, p)).await
    }
    pub async fn query_usage(&self, q: UsageQuery) -> TrafficResult<UsageResult> {
        call(&self.actor, |p| Message::Usage(q, p)).await
    }
    pub async fn query_topology(&self, q: TopologyQuery) -> TrafficResult<TopologyResult> {
        call(&self.actor, |p| Message::Topology(q, p)).await
    }
    pub fn subscribe_summary(&self) -> watch::Receiver<Option<TrafficSummary>> {
        self.summary.clone()
    }
    pub fn subscribe_details(&self) -> watch::Receiver<Option<TrafficDetails>> {
        self.details.subscribe()
    }
}
