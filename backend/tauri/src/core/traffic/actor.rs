//! Owns the traffic session: consumes the raw connections feed and persists it
//! in batches. A failed flush is logged and its batch dropped; this is
//! statistics, not a ledger.
use std::{
    collections::HashMap,
    hash::Hash,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use nyanpasu_traffic::{
    Bytes, ClosedCursor, ClosedPage, FlushBatch, Frame, GroupBy, Rate, Session, Topology,
    TopologyKey, TopologyPath, TrafficError, TrafficResult, TrafficStore, TrafficSummary, Usage,
    UsageGroup, merge_closed_page, topology,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::{sync::watch, task::JoinHandle, time::MissedTickBehavior};

use super::{ports::ProfileSelection, source::frame_from_snapshot};
use crate::core::clash::ws::ClashConnectionsFrame;

const FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const MAX_LIMIT: usize = 200;

pub(super) enum Message {
    Observe(Frame, RpcReplyPort<()>),
    Disconnected(RpcReplyPort<()>),
    Flush(RpcReplyPort<()>),
    Summary(RpcReplyPort<TrafficResult<TrafficSummary>>),
    Usage(GroupBy, usize, RpcReplyPort<TrafficResult<Usage>>),
    Topology(usize, RpcReplyPort<TrafficResult<Topology>>),
    ClosedConnections(
        Option<ClosedCursor>,
        usize,
        RpcReplyPort<TrafficResult<ClosedPage>>,
    ),
}

pub struct TrafficArgs {
    pub store: Arc<dyn TrafficStore>,
    pub profiles: Arc<dyn ProfileSelection>,
    pub frames: watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
}

pub(super) struct TrafficActor;

pub(super) struct State {
    store: Arc<dyn TrafficStore>,
    profiles: Arc<dyn ProfileSelection>,
    session: Session,
    pump: JoinHandle<()>,
}

impl Actor for TrafficActor {
    type Msg = Message;
    type State = State;
    type Arguments = TrafficArgs;

    async fn pre_start(
        &self,
        myself: ActorRef<Message>,
        args: TrafficArgs,
    ) -> Result<State, ActorProcessingErr> {
        let TrafficArgs {
            store,
            profiles,
            frames,
        } = args;
        let current = profiles.current();
        let stored = blocking(&store, |store| store.load())
            .await
            .unwrap_or_else(|error| {
                tracing::warn!("failed to load the traffic session: {error}");
                None
            });
        let session = match stored {
            Some((meta, active)) if meta.profile == current => Session::restore(meta, active),
            // No data, another profile, or an unreadable store: the bootstrap wipe.
            _ => {
                let mut session = Session::new(current, wall_clock_ms());
                flush_session(&store, &mut session).await;
                session
            }
        };
        Ok(State {
            store,
            profiles,
            session,
            pump: tokio::spawn(pump(myself, frames)),
        })
    }

    async fn handle(
        &self,
        _: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Observe(frame, reply) => {
                state.observe(&frame).await;
                let _ = reply.send(());
            }
            Message::Disconnected(reply) => {
                state.session.disconnect();
                let _ = reply.send(());
            }
            Message::Flush(reply) => {
                state.flush().await;
                let _ = reply.send(());
            }
            Message::Summary(reply) => {
                let _ = reply.send(state.summary().await);
            }
            Message::Usage(group, limit, reply) => {
                let _ = reply.send(state.usage(group, limit).await);
            }
            Message::Topology(limit, reply) => {
                let _ = reply.send(state.topology(limit).await);
            }
            Message::ClosedConnections(cursor, limit, reply) => {
                let _ = reply.send(state.closed_connections(cursor, limit).await);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        state.pump.abort();
        if let Err(error) = (&mut state.pump).await
            && let Ok(panic) = error.try_into_panic()
        {
            std::panic::resume_unwind(panic);
        }
        state.flush().await;
        Ok(())
    }
}

impl State {
    async fn observe(&mut self, frame: &Frame) {
        let current = self.profiles.current();
        if current != self.session.meta().profile {
            self.session.switch_profile(current, frame.wall_ms);
            // The wipe and the surviving baselines go out together, before the next frame moves
            // them.
            self.flush().await;
        }
        self.session.observe(frame);
    }

    async fn flush(&mut self) {
        flush_session(&self.store, &mut self.session).await;
    }

    // While the wipe is pending the store still holds the previous session, so the queries
    // below answer from memory alone.
    async fn summary(&self) -> TrafficResult<TrafficSummary> {
        let stored_closed = if self.session.reset_pending() {
            0
        } else {
            blocking(&self.store, |store| store.closed_count()).await?
        };
        Ok(self.session.summary(stored_closed))
    }

    async fn usage(&self, group: GroupBy, limit: usize) -> TrafficResult<Usage> {
        let stored = if self.session.reset_pending() {
            Vec::new()
        } else {
            blocking(&self.store, move |store| store.totals(group)).await?
        };
        let pending = self
            .session
            .pending_totals(group)
            .map(|(key, bytes)| (key.to_owned(), bytes));
        Ok(rank_usage(
            merge(stored, pending),
            self.session.current_rate_by(group),
            limit,
        ))
    }

    async fn topology(&self, limit: usize) -> TrafficResult<Topology> {
        let stored = if self.session.reset_pending() {
            Vec::new()
        } else {
            blocking(&self.store, |store| store.topology()).await?
        };
        let pending = self
            .session
            .pending_topology()
            .map(|(key, bytes)| (key.clone(), bytes));
        Ok(rank_topology(merge(stored, pending), limit))
    }

    async fn closed_connections(
        &self,
        cursor: Option<ClosedCursor>,
        limit: usize,
    ) -> TrafficResult<ClosedPage> {
        let stored = if self.session.reset_pending() {
            ClosedPage {
                connections: Vec::new(),
                next: None,
            }
        } else {
            let before = cursor.clone();
            blocking(&self.store, move |store| {
                store.closed_connections(before.as_ref(), limit)
            })
            .await?
        };
        Ok(merge_closed_page(
            stored,
            self.session.pending_closed(),
            cursor.as_ref(),
            limit,
        ))
    }
}

fn merge<K: Eq + Hash>(
    stored: Vec<(K, Bytes)>,
    pending: impl IntoIterator<Item = (K, Bytes)>,
) -> HashMap<K, Bytes> {
    let mut merged: HashMap<K, Bytes> = HashMap::new();
    for (key, bytes) in stored.into_iter().chain(pending) {
        let slot = merged.entry(key).or_default();
        *slot = slot.saturating_add(bytes);
    }
    merged
}

fn sum<'a>(bytes: impl IntoIterator<Item = &'a Bytes>) -> Bytes {
    bytes
        .into_iter()
        .fold(Bytes::default(), |sum, bytes| sum.saturating_add(*bytes))
}

/// Top `limit` groups by traffic; the rest is folded into `other`.
fn rank_usage(
    totals: HashMap<String, Bytes>,
    mut rates: HashMap<String, Rate>,
    limit: usize,
) -> Usage {
    let total = sum(totals.values());
    let mut ranked: Vec<(String, Bytes)> = totals.into_iter().collect();
    ranked
        .sort_by(|(a_key, a), (b_key, b)| b.total().cmp(&a.total()).then_with(|| a_key.cmp(b_key)));
    let rest = ranked.split_off(limit.clamp(1, MAX_LIMIT).min(ranked.len()));
    Usage {
        total,
        groups: ranked
            .into_iter()
            .map(|(key, bytes)| UsageGroup {
                current_rate: rates.remove(&key),
                key,
                bytes,
            })
            .collect(),
        other: sum(rest.iter().map(|(_, bytes)| bytes)),
    }
}

/// Top `limit` paths by traffic; the rest is folded into `other`.
fn rank_topology(paths: HashMap<TopologyKey, Bytes>, limit: usize) -> Topology {
    let mut ranked: Vec<TopologyPath> = paths
        .into_iter()
        .map(|(key, bytes)| TopologyPath { key, bytes })
        .collect();
    let identity = |key: &TopologyKey| {
        (
            key.source.clone(),
            key.rule.kind.clone(),
            key.rule.payload.clone(),
            key.groups.clone(),
            key.exit.clone(),
        )
    };
    ranked.sort_by(|a, b| {
        b.bytes
            .total()
            .cmp(&a.bytes.total())
            .then_with(|| identity(&a.key).cmp(&identity(&b.key)))
    });
    let rest = ranked.split_off(limit.clamp(1, MAX_LIMIT).min(ranked.len()));
    let (nodes, edges) = topology::project(&ranked);
    Topology {
        paths: ranked,
        nodes,
        edges,
        other: sum(rest.iter().map(|path| &path.bytes)),
    }
}

/// A failed batch is dropped, except that a lost wipe is asked for again by the next one.
async fn flush_session(store: &Arc<dyn TrafficStore>, session: &mut Session) {
    let batch = session.take_batch();
    let reset = batch.reset;
    if let Err(error) = flush_store(store, batch).await {
        tracing::warn!("failed to flush traffic statistics, dropping the batch: {error}");
        if reset {
            session.require_reset();
        }
    }
}

async fn flush_store(store: &Arc<dyn TrafficStore>, batch: FlushBatch) -> TrafficResult<()> {
    blocking(store, move |store| store.flush(&batch)).await
}

/// Runs a synchronous store call off the runtime. A panic inside it is a bug
/// and keeps unwinding; a cancelled task is reported as a storage error.
async fn blocking<T: Send + 'static>(
    store: &Arc<dyn TrafficStore>,
    call: impl FnOnce(&dyn TrafficStore) -> TrafficResult<T> + Send + 'static,
) -> TrafficResult<T> {
    let store = store.clone();
    match tokio::task::spawn_blocking(move || call(store.as_ref())).await {
        Ok(result) => result,
        Err(error) => match error.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(error) => Err(TrafficError::Storage(format!(
                "storage task did not complete: {error}"
            ))),
        },
    }
}

