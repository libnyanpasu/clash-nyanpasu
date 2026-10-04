use std::{sync::Arc, time::Duration};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{model::*, ports::CoreLogStore};
use nyanpasu_config::application::CoreLogSettings;

const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

enum Message {
    Append(CoreLogRecord, RpcReplyPort<CoreLogResult<()>>),
    Discard(String, RpcReplyPort<()>),
    Flush,
    Query(CoreLogQuery, RpcReplyPort<CoreLogResult<CoreLogPage>>),
    Detail(CoreLogCursor, RpcReplyPort<CoreLogResult<CoreLogRecord>>),
    Clear(RpcReplyPort<CoreLogResult<()>>),
    SetInstance(Option<String>, RpcReplyPort<CoreLogResult<()>>),
    Status(RpcReplyPort<CoreLogStatus>),
    #[cfg(test)]
    FlushNow(RpcReplyPort<CoreLogResult<()>>),
}
struct Args {
    settings: CoreLogSettings,
    store: Box<dyn CoreLogStore>,
    shutdown: CancellationToken,
    changed: watch::Sender<CoreLogStatus>,
}
struct State {
    store: Option<Box<dyn CoreLogStore>>,
    shutdown: CancellationToken,
    changed: watch::Sender<CoreLogStatus>,
    status: CoreLogStatus,
    instance: Option<String>,
    failed: bool,
    pending: Vec<PreparedCoreLog>,
    pending_bytes: usize,
    timer: Option<JoinHandle<()>>,
}

async fn joined<T>(task: JoinHandle<T>) -> T {
    match task.await {
        Ok(value) => value,
        Err(error) => match error.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(error) => panic!("Core log owner task cancelled: {error}"),
        },
    }
}

impl State {
    async fn blocking<T: Send + 'static>(
        &mut self,
        operation: impl FnOnce(&mut dyn CoreLogStore) -> CoreLogResult<T> + Send + 'static,
    ) -> CoreLogResult<T> {
        let mut store = self.store.take().expect("the actor owns its log store");
        let (store, result) = joined(tokio::task::spawn_blocking(move || {
            let result = operation(store.as_mut());
            (store, result)
        }))
        .await;
        self.store = Some(store);
        result
    }

    fn publish(&mut self) {
        self.status.version = self.status.version.wrapping_add(1);
        self.changed.send_replace(self.status.clone());
    }

    fn schedule(&mut self, actor: &ActorRef<Message>) {
        if self.timer.is_some() || self.shutdown.is_cancelled() {
            return;
        }
        let actor = actor.clone();
        self.timer = Some(tokio::spawn(async move {
            tokio::time::sleep(FLUSH_INTERVAL).await;
            let _ = actor.cast(Message::Flush);
        }));
    }

    fn discard(&mut self, actor: &ActorRef<Message>) {
        self.status.discarded = self.status.discarded.saturating_add(1);
        self.schedule(actor);
    }

    async fn flush(&mut self) -> CoreLogResult<()> {
        if let Some(timer) = self.timer.take() {
            timer.abort();
        }
        if self.pending.is_empty() {
            if *self.changed.borrow() != self.status {
                self.publish();
            }
            return Ok(());
        }
        let records = std::mem::take(&mut self.pending);
        self.pending_bytes = 0;
        let count = records.len() as u64;
        let (result, status) = self
            .blocking(move |store| Ok((store.append(&records), store.status())))
            .await?;
        if let Ok(mut status) = status {
            status.version = self.status.version;
            status.discarded = self.status.discarded;
            self.status = status;
        }
        if let Err(error) = &result {
            // Commit errors can have an uncertain outcome. Never replay this batch.
            self.failed = true;
            self.status.error = Some(error.to_string());
            self.status.discarded = self.status.discarded.saturating_add(count);
        }
        self.publish();
        result
    }

    async fn clear(&mut self) -> CoreLogResult<()> {
        if let Some(timer) = self.timer.take() {
            timer.abort();
        }
        self.pending.clear();
        self.pending_bytes = 0;
        let (result, status) = self
            .blocking(|store| Ok((store.clear(), store.status())))
            .await?;
        match status {
            Ok(mut status) => {
                status.version = self.status.version;
                self.status = status;
            }
            Err(error) => self.status.error = Some(error.to_string()),
        }
        self.failed = result.is_err();
        if let Err(error) = &result {
            self.status.error = Some(error.to_string());
        }
        self.publish();
        result
    }

    async fn append(
        &mut self,
        actor: &ActorRef<Message>,
        record: CoreLogRecord,
    ) -> CoreLogResult<()> {
        if self.instance.as_deref() != Some(&record.source.instance_id) {
            return Err(CoreLogError::Unavailable(
                "Core log source is not the active instance".into(),
            ));
        }
        if self.failed {
            self.discard(actor);
            return Err(CoreLogError::Unavailable(
                self.status
                    .error
                    .clone()
                    .unwrap_or_else(|| "Core log storage unavailable".into()),
            ));
        }
        let record = match PreparedCoreLog::new(&record) {
            Ok(record) => record,
            Err(error) => {
                self.discard(actor);
                return Err(error);
            }
        };
        if !self.pending.is_empty()
            && self.pending_bytes + record.bytes.len() > BATCH_BYTES
            && let Err(error) = self.flush().await
        {
            self.discard(actor);
            return Err(error);
        }
        self.pending_bytes += record.bytes.len();
        self.pending.push(record);
        if self.pending_bytes >= BATCH_BYTES {
            self.flush().await?;
        } else {
            self.schedule(actor);
        }
        Ok(())
    }
}

