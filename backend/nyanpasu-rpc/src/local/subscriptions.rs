//! Full-value snapshots and connection-detail demand owned by a local connection.

use std::sync::Arc;

use nyanpasu_core::{clash::ws::ClashConnectionDetails, client::NyanpasuClient};
use nyanpasu_rpc_protocol::{
    EventName, RpcError,
    frame::{Frame, LagReason, SubscriptionErrorCode, SubscriptionTarget},
    types::CoreLogsChanged,
};
use nyanpasu_runtime_values::core_status::CoreStatusInfo;
use serde_json::Value;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

use crate::{
    audit::{AuditInvocation, AuditPhase, DeliveryOutcome},
    error,
};

// Bound recovery work even when a publisher never becomes idle.
const SNAPSHOT_ATTEMPTS: usize = 4;
const SNAPSHOT_DRAIN_LIMIT: usize = 512;

pub(super) type Details = watch::Receiver<Option<Arc<ClashConnectionDetails>>>;
pub(super) type Events = broadcast::Receiver<(&'static str, Value)>;

pub(super) struct Delivery {
    pub frame: Frame,
    pub written: Option<oneshot::Sender<()>>,
    pub audits: Vec<DeliveryAudit>,
}

/// A write observation may be associated with several invocation headers.
pub(super) struct DeliveryAudit {
    invocation: AuditInvocation,
    outcome: DeliveryOutcome,
}

impl DeliveryAudit {
    pub(super) fn new(invocation: AuditInvocation) -> Self {
        Self {
            invocation,
            outcome: DeliveryOutcome::NotAttempted,
        }
    }

    pub(super) fn observe(&mut self, outcome: DeliveryOutcome) {
        self.outcome = outcome;
    }
}

impl Drop for DeliveryAudit {
    fn drop(&mut self) {
        self.invocation.record(AuditPhase::Delivery {
            outcome: self.outcome,
        });
    }
}

pub(super) struct Output(mpsc::Sender<Delivery>);

impl Output {
    pub(super) fn new(sender: mpsc::Sender<Delivery>) -> Self {
        Self(sender)
    }

    // One frame in flight per stream. A blocked writer leaves details in the
    // watch receiver, where successive values coalesce instead of becoming history.
    async fn send(&self, frame: Frame) -> Result<(), ()> {
        let (written, receiver) = oneshot::channel();
        self.0
            .send(Delivery {
                frame,
                written: Some(written),
                audits: Vec::new(),
            })
            .await
            .map_err(|_| ())?;
        receiver.await.map_err(|_| ())
    }
}

/// The transport consumes snapshots and demand without calling public RPC queries.
#[async_trait::async_trait]
pub(super) trait SubscriptionSource: Send + Sync {
    async fn snapshot(&self, event: EventName) -> Result<Value, RpcError>;
    async fn details(&self) -> Result<Details, RpcError>;
}

#[async_trait::async_trait]
impl SubscriptionSource for NyanpasuClient {
    async fn snapshot(&self, event: EventName) -> Result<Value, RpcError> {
        let value = match event {
            EventName::ConfigurationStatusChanged => {
                serde_json::to_value(self.configuration_status())
            }
            EventName::CoreStatusChanged => {
                serde_json::to_value(CoreStatusInfo::from(self.core_status()))
            }
            EventName::ServiceStatusChanged => serde_json::to_value(self.service_status()),
            EventName::AppUpdateStateChanged => serde_json::to_value(
                self.get_app_update_state()
                    .await
                    .map_err(error::application_error)?,
            ),
            EventName::CoreLogsChanged => serde_json::to_value(CoreLogsChanged {
                status: self
                    .get_core_log_status()
                    .await
                    .map_err(error::application_error)?,
            }),
            _ => unreachable!("local subscription admission only permits full-value events"),
        };
        value.map_err(error::application_error)
    }

    async fn details(&self) -> Result<Details, RpcError> {
        self.subscribe_clash_connection_details()
            .await
            .map_err(error::application_error)
    }
}

pub(super) enum Subscription {
    Events {
        event: EventName,
        receiver: Events,
        snapshot: Value,
    },
    Details(Details),
}

/// Retry a snapshot if its source publishes while it is being read. A finite
/// retry budget turns continuous overflow into an explicit resource-limit error.
pub(super) async fn snapshot(
    source: &dyn SubscriptionSource,
    event: EventName,
    receiver: &mut Events,
) -> Result<Value, SubscriptionErrorCode> {
    for _ in 0..SNAPSHOT_ATTEMPTS {
        // Queued observations may predate the facade's current snapshot.
        drain(receiver, event);
        let value = source
            .snapshot(event)
            .await
            .map_err(|_| SubscriptionErrorCode::SnapshotUnavailable)?;
        if !drain(receiver, event) {
            return Ok(value);
        }
    }
    Err(SubscriptionErrorCode::ResourceLimit)
}

fn drain(receiver: &mut Events, event: EventName) -> bool {
    let mut changed = false;
    // The channel is bounded, but an indefinitely busy producer must not trap us.
    for _ in 0..SNAPSHOT_DRAIN_LIMIT {
        match receiver.try_recv() {
            Ok((name, _)) => changed |= name == event.as_str(),
            Err(broadcast::error::TryRecvError::Lagged(_)) => changed = true,
            Err(_) => return changed,
        }
    }
    true
}

pub(super) async fn forward(
    subscription: String,
    stream: Subscription,
    source: Arc<dyn SubscriptionSource>,
    output: Output,
    stop: CancellationToken,
) {
    let forwarding = async {
        match stream {
            Subscription::Details(mut receiver) => loop {
                let current = receiver.borrow_and_update().clone();
                if let Some(current) = current {
                    let Ok(payload) = serde_json::to_value(current) else {
                        break;
                    };
                    if output
                        .send(Frame::Data {
                            subscription: subscription.clone(),
                            stream: "connection-details".into(),
                            payload,
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                if receiver.changed().await.is_err() {
                    break;
                }
            },
            Subscription::Events {
                event,
                mut receiver,
                snapshot: initial,
            } => {
                if output
                    .send(Frame::Reset {
                        subscription: subscription.clone(),
                        event: event.as_str().into(),
                        snapshot: initial,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
                loop {
                    match receiver.recv().await {
                        Ok((name, _)) if name == event.as_str() => {
                            // A source forwarder may publish an older queued observation
                            // after a reset overtook it. Read the authoritative full value
                            // on each notification instead of replaying that old payload.
                            let value = match source.snapshot(event).await {
                                Ok(value) => value,
                                Err(_) => {
                                    failed(
                                        &output,
                                        &subscription,
                                        SubscriptionErrorCode::SnapshotUnavailable,
                                    )
                                    .await;
                                    return;
                                }
                            };
                            if output
                                .send(Frame::Event {
                                    subscription: subscription.clone(),
                                    name: name.into(),
                                    payload: value,
                                })
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        Ok(_) => (),
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            if output
                                .send(Frame::Lag {
                                    subscription: subscription.clone(),
                                    reason: LagReason::Overflow,
                                    resync: true,
                                })
                                .await
                                .is_err()
                            {
                                return;
                            }
                            let value = match snapshot(source.as_ref(), event, &mut receiver).await
                            {
                                Ok(value) => value,
                                Err(code) => {
                                    failed(&output, &subscription, code).await;
                                    return;
                                }
                            };
                            if output
                                .send(Frame::Reset {
                                    subscription: subscription.clone(),
                                    event: event.as_str().into(),
                                    snapshot: value,
                                })
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
        failed(
            &output,
            &subscription,
            SubscriptionErrorCode::SnapshotUnavailable,
        )
        .await;
    };
    tokio::select! { () = stop.cancelled() => (), () = forwarding => () }
}

async fn failed(output: &Output, subscription: &str, code: SubscriptionErrorCode) {
    let _ = output
        .send(Frame::SubscriptionError {
            target: SubscriptionTarget::Subscription(subscription.to_owned()),
            code,
        })
        .await;
}
