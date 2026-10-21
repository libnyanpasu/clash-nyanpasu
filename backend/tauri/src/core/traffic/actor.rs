//! Owns the traffic session: consumes the raw connections feed and persists it
//! in batches. A batch the store refuses goes back to the session, which ignores
//! frames until the next flush retries it.
use std::{sync::Arc, time::Duration};

use nyanpasu_geodata::IpIndex;
use nyanpasu_traffic::{
    ClosedCursor, ClosedPage, ClosedSelection, Dimension, Frame, Metric, ReportRequest, Session,
    TrafficDb, TrafficError, TrafficFilter, TrafficQuery, TrafficRange, TrafficReport,
    TrafficResult, TrafficSummary, UsageCursor, UsageGroup, UsagePage, check_filters,
    store::{FlushError, Recovery},
};
use nyanpasu_tsdb::{Profile, storage::Storage};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use std::marker::PhantomData;
use tokio::{sync::watch, task::JoinHandle, time::MissedTickBehavior};

use super::{
    geo::CountryLookup,
    ports::{Clock, LocalSourceLocation, ProfileSelection, RetentionPolicy, TrafficStorage},
    source::frame_from_snapshot,
};
use crate::core::clash::ws::ClashConnectionsFrame;

const FLUSH_INTERVAL: Duration = Duration::from_secs(30);
pub(super) enum Message {
    #[cfg(test)]
    StoredReport(ReportRequest, RpcReplyPort<TrafficResult<TrafficReport>>),
    #[cfg(test)]
    StoredClosed(RpcReplyPort<TrafficResult<ClosedPage>>),
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
        TrafficRange,
        Vec<TrafficFilter>,
        Option<ClosedCursor>,
        usize,
        RpcReplyPort<TrafficResult<ClosedPage>>,
    ),
    ActiveIds(Vec<TrafficFilter>, RpcReplyPort<TrafficResult<Vec<String>>>),
}

pub struct TrafficArgs<S: Storage> {
    pub storage: Arc<dyn TrafficStorage<S>>,
    pub profile: Profile,
    pub profiles: Arc<dyn ProfileSelection>,
    pub retention: Arc<dyn RetentionPolicy>,
    pub clock: Arc<dyn Clock>,
    pub frames: watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
    /// The country index regions are looked up in; `None` until the core's database loads.
    pub geo: watch::Receiver<Option<Arc<IpIndex>>>,
    pub local_source: Arc<dyn LocalSourceLocation>,
}

pub(super) struct TrafficActor<S: Storage>(pub PhantomData<fn() -> S>);

pub(super) struct State<S: Storage> {
    storage: Arc<dyn TrafficStorage<S>>,
    db: TrafficDb<S>,
    profiles: Arc<dyn ProfileSelection>,
    retention: Arc<dyn RetentionPolicy>,
    clock: Arc<dyn Clock>,
    session: Session,
    pump: JoinHandle<()>,
}

impl<S: Storage> Actor for TrafficActor<S> {
    type Msg = Message;
    type State = State<S>;
    type Arguments = TrafficArgs<S>;

    async fn pre_start(
        &self,
        myself: ActorRef<Message>,
        args: TrafficArgs<S>,
    ) -> Result<State<S>, ActorProcessingErr> {
        let TrafficArgs {
            storage,
            profile,
            profiles,
            retention,
            clock,
            frames,
            geo,
            local_source,
        } = args;
        // An unreadable store must not start an empty session: the stored live connections would
        // be recorded again and the store diverge from what the session knows.
        let opener = storage.clone();
        let (mut db, restored) = crate::utils::blocking::join(
            tokio::task::spawn_blocking(move || opener.initialize(profile)).await,
        )?;
        db.set_retention(retention.retention());
        let session = restored.map_or_else(Session::new, |(meta, active)| {
            Session::restore(meta, active)
        });
        Ok(State {
            storage,
            db,
            profiles,
            retention,
            session,
            pump: tokio::spawn(pump(myself, frames, geo, local_source, clock.clone())),
            clock,
        })
    }