struct CoreLogsActor;
impl Actor for CoreLogsActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;

    async fn pre_start(
        &self,
        _: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        let mut state = State {
            store: Some(args.store),
            shutdown: args.shutdown,
            changed: args.changed,
            status: CoreLogStatus::default(),
            instance: None,
            failed: false,
            pending: Vec::new(),
            pending_bytes: 0,
            timer: None,
        };
        state.status = state
            .blocking(move |store| {
                store.configure(args.settings)?;
                store.status()
            })
            .await
            .unwrap_or_else(|error| CoreLogStatus {
                error: Some(error.to_string()),
                ..Default::default()
            });
        state.failed = state.status.error.is_some();
        state.changed.send_replace(state.status.clone());
        Ok(state)
    }

    async fn handle(
        &self,
        actor: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        if state.shutdown.is_cancelled() {
            return Ok(());
        }
        match message {
            Message::Append(record, reply) => {
                let _ = reply.send(state.append(&actor, record).await);
            }
            Message::Discard(error, reply) => {
                state.status.error = Some(error);
                state.discard(&actor);
                let _ = reply.send(());
            }
            Message::Flush => {
                state.timer = None;
                let _ = state.flush().await;
            }
            Message::Query(query, reply) => {
                let result =
                    state
                        .blocking(move |store| store.query(query))
                        .await
                        .map(|mut page| {
                            page.status.version = state.status.version;
                            page.status.error = state.status.error.clone();
                            page.status.discarded = state.status.discarded;
                            page
                        });
                let _ = reply.send(result);
            }
            Message::Detail(cursor, reply) => {
                let _ = reply.send(state.blocking(move |store| store.detail(cursor)).await);
            }
            Message::Status(reply) => {
                let _ = reply.send(state.status.clone());
            }
            Message::Clear(reply) => {
                let _ = reply.send(state.clear().await);
            }
            Message::SetInstance(instance, reply) => {
                let result = if state.instance != instance {
                    state.instance = instance;
                    state.clear().await
                } else {
                    Ok(())
                };
                let _ = reply.send(result);
            }
            #[cfg(test)]
            Message::FlushNow(reply) => {
                let _ = reply.send(state.flush().await);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(timer) = state.timer.take() {
            timer.abort();
        }
        if let Err(error) = state.clear().await {
            tracing::warn!(%error, "Core log shutdown cleanup failed");
        }
        let store = state.store.take();
        joined(tokio::task::spawn_blocking(move || drop(store))).await;
        Ok(())
    }
}

#[derive(Clone)]
pub struct CoreLogsClient(Arc<Inner>);
struct Inner {
    actor: ActorRef<Message>,
    changed: watch::Sender<CoreLogStatus>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}
impl CoreLogsClient {
    pub async fn spawn(
        store: Box<dyn CoreLogStore>,
        settings: CoreLogSettings,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        let changed = watch::channel(CoreLogStatus::default()).0;
        let (actor, _) = Actor::spawn(
            None,
            CoreLogsActor,
            Args {
                store,
                settings,
                shutdown: shutdown.clone(),
                changed: changed.clone(),
            },
        )
        .await?;
        crate::client::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self(Arc::new(Inner { actor, changed })))
    }

    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> Message,
    ) -> CoreLogResult<T> {
        match self.0.actor.call(message, None).await {
            Ok(CallResult::Success(value)) => Ok(value),
            _ => Err(CoreLogError::Unavailable(
                "Core log actor unavailable".into(),
            )),
        }
    }
    pub async fn append(&self, record: CoreLogRecord) -> CoreLogResult<()> {
        self.call(|reply| Message::Append(record, reply)).await?
    }
    pub async fn discard(&self, error: String) -> CoreLogResult<()> {
        self.call(|reply| Message::Discard(error, reply)).await
    }
    pub async fn query(&self, query: CoreLogQuery) -> CoreLogResult<CoreLogPage> {
        self.call(|reply| Message::Query(query, reply)).await?
    }
    pub async fn detail(&self, cursor: CoreLogCursor) -> CoreLogResult<CoreLogRecord> {
        self.call(|reply| Message::Detail(cursor, reply)).await?
    }
    pub async fn clear(&self) -> CoreLogResult<()> {
        self.call(Message::Clear).await?
    }
    pub(crate) async fn set_instance(&self, instance: Option<String>) -> CoreLogResult<()> {
        self.call(|reply| Message::SetInstance(instance, reply))
            .await?
    }
    pub async fn status(&self) -> CoreLogResult<CoreLogStatus> {
        self.call(Message::Status).await
    }
    pub fn subscribe(&self) -> watch::Receiver<CoreLogStatus> {
        self.0.changed.subscribe()
    }
    #[cfg(test)]
    pub(super) async fn flush(&self) -> CoreLogResult<()> {
        self.call(Message::FlushNow).await?
    }
}
