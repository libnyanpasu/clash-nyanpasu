//! Supervised local connections. Accepted calls finish independently of transport IO.

use std::{collections::HashMap, sync::Arc};

use nyanpasu_contracts::logs::LogSource;
use nyanpasu_logging::LogSession;
use nyanpasu_owner_support::blocking;
use nyanpasu_rpc_protocol::{
    RpcError, RpcOwner,
    frame::{self, Feature, Frame, Reply, RequestId, SubscriptionErrorCode, SubscriptionTarget},
    session::{RECOVERABLE_EVENTS, Session},
};
use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use tracing::instrument::WithSubscriber as _;

use nyanpasu_single_instance::endpoint::{EndpointError, EndpointListener};

use super::subscriptions::{
    self, Delivery, DeliveryAudit, Output, Subscription, SubscriptionSource,
};
use crate::{
    audit::{AuditInvocation, AuditPhase, DeliveryOutcome, RpcTransport, SubscriptionOutcome},
    registry::{RpcFuture, UnifiedRpc, dispatch_with_audit, unreported},
};

/// Bounds are per connection, so a blocked peer cannot consume another's capacity.
#[derive(Debug, Clone, Copy)]
pub struct ConnectionLimits {
    /// Maximum accepted calls and pending establishments per connection.
    pub in_flight: usize,
    /// Maximum entries in each output handoff; each frame is limited to 4 MiB.
    pub output: usize,
    /// Combined limit for reserved/established resources and retained closed log IDs.
    pub resources: usize,
}

impl Default for ConnectionLimits {
    fn default() -> Self {
        Self {
            in_flight: 16,
            output: 16,
            resources: 32,
        }
    }
}

/// An explicitly composed local transport; construction does not bind an endpoint.
pub struct LocalServer {
    rpc: UnifiedRpc,
    source: Arc<dyn SubscriptionSource>,
    instance: String,
    limits: ConnectionLimits,
}

impl LocalServer {
    /// Uses the facade for snapshots and creates a fresh instance namespace.
    ///
    /// # Panics
    /// Panics if the configured in-flight or output capacity is zero.
    pub fn new(rpc: UnifiedRpc, limits: ConnectionLimits) -> Self {
        assert!(
            limits.in_flight > 0 && limits.output > 0,
            "local IO capacities must be positive: {limits:?}"
        );
        Self {
            source: Arc::new(rpc.dependencies.client.clone()),
            rpc,
            instance: uuid::Uuid::new_v4().to_string(),
            limits,
        }
    }

    /// Accept until shutdown, then drain every connection and its accepted work.
    /// The shell must supervise this future; task panics resume their original payload.
    ///
    /// # Errors
    /// Reports an endpoint accept failure after draining already accepted connections.
    pub async fn serve(
        self,
        listener: EndpointListener,
        shutdown: CancellationToken,
    ) -> Result<(), EndpointError> {
        let mut connections = JoinSet::new();
        let mut number = 0u64;
        let result = loop {
            tokio::select! {
                biased;
                () = shutdown.cancelled() => break Ok(()),
                Some(result) = connections.join_next() => blocking::join(result),
                accepted = listener.accept() => match accepted {
                    Ok(stream) => {
                        number = number.checked_add(1).expect("local connection IDs exhausted");
                        connections.spawn(serve_stream(stream, RpcOwner::local_connection(&self.instance, number), self.rpc.clone(), self.source.clone(), self.limits, shutdown.child_token()).with_current_subscriber());
                    }
                    Err(error) => break Err(error),
                }
            }
        };
        drop(listener);
        shutdown.cancel();
        while let Some(result) = connections.join_next().await {
            blocking::join(result);
        }
        result
    }
}

struct LogResource {
    source: LogSource,
    session: String,
    closed: bool,
}

impl LogResource {
    fn same_resource(&self, other: &Self) -> bool {
        self.session == other.session
            && matches!(
                (self.source, other.source),
                (LogSource::App, LogSource::App) | (LogSource::Service, LogSource::Service)
            )
    }
}

struct SubscriptionTask {
    stop: CancellationToken,
    closed: oneshot::Receiver<()>,
    audit: AuditInvocation,
}