    async fn handle(
        &self,
        _: ActorRef<Message>,
        message: Message,
        state: &mut State<S>,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            #[cfg(test)]
            Message::StoredReport(request, reply) => {
                let _ = reply.send(state.db.report(
                    &Session::new(),
                    &request,
                    state.clock.now_ms(),
                ));
            }
            #[cfg(test)]
            Message::StoredClosed(reply) => {
                let _ = reply.send(state.db.closed_connections(
                    &Session::new(),
                    &ClosedSelection::default(),
                    None,
                    500,
                    state.clock.now_ms(),
                ));
            }
            Message::Observe(frame, reply) => {
                if !state.db.needs_recovery() {
                    state
                        .session
                        .observe(frame, state.profiles.current().as_deref(), &mut || {
                            state.db.next_event_seq()
                        });
                    if state.session.pending_bytes() >= state.db.batch_bytes() / 2 {
                        state.flush();
                    }
                }
                let _ = reply.send(());
            }
            Message::Disconnected(reply) => {
                state.session.disconnect();
                let _ = reply.send(());
            }
            Message::Flush(reply) => {
                state.flush();
                let _ = reply.send(());
            }
            Message::Summary(reply) => {
                let _ = reply.send(state.db.summary(&state.session));
            }
            Message::Report(request, reply) => {
                let _ = reply.send(request.checked().and_then(|request| {
                    state
                        .db
                        .report(&state.session, &request, state.clock.now_ms())
                }));
            }
            Message::Usage(query, dimension, metric, after, limit, reply) => {
                let _ = reply.send(state.db.usage_page(
                    &state.session,
                    &query,
                    dimension,
                    metric,
                    after.as_ref(),
                    limit,
                    state.clock.now_ms(),
                ));
            }
            Message::UsageByKeys(query, dimension, keys, reply) => {
                let _ = reply.send(state.db.usage_by_keys(
                    &state.session,
                    &query,
                    dimension,
                    &keys,
                    state.clock.now_ms(),
                ));
            }
            Message::ClosedConnections(range, filters, cursor, limit, reply) => {
                let _ = reply.send(state.db.closed_connections(
                    &state.session,
                    &ClosedSelection {
                        since_ms: range.start_ms(state.clock.now_ms()),
                        filters,
                    },
                    cursor.as_ref(),
                    limit,
                    state.clock.now_ms(),
                ));
            }
            Message::ActiveIds(filters, reply) => {
                let _ = reply.send(state.active_ids(&filters));
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State<S>,
    ) -> Result<(), ActorProcessingErr> {
        state.pump.abort();
        if let Err(error) = (&mut state.pump).await
            && let Ok(panic) = error.try_into_panic()
        {
            std::panic::resume_unwind(panic);
        }
        state.flush();
        Ok(())
    }
}

impl<S: Storage> State<S> {
    fn recover(&mut self) -> bool {
        match self.db.recover(|| self.storage.open()) {
            Ok(Recovery::Restored(restored)) => {
                self.session = restored.map_or_else(Session::new, |(meta, active)| {
                    Session::restore(meta, active)
                });
                true
            }
            Ok(Recovery::WaitingForReaders) => {
                tracing::warn!("traffic recovery is waiting for readers; retrying next flush");
                false
            }
            Err(error) => {
                tracing::warn!(
                    "failed to recover traffic statistics; retrying next flush: {error}"
                );
                false
            }
        }
    }

    fn flush(&mut self) {
        if self.db.needs_recovery() && !self.recover() {
            return;
        }
        let now = self.clock.now_ms();
        let batch = self.session.take_batch(now);
        match self.db.flush(&batch, now, self.retention.retention()) {
            Ok(()) => {}
            Err(FlushError::NotApplied(error)) => {
                tracing::warn!("traffic batch was refused; retrying next flush: {error}");
                self.session.requeue(batch);
            }
            Err(FlushError::Partial {
                error,
                closed,
                active,
            }) => {
                tracing::warn!(
                    "traffic batch partially committed; retrying only unwritten data: {error}"
                );
                self.session.requeue(batch.uncommitted(closed, active));
            }
            Err(error @ FlushError::Oversized { .. }) => {
                // Only a batch without per-close buckets cannot be divided safely. Drop its
                // uncommitted input, with an error, rather than freeze all future recording.
                // Already committed transactions are never replayed.
                tracing::error!(%error, "discarding unrepresentable pending traffic input; recording continues");
            }
            Err(FlushError::Recovery(error)) => {
                tracing::warn!("traffic storage failed; recovering: {error}");
                // Recovery replaces the session that produced this input; never replay it.
                drop(batch);
                self.recover();
            }
            Err(FlushError::Applied(error)) => {
                tracing::warn!("traffic maintenance failed: {error}")
            }
        }
    }
    fn active_ids(&self, filters: &[TrafficFilter]) -> TrafficResult<Vec<String>> {
        if self.db.needs_recovery() {
            return Err(TrafficError::Storage("recovery is pending".into()));
        }
        check_filters(filters)?;
        Ok(self.session.active_ids(filters))
    }
}

/// Feeds the actor from the frames watch and drives the periodic flush. It
/// awaits every call, so at most one frame is in flight and the watch coalesces
/// the rest. Ends when the feed closes or the actor is gone.
async fn pump(
    actor: ActorRef<Message>,
    mut frames: watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
    geo: watch::Receiver<Option<Arc<IpIndex>>>,
    local_source: Arc<dyn LocalSourceLocation>,
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
                            local_source.addresses(),
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
