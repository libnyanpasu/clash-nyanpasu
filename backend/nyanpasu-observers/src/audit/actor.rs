use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use nyanpasu_tsdb::{ErrorKind, Retention};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use super::{
    model::{
        AUDIT_BATCH_ROWS, AuditCursor, AuditDropped, AuditFilter, AuditPage, AuditRow,
        MAX_PAGE_ROWS,
    },
    ports::{Appended, AuditRetention, AuditStore, Error},
};

const AUDIT_TICK: Duration = Duration::from_secs(10);

pub(super) enum Message {
    Tick,
    Query(
        AuditFilter,
        Option<AuditCursor>,
        usize,
        RpcReplyPort<Result<AuditPage, Error>>,
    ),
    #[cfg(any(test, feature = "test-support"))]
    TickNow(RpcReplyPort<Result<(), Error>>),
}

/// Explicit inputs for the non-generic audit owner. Channel construction and
/// store opening belong to the composition root.
pub struct AuditArgs {
    /// Opened storage, uniquely owned by the actor after startup.
    pub store: Box<dyn AuditStore>,
    /// The sink's only bounded diagnostic buffer.
    pub rows: mpsc::Receiver<AuditRow>,
    /// Shared only with the sink so both boundaries count losses.
    pub dropped: AuditDropped,
    /// Current settings, sampled at every tick.
    pub retention: Arc<dyn AuditRetention>,
}

pub(super) struct AuditActor;

pub(super) struct Args {
    pub audit: AuditArgs,
    pub shutdown: CancellationToken,
}

pub(super) struct State {
    store: Option<Box<dyn AuditStore>>,
    rows: mpsc::Receiver<AuditRow>,
    dropped: AuditDropped,
    retention: Arc<dyn AuditRetention>,
    shutdown: CancellationToken,
    timer: JoinHandle<()>,
}

impl State {
    async fn blocking<T: Send + 'static>(
        &mut self,
        operation: impl FnOnce(&mut dyn AuditStore) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let Some(mut store) = self.store.take() else {
            return Err(Error::AccessStore);
        };
        let (store, result) = nyanpasu_owner_support::blocking::join(
            tokio::task::spawn_blocking(move || {
                let result = operation(store.as_mut());
                // Drop a failed backend on this blocking thread as well. No
                // uncertain write is retried, even if it may have committed.
                let store = if result
                    .as_ref()
                    .err()
                    .is_some_and(|e| e.kind() == ErrorKind::BackendFailed)
                {
                    None
                } else {
                    Some(store)
                };
                (store, result)
            })
            .await,
        );
        self.store = store;
        if self.store.is_none() {
            self.rows.close();
        }
        result
    }

    async fn write(&mut self, rows: Vec<AuditRow>) -> Result<(), Error> {
        let now = SystemTime::now();
        let (max_age, max_bytes) = self.retention.retention();
        let retention = Retention {
            max_age: Some(max_age),
            max_bytes: Some(max_bytes),
        };
        let dropped = self.dropped.clone();
        self.blocking(move |store| {
            let appended = if rows.is_empty() {
                Ok(Appended::default())
            } else {
                store.append(&rows)
            };
            match &appended {
                Ok(result) => dropped.add(result.late),
                Err(_) => dropped.add(rows.len() as u64),
            }
            if appended
                .as_ref()
                .err()
                .is_some_and(|e| e.kind() == ErrorKind::BackendFailed)
            {
                return appended.map(|_| ());
            }
            // Even an ordinary refused batch must not postpone retention of
            // existing records; a fatal maintenance error takes precedence.
            store.maintain(now, retention)?;
            appended.map(|_| ())
        })
        .await
    }

    async fn tick(&mut self) -> Result<(), Error> {
        if self.store.is_none() {
            return Err(Error::AccessStore);
        }
        let mut rows = Vec::new();
        while rows.len() < AUDIT_BATCH_ROWS {
            let Ok(row) = self.rows.try_recv() else { break };
            rows.push(row);
        }
        self.write(rows).await
    }
}

impl Actor for AuditActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;

    async fn pre_start(
        &self,
        myself: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        Ok(State {
            store: Some(args.audit.store),
            rows: args.audit.rows,
            dropped: args.audit.dropped,
            retention: args.audit.retention,
            shutdown: args.shutdown,
            timer: myself.send_interval(AUDIT_TICK, || Message::Tick),
        })
    }

    async fn handle(
        &self,
        _: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        if state.shutdown.is_cancelled() {
            return Ok(());
        }
        match message {
            Message::Tick => {
                if state.store.is_some()
                    && let Err(error) = state.tick().await
                {
                    tracing::warn!(%error, "Audit tick failed");
                }
            }
            Message::Query(filter, before, limit, reply) => {
                let result = state
                    .blocking(move |store| {
                        store.query(filter, before, limit.clamp(1, MAX_PAGE_ROWS))
                    })
                    .await
                    .map(|mut page| {
                        page.dropped = state.dropped.count();
                        page
                    });
                let _ = reply.send(result);
            }
            #[cfg(any(test, feature = "test-support"))]
            Message::TickNow(reply) => {
                let _ = reply.send(state.tick().await);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        state.timer.abort();
        state.rows.close();
        if state.store.is_some() {
            let mut rows = Vec::new();
            while let Ok(row) = state.rows.try_recv() {
                rows.push(row);
            }
            if let Err(error) = state.write(rows).await {
                tracing::warn!(%error, "Audit shutdown flush failed");
            }
        }
        let store = state.store.take();
        nyanpasu_owner_support::blocking::join(
            tokio::task::spawn_blocking(move || drop(store)).await,
        );
        Ok(())
    }
}