enum Completion {
    Call {
        audit: AuditInvocation,
        id: RequestId,
        result: Result<Value, RpcError>,
        opened: Option<LogResource>,
        closed: Option<LogResource>,
        reserved: bool,
        log_call: bool,
        permit: OwnedSemaphorePermit,
    },
    Subscribe {
        audit: AuditInvocation,
        id: RequestId,
        result: Result<Subscription, SubscriptionErrorCode>,
        permit: OwnedSemaphorePermit,
    },
}

struct Connection {
    rpc: UnifiedRpc,
    source: Arc<dyn SubscriptionSource>,
    owner: RpcOwner,
    session: Session,
    limits: ConnectionLimits,
    pending_resources: usize,
    pending_log_calls: usize,
    logs: Vec<LogResource>,
    subscriptions: HashMap<String, SubscriptionTask>,
    calls: JoinSet<Completion>,
    forwarders: JoinSet<()>,
    outgoing: mpsc::Sender<Delivery>,
    events: mpsc::Sender<Delivery>,
    stop: CancellationToken,
}

impl Connection {
    fn has_capacity(&self) -> bool {
        self.pending_resources + self.logs.len() + self.subscriptions.len() < self.limits.resources
    }

    async fn send_delivery(&mut self, delivery: Delivery) {
        let frame = &delivery.frame;
        if self.session.send(frame).is_err() {
            self.stop.cancel();
            return;
        }
        if frame::encode(frame).is_err() {
            // Subscription payload overflow ends only that stream.
            if let Some(subscription) = subscription_id(frame) {
                let subscription = subscription.to_owned();
                self.remove_subscription(&subscription, SubscriptionOutcome::Failed)
                    .await;
                let failure = Frame::SubscriptionError {
                    target: SubscriptionTarget::Subscription(subscription),
                    code: SubscriptionErrorCode::ResourceLimit,
                };
                if self.session.send(&failure).is_ok() {
                    self.enqueue(failure).await;
                }
            } else {
                self.stop.cancel();
            }
            return;
        }
        self.enqueue_delivery(delivery).await;
    }

    async fn enqueue(&self, frame: Frame) {
        self.enqueue_delivery(Delivery {
            frame,
            written: None,
            audits: Vec::new(),
        })
        .await;
    }

    async fn enqueue_delivery(&self, delivery: Delivery) {
        tokio::select! {
            biased;
            () = self.stop.cancelled() => (),
            result = self.outgoing.send(delivery) => if result.is_err() { self.stop.cancel(); },
        }
    }

    async fn remove_subscription(&mut self, subscription: &str, outcome: SubscriptionOutcome) {
        if let Some(task) = self.subscriptions.remove(subscription) {
            task.stop.cancel();
            // Release the receiver/demand before acknowledging unsubscribe.
            let _ = task.closed.await;
            task.audit.record(AuditPhase::Subscription { outcome });
        }
    }

