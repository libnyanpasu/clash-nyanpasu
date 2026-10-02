//! Owns the traffic session: consumes the raw connections feed and persists it
//! in batches. A batch the store refuses goes back to the session, which ignores
//! frames until the next flush retries it.
use std::{sync::Arc, time::Duration};

use nyanpasu_geodata::IpIndex;
use nyanpasu_traffic::{
    ClosedCursor, ClosedPage, Dimension, Frame, Metric, Prune, ReportRequest, Row, Session, Tier,
    TrafficError, TrafficQuery, TrafficReport, TrafficResult, TrafficScope, TrafficStore,
    TrafficSummary, UsageCursor, UsageGroup, UsagePage, filter_rows, merge_closed_page, report,
    usage_by_keys, usage_page,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::{sync::watch, task::JoinHandle, time::MissedTickBehavior};

use super::{
    geo::CountryLookup,
    ports::{Clock, ProfileSelection, RetentionPolicy},
    source::frame_from_snapshot,
};
use crate::core::clash::ws::ClashConnectionsFrame;

const FLUSH_INTERVAL: Duration = Duration::from_secs(30);
/// Dimension combinations that no usage row refers to are collected at most this often.
const COLLECT_INTERVAL_MS: i64 = 60 * 60 * 1000;

pub(super) enum Message {
    Observe(Frame, RpcReplyPort<()>),
    Disconnected(RpcReplyPort<()>),
    Flush(RpcReplyPort<()>),
    Summary(RpcReplyPort<TrafficResult<TrafficSummary>>),
    Report(ReportRequest, RpcReplyPort<TrafficResult<TrafficReport>>),
    Usage(
        TrafficQuery,
        Dimension,
        Metric,
        Option<UsageCursor>,
        usize,
        RpcReplyPort<TrafficResult<UsagePage>>,
    ),
    UsageByKeys(
        TrafficQuery,
        Dimension,
        Vec<String>,
        RpcReplyPort<TrafficResult<Vec<UsageGroup>>>,
    ),
    ClosedConnections(
        Option<ClosedCursor>,
        usize,
        RpcReplyPort<TrafficResult<ClosedPage>>,
    ),
}

pub struct TrafficArgs {
    pub store: Arc<dyn TrafficStore>,
    pub profiles: Arc<dyn ProfileSelection>,
    pub retention: Arc<dyn RetentionPolicy>,
    pub clock: Arc<dyn Clock>,
    pub frames: watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
    /// The country index regions are looked up in; `None` until the core's database loads.
    pub geo: watch::Receiver<Option<Arc<IpIndex>>>,
}

pub(super) struct TrafficActor;

pub(super) struct State {
    store: Arc<dyn TrafficStore>,
    profiles: Arc<dyn ProfileSelection>,
    retention: Arc<dyn RetentionPolicy>,
    clock: Arc<dyn Clock>,
    session: Session,
    /// When the unreferenced dimension combinations were last collected.
    collected_at: Option<i64>,
    /// Hour rows expired since the last successful collection. Starts due: the previous run may
    /// have ended before a collection that was still waiting out its cooldown.
    collection_due: bool,
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
            retention,
            clock,
            frames,
            geo,
        } = args;
        // An unreadable store must not start an empty session: the stored live connections would
        // be recorded again and the store diverge from what the session knows.
        let session = match blocking(&store, |store| store.load()).await? {
            Some((meta, active)) => Session::restore(meta, active),
            None => Session::new(),
        };
        Ok(State {
            store,
            profiles,
            retention,
            session,
            collected_at: None,
            collection_due: true,
            pump: tokio::spawn(pump(myself, frames, geo, clock.clone())),
            clock,
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
                state
                    .session
                    .observe(&frame, state.profiles.current().as_deref());
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
            Message::Report(request, reply) => {
                let _ = reply.send(state.report(request).await);
            }
            Message::Usage(query, dimension, metric, after, limit, reply) => {
                let _ = reply.send(state.usage(query, dimension, metric, after, limit).await);
            }
            Message::UsageByKeys(query, dimension, keys, reply) => {
                let _ = reply.send(state.usage_by_keys(query, dimension, keys).await);
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
    /// Writes what is pending and deletes what the retention no longer covers. A batch the store
    /// refuses goes back to the session, so the next flush carries it again.
    async fn flush(&mut self) {
        let now = self.clock.now_ms();
        let batch = self
            .session
            .take_batch(now, Prune::new(now, self.retention.retention()));
        let flushed = blocking(&self.store, move |store| Ok((store.flush(&batch), batch))).await;
        let hours_pruned = match flushed {
            Ok((Ok(flushed), _)) => flushed.hours_pruned,
            Ok((Err(error), batch)) => {
                tracing::warn!("failed to flush traffic statistics, retrying next time: {error}");
                self.session.requeue(batch);
                return;
            }
            Err(error) => {
                tracing::warn!("failed to flush traffic statistics: {error}");
                return;
            }
        };
        // Only expired hour rows can leave dimension combinations unreferenced. The collection
        // stays due until one succeeds, however long the cooldown or a failure delays it.
        self.collection_due |= hours_pruned;
        if self.collection_due
            && self
                .collected_at
                .is_none_or(|at| now.saturating_sub(at) >= COLLECT_INTERVAL_MS)
        {
            let keep = self.session.referenced_dimensions();
            match blocking(&self.store, move |store| store.collect_tuples(&keep)).await {
                Ok(_) => {
                    self.collected_at = Some(now);
                    self.collection_due = false;
                }
                Err(error) => tracing::warn!("failed to collect traffic dimensions: {error}"),
            }
        }
    }

    async fn summary(&self) -> TrafficResult<TrafficSummary> {
        let stored_closed = blocking(&self.store, |store| store.closed_count()).await?;
        Ok(self.session.summary(stored_closed))
    }

    async fn report(&self, request: ReportRequest) -> TrafficResult<TrafficReport> {
        let request = request.checked()?;
        self.with_rows(&request.query, |rows| report(rows, &request))
            .await
    }

    async fn usage(
        &self,
        query: TrafficQuery,
        dimension: Dimension,
        metric: Metric,
        after: Option<UsageCursor>,
        limit: usize,
    ) -> TrafficResult<UsagePage> {
        query.check()?;
        self.with_rows(&query, |rows| {
            usage_page(
                filter_rows(rows, &query.filters),
                dimension,
                metric,
                after.as_ref(),
                limit,
            )
        })
        .await
    }

    /// In request order; keys without traffic are left out.
    async fn usage_by_keys(
        &self,
        query: TrafficQuery,
        dimension: Dimension,
        keys: Vec<String>,
    ) -> TrafficResult<Vec<UsageGroup>> {
        query.check()?;
        self.with_rows(&query, |rows| {
            usage_by_keys(filter_rows(rows, &query.filters), dimension, &keys)
        })
        .await
    }

    async fn closed_connections(
        &self,
        cursor: Option<ClosedCursor>,
        limit: usize,
    ) -> TrafficResult<ClosedPage> {
        let before = cursor.clone();
        let stored = blocking(&self.store, move |store| {
            store.closed_connections(before.as_ref(), limit)
        })
        .await?;
        Ok(merge_closed_page(
            stored,
            self.session.pending_closed(),
            cursor.as_ref(),
            limit,
        ))
    }

    /// Hands `f` the usage rows `query` selects by range and scope. Closed connections are what
    /// the store and the pending buffer hold, live ones what the session holds; the two never
    /// overlap, so `All` is their union.
    async fn with_rows<T>(
        &self,
        query: &TrafficQuery,
        f: impl FnOnce(Vec<Row<'_>>) -> T,
    ) -> TrafficResult<T> {
        let tier: Tier = query.range.tier();
        let from = query.range.start(self.clock.now_ms());
        let stored = match query.scope {
            TrafficScope::Active => Vec::new(),
            TrafficScope::All | TrafficScope::Closed => {
                blocking(&self.store, move |store| store.usage(tier, from)).await?
            }
        };
        let mut rows: Vec<Row<'_>> = Vec::new();
        if query.scope != TrafficScope::Active {
            rows.extend(stored.iter().map(|(d, usage)| (&**d, *usage, None)));
            rows.extend(self.session.pending_rows(tier, from));
        }
        if query.scope != TrafficScope::Closed {
            rows.extend(self.session.active_rows(tier, from));
        }
        Ok(f(rows))
    }
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

/// Feeds the actor from the frames watch and drives the periodic flush. It
/// awaits every call, so at most one frame is in flight and the watch coalesces
/// the rest. Ends when the feed closes or the actor is gone.
async fn pump(
    actor: ActorRef<Message>,
    mut frames: watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
    geo: watch::Receiver<Option<Arc<IpIndex>>>,
    clock: Arc<dyn Clock>,
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
                        // The index as published when the frame converts.
                        let index = geo.borrow().clone();
                        let frame = frame_from_snapshot(
                            &raw,
                            clock.now_ms(),
                            origin.elapsed(),
                            index.as_deref().map(|index| index as &dyn CountryLookup),
                        );
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