fn wall_clock_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

/// Feeds the actor from the frames watch and drives the periodic flush. It
/// awaits every call, so at most one frame is in flight and the watch coalesces
/// the rest. Ends when the feed closes or the actor is gone.
async fn pump(
    actor: ActorRef<Message>,
    mut frames: watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
) {
    let origin = tokio::time::Instant::now();
    let mut flush = tokio::time::interval_at(origin + FLUSH_INTERVAL, FLUSH_INTERVAL);
    flush.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        let delivered = tokio::select! {
            changed = frames.changed() => {
                if changed.is_err() {
                    return;
                }
                let latest = frames.borrow_and_update().clone();
                match latest {
                    Some(raw) => {
                        let frame = frame_from_snapshot(&raw, wall_clock_ms(), origin.elapsed());
                        call(&actor, |reply| Message::Observe(frame, reply)).await
                    }
                    None => call(&actor, Message::Disconnected).await,
                }
            }
            _ = flush.tick() => call(&actor, Message::Flush).await,
        };
        if !delivered {
            return;
        }
    }
}

async fn call(
    actor: &ActorRef<Message>,
    message: impl FnOnce(RpcReplyPort<()>) -> Message,
) -> bool {
    matches!(actor.call(message, None).await, Ok(CallResult::Success(())))
}