    async fn receive(&mut self, frame: Frame, permit: OwnedSemaphorePermit) {
        let control = match self.session.receive(&frame) {
            Ok(control) => control,
            Err(error) => {
                self.enqueue(Frame::Reject {
                    code: error.reject_code(),
                })
                .await;
                // The writer closes after the rejection is flushed. Shutdown or
                // peer loss can still interrupt an unwritable connection.
                self.stop.cancelled().await;
                return;
            }
        };
        match frame {
            Frame::Request { id, method, params } => {
                let opening = method == "open_log_session";
                let log_call = opening || method == "close_log_session";
                if log_call {
                    self.pending_log_calls += 1;
                }
                let reserved = opening && self.has_capacity();
                if reserved {
                    self.pending_resources += 1;
                }
                let admission = if opening && !reserved {
                    Err(RpcError::new(
                        "resource_limit",
                        "connection resource limit reached",
                    ))
                } else {
                    Ok(())
                };
                let rpc = self.rpc.clone();
                let owner = self.owner.clone();
                let audit =
                    self.rpc
                        .audit_invocation(RpcTransport::Local, &method, &owner, Some(id));
                self.calls.spawn(
                    async move {
                        let result = rpc
                            .call_with_admission(
                                &audit,
                                RpcTransport::Local,
                                &method,
                                owner,
                                params.clone(),
                                admission,
                            )
                            .await;
                        let opened = if opening {
                            result.as_ref().ok().map(|value| {
                                let session: LogSession = serde_json::from_value(value.clone())
                                    .expect("open_log_session returns LogSession");
                                let source = serde_json::from_value(params["source"].clone())
                                    .expect("successful open parsed its source");
                                LogResource {
                                    source,
                                    session: session.id,
                                    closed: false,
                                }
                            })
                        } else {
                            None
                        };
                        let closed = if method == "close_log_session" && result.is_ok() {
                            Some(LogResource {
                                source: serde_json::from_value(params["source"].clone())
                                    .expect("successful close parsed its source"),
                                session: params["session"]
                                    .as_str()
                                    .expect("successful close parsed its session")
                                    .to_owned(),
                                closed: false,
                            })
                        } else {
                            None
                        };
                        Completion::Call {
                            audit,
                            id,
                            result,
                            opened,
                            closed,
                            reserved,
                            log_call,
                            permit,
                        }
                    }
                    .with_current_subscriber(),
                );
            }
            Frame::Subscribe { id, stream, event } => {
                let audit = self.rpc.dependencies.audit.invocation(
                    RpcTransport::Local,
                    &self.owner,
                    "local_subscribe",
                    Some(id),
                );
                let pending = control.is_none();
                let rejected = control.or_else(|| {
                    (!self.has_capacity()).then_some(Frame::SubscriptionError {
                        target: SubscriptionTarget::Id(id),
                        code: SubscriptionErrorCode::ResourceLimit,
                    })
                });
                if let Some(rejection) = rejected {
                    let code = match rejection {
                        Frame::SubscriptionError { code, .. } => code,
                        _ => unreachable!("subscribe control reply"),
                    };
                    let _ = dispatch_with_audit(&audit, || {
                        Err::<RpcFuture, _>(subscription_error(code))
                    })
                    .await;
                    audit.record(AuditPhase::Subscription {
                        outcome: SubscriptionOutcome::Failed,
                    });
                    drop(permit);
                    // Session rejects before pending insertion; adapter capacity rejects after it.
                    if pending {
                        self.send_delivery(Delivery {
                            frame: rejection,
                            written: None,
                            audits: vec![DeliveryAudit::new(audit)],
                        })
                        .await;
                    } else {
                        self.enqueue_delivery(Delivery {
                            frame: rejection,
                            written: None,
                            audits: vec![DeliveryAudit::new(audit)],
                        })
                        .await;
                    }
                    return;
                }
                self.pending_resources += 1;
                let rpc = self.rpc.clone();
                let source = self.source.clone();
                // Subscribe synchronously, before polling any snapshot future.
                let mut receiver = rpc.dependencies.events.subscribe();
                self.calls.spawn(
                    async move {
                        let mut failure = SubscriptionErrorCode::SnapshotUnavailable;
                        let result = dispatch_with_audit(&audit, || {
                            let event = (stream != "connection-details").then(|| {
                                RECOVERABLE_EVENTS
                                    .into_iter()
                                    .find(|name| Some(name.as_str()) == event.as_deref())
                                    .expect("session admitted a recoverable identity")
                            });
                            if event
                                == Some(nyanpasu_rpc_protocol::EventName::AppUpdateStateChanged)
                                && !rpc.command_capabilities_available("get_app_update_state")
                            {
                                failure = SubscriptionErrorCode::Unsupported;
                                return Err(RpcError::new(
                                    "unsupported",
                                    "application updates are unavailable in this shell",
                                ));
                            }
                            let failure = &mut failure;
                            Ok(unreported(async move {
                                let Some(event) = event else {
                                    return source.details().await.map(Subscription::Details);
                                };
                                let snapshot =
                                    subscriptions::snapshot(source.as_ref(), event, &mut receiver)
                                        .await
                                        .map_err(|code| {
                                            *failure = code;
                                            subscription_error(code)
                                        })?;
                                if frame::encode(&Frame::Reset {
                                    subscription: "s18446744073709551615".into(),
                                    event: event.as_str().into(),
                                    snapshot: snapshot.clone(),
                                })
                                .is_err()
                                {
                                    *failure = SubscriptionErrorCode::ResourceLimit;
                                    return Err(subscription_error(*failure));
                                }
                                Ok(Subscription::Events {
                                    event,
                                    receiver,
                                    snapshot,
                                })
                            }))
                        })
                        .await
                        .map_err(|_| failure);
                        Completion::Subscribe {
                            audit,
                            id,
                            result,
                            permit,
                        }
                    }
                    .with_current_subscriber(),
                );
            }
            Frame::Unsubscribe { id, subscription } => {
                let audit = self.rpc.dependencies.audit.invocation(
                    RpcTransport::Local,
                    &self.owner,
                    "local_unsubscribe",
                    Some(id),
                );
                let _ = dispatch_with_audit(&audit, || Ok(unreported(async { Ok(()) }))).await;
                self.remove_subscription(&subscription, SubscriptionOutcome::Unsubscribed)
                    .await;
                audit.record(AuditPhase::Subscription {
                    outcome: SubscriptionOutcome::Unsubscribed,
                });
                drop(permit);
                self.send_delivery(Delivery {
                    frame: Frame::Unsubscribed { id, subscription },
                    written: None,
                    audits: vec![DeliveryAudit::new(audit)],
                })
                .await;
            }
            _ => self.stop.cancel(),
        }
    }

    async fn complete(&mut self, completion: Completion) {
        match completion {
            Completion::Call {
                audit,
                id,
                result,
                opened,
                closed,
                reserved,
                log_call,
                permit,
            } => {
                if log_call {
                    self.pending_log_calls -= 1;
                }
                if reserved {
                    self.pending_resources -= 1;
                }
                if let Some(opened) = opened {
                    // Retired IDs remain registered until overlapping log calls
                    // settle, so a delayed duplicate cannot resurrect them.
                    if !self.logs.iter().any(|log| log.same_resource(&opened)) {
                        self.logs.push(opened);
                    }
                }
                if let Some(closed) = closed {
                    for log in &mut self.logs {
                        if log.same_resource(&closed) {
                            log.closed = true;
                        }
                    }
                }
                // Retired IDs still occupy their resource slot, keeping this
                // ledger bounded even if a log call stays pending indefinitely.
                self.logs
                    .retain(|log| !log.closed || self.pending_log_calls != 0);
                // Release work capacity before its reply can reach the peer.
                drop(permit);
                if self.stop.is_cancelled() {
                    audit.record(AuditPhase::Delivery {
                        outcome: DeliveryOutcome::NotAttempted,
                    });
                    self.close_logs().await;
                    return;
                }
                let reply = match result {
                    Ok(value) => Reply::Result(value),
                    Err(error) => {
                        Reply::Error(serde_json::to_value(error).expect("RPC errors serialize"))
                    }
                };
                self.send_delivery(Delivery {
                    frame: Frame::Response { id, reply },
                    written: None,
                    audits: vec![DeliveryAudit::new(audit)],
                })
                .await;
            }
            Completion::Subscribe {
                audit,
                id,
                result,
                permit,
            } => {
                self.pending_resources -= 1;
                drop(permit);
                if self.stop.is_cancelled() {
                    drop(result);
                    audit.record(AuditPhase::Subscription {
                        outcome: SubscriptionOutcome::Disconnected,
                    });
                    audit.record(AuditPhase::Delivery {
                        outcome: DeliveryOutcome::NotAttempted,
                    });
                    return;
                }
                match result {
                    Ok(stream) => {
                        let ack = match self.session.subscribed(id) {
                            Ok(ack) => ack,
                            Err(_) => {
                                drop(stream);
                                audit.record(AuditPhase::Subscription {
                                    outcome: SubscriptionOutcome::Failed,
                                });
                                audit.record(AuditPhase::Delivery {
                                    outcome: DeliveryOutcome::NotAttempted,
                                });
                                self.stop.cancel();
                                return;
                            }
                        };
                        let Frame::Subscribed {
                            ref subscription, ..
                        } = ack
                        else {
                            unreachable!("subscription acknowledgement")
                        };
                        let subscription = subscription.clone();
                        let stop = self.stop.child_token();
                        let (closed, receiver) = oneshot::channel();
                        self.subscriptions.insert(
                            subscription.clone(),
                            SubscriptionTask {
                                stop: stop.clone(),
                                closed: receiver,
                                audit: audit.clone(),
                            },
                        );
                        audit.record(AuditPhase::Subscription {
                            outcome: SubscriptionOutcome::Established,
                        });
                        self.enqueue_delivery(Delivery {
                            frame: ack,
                            written: None,
                            audits: vec![DeliveryAudit::new(audit)],
                        })
                        .await;
                        let forwarding = subscriptions::forward(
                            subscription,
                            stream,
                            self.source.clone(),
                            Output::new(self.events.clone()),
                            stop,
                        );
                        self.forwarders.spawn(async move {
                            forwarding.await;
                            let _ = closed.send(());
                        });
                    }
                    Err(code) => {
                        audit.record(AuditPhase::Subscription {
                            outcome: SubscriptionOutcome::Failed,
                        });
                        self.send_delivery(Delivery {
                            frame: Frame::SubscriptionError {
                                target: SubscriptionTarget::Id(id),
                                code,
                            },
                            written: None,
                            audits: vec![DeliveryAudit::new(audit)],
                        })
                        .await
                    }
                }
            }
        }
    }

    async fn close_logs(&mut self) {
        // Opens may return the same deduplicated ID, and explicit closes may
        // already have succeeded. Settle those operations before sweeping once;
        // unrelated accepted work does not delay log cleanup.
        if self.pending_log_calls != 0 {
            return;
        }
        for log in self.logs.drain(..) {
            if let Err(error) = self
                .rpc
                .dependencies
                .client
                .close_log_session(log.source, self.owner.label().to_owned(), log.session)
                .await
            {
                tracing::warn!(%error, "failed to close local connection log session");
            }
        }
    }
}

#[cfg(all(test, feature = "http"))]
#[path = "tests.rs"]
mod tests;

fn subscription_error(code: SubscriptionErrorCode) -> RpcError {
    let kind = match code {
        SubscriptionErrorCode::FeatureNotNegotiated => "feature_not_negotiated",
        SubscriptionErrorCode::Unsupported => "unsupported",
        SubscriptionErrorCode::UnknownStream => "unknown_stream",
        SubscriptionErrorCode::ResourceLimit => "resource_limit",
        SubscriptionErrorCode::SnapshotUnavailable => "snapshot_unavailable",
    };
    RpcError::new(kind, "local subscription admission failed")
}

fn subscription_id(frame: &Frame) -> Option<&str> {
    match frame {
        Frame::Event { subscription, .. }
        | Frame::Reset { subscription, .. }
        | Frame::Lag { subscription, .. }
        | Frame::Data { subscription, .. }
        | Frame::SubscriptionError {
            target: SubscriptionTarget::Subscription(subscription),
            ..
        } => Some(subscription),
        _ => None,
    }
}

pub(super) async fn serve_stream<S>(
    mut stream: S,
    owner: RpcOwner,
    rpc: UnifiedRpc,
    source: Arc<dyn SubscriptionSource>,
    limits: ConnectionLimits,
    stop: CancellationToken,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut session = Session::new(
        vec![Feature::Rpc, Feature::Events, Feature::ConnectionDetails],
        limits.in_flight,
        limits.resources,
    );
    let handshake = async {
        let first = match frame::read_frame(&mut stream).await {
            Ok(first) => first?,
            Err(error) => {
                let _ = frame::write_frame(
                    &mut stream,
                    &Frame::Reject {
                        code: error.reject_code(),
                    },
                )
                .await;
                return None;
            }
        };
        let accept = match session.receive(&first) {
            Ok(Some(accept)) => accept,
            Err(error) => {
                let _ = frame::write_frame(
                    &mut stream,
                    &Frame::Reject {
                        code: error.reject_code(),
                    },
                )
                .await;
                return None;
            }
            _ => return None,
        };
        frame::write_frame(&mut stream, &accept).await.ok()?;
        session.send(&accept).ok()?;
        Some(())
    };
    if tokio::select! { biased; () = stop.cancelled() => None, result = handshake => result }
        .is_none()
    {
        return;
    }
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (incoming, mut received) = mpsc::channel(limits.in_flight);
    let admission = Arc::new(Semaphore::new(limits.in_flight));
    let (outgoing, mut output) = mpsc::channel::<Delivery>(limits.output);
    let (events, mut updates) = mpsc::channel(limits.output);
    let mut io = JoinSet::new();
    let reader_stop = stop.clone();
    io.spawn(async move {
        let _cancel_on_exit = reader_stop.clone().drop_guard();
        let reading = async {
            loop {
                match frame::read_frame(&mut reader).await {
                    Ok(Some(frame)) => {
                        // Count queued and dispatched work together. The handoff
                        // fits the entire budget, and the reader never waits on
                        // saturated calls, so EOF remains visible.
                        let Ok(permit) = admission.clone().try_acquire_owned() else {
                            break;
                        };
                        if incoming.try_send(Ok((frame, permit))).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        if incoming.try_send(Err(error.reject_code())).is_ok() {
                            reader_stop.cancelled().await;
                        }
                        break;
                    }
                }
            }
        };
        tokio::select! { () = reader_stop.cancelled() => (), () = reading => () }
        reader_stop.cancel();
    });
    let writer_stop = stop.clone();
    io.spawn(
        async move {
            let _cancel_on_exit = writer_stop.clone().drop_guard();
            let writing = async {
                while let Some(mut delivery) = output.recv().await {
                    for audit in &mut delivery.audits {
                        audit.observe(DeliveryOutcome::Interrupted);
                    }
                    let result = frame::write_frame(&mut writer, &delivery.frame).await;
                    for audit in &mut delivery.audits {
                        audit.observe(if result.is_ok() {
                            DeliveryOutcome::WriteCompleted
                        } else {
                            DeliveryOutcome::WriteFailed
                        });
                    }
                    if result.is_err() || matches!(delivery.frame, Frame::Reject { .. }) {
                        break;
                    }
                    if let Some(written) = delivery.written {
                        let _ = written.send(());
                    }
                }
            };
            tokio::select! { () = writer_stop.cancelled() => (), () = writing => () }
            writer_stop.cancel();
        }
        .with_current_subscriber(),
    );
    let mut connection = Connection {
        rpc,
        source,
        owner,
        session,
        limits,
        pending_resources: 0,
        pending_log_calls: 0,
        logs: Vec::new(),
        subscriptions: HashMap::new(),
        calls: JoinSet::new(),
        forwarders: JoinSet::new(),
        outgoing,
        events,
        stop: stop.clone(),
    };
    loop {
        tokio::select! {
            biased;
            () = stop.cancelled() => break,
            Some(result) = io.join_next() => blocking::join(result),
            Some(result) = connection.calls.join_next() => connection.complete(blocking::join(result)).await,
            Some(result) = connection.forwarders.join_next() => blocking::join(result),
            Some(frame) = received.recv() => match frame {
                Ok((frame, permit)) => connection.receive(frame, permit).await,
                Err(code) => { connection.enqueue(Frame::Reject { code }).await; stop.cancelled().await; }
            },
            Some(delivery) = updates.recv() => {
                if let Some(id) = subscription_id(&delivery.frame) && connection.subscriptions.contains_key(id) {
                    if matches!(delivery.frame, Frame::SubscriptionError { .. }) { connection.remove_subscription(id, SubscriptionOutcome::Failed).await; }
                    connection.send_delivery(delivery).await;
                }
            }
        }
    }
    // Finish IO first so no admitted frame can race the final handoff drain.
    while let Some(result) = io.join_next().await {
        blocking::join(result);
    }
    while let Ok(frame) = received.try_recv() {
        if let Ok((frame, permit)) = frame {
            connection.receive(frame, permit).await;
        }
    }
    connection.session.close();
    let mut disconnected = Vec::new();
    for (_, task) in connection.subscriptions.drain() {
        task.stop.cancel();
        disconnected.push(task.audit);
    }
    connection.close_logs().await;
    // Never abort accepted work: successful late resources are registered and closed.
    while let Some(result) = connection.calls.join_next().await {
        connection.complete(blocking::join(result)).await;
    }
    connection.close_logs().await;
    while let Some(result) = connection.forwarders.join_next().await {
        blocking::join(result);
    }
    for audit in disconnected {
        audit.record(AuditPhase::Subscription {
            outcome: SubscriptionOutcome::Disconnected,
        });
    }
}
