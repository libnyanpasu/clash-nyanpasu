//! Serial exchanges and bounded latest-value subscriptions owned by one IO task.

use std::{collections::HashMap, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWrite},
    sync::{mpsc, watch},
    time::Instant,
};

use super::{
    Completed, LocalCallError, LocalSubscription, Operation, Request, SUBSCRIPTION_CAPACITY,
    SubscriptionState, TransportError,
};
use crate::frame::{
    self, Codec, Feature, Frame, PROTOCOL_VERSION, RequestId, SubscriptionErrorCode,
    SubscriptionTarget,
};

pub(super) async fn negotiate<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    timeout: Duration,
) -> Result<(), TransportError> {
    tokio::time::timeout(timeout, async {
        frame::write_frame(
            stream,
            &Frame::Hello {
                versions: vec![PROTOCOL_VERSION],
                required_features: vec!["rpc".into()],
                optional_features: vec!["events".into(), "connection-details".into()],
            },
        )
        .await
        .map_err(|source| TransportError::Frame { source })?;
        match frame::read_frame(stream)
            .await
            .map_err(|source| TransportError::Frame { source })?
        {
            Some(Frame::Accept {
                version: PROTOCOL_VERSION,
                features,
            }) if features.contains(&Feature::Rpc)
                && features
                    .iter()
                    .enumerate()
                    .all(|(i, f)| !features[..i].contains(f)) =>
            {
                Ok(())
            }
            _ => Err(TransportError::ValidateReply),
        }
    })
    .await
    .unwrap_or(Err(TransportError::TimedOut))
}

struct Subscription {
    event: Option<crate::EventName>,
    needs_reset: bool,
    state: watch::Sender<SubscriptionState>,
}

struct Pending {
    id: RequestId,
    request: Request,
    deadline: Option<Instant>,
}

struct Transport {
    pending: Option<Pending>,
    subscriptions: HashMap<String, Subscription>,
    last_id: u64,
}

impl Transport {
    fn receive(&mut self, frame: Frame) -> Result<(), TransportError> {
        let correlated = match &frame {
            Frame::Response { id, .. } => Some((
                *id,
                matches!(
                    self.pending.as_ref().map(|p| &p.request.operation),
                    Some(Operation::Call { .. })
                ),
            )),
            Frame::Subscribed { id, subscription } => Some((
                *id,
                matches!(
                    self.pending.as_ref().map(|p| &p.request.operation),
                    Some(Operation::Subscribe { .. })
                ) && !self.subscriptions.contains_key(subscription)
                    && self.subscriptions.len() < SUBSCRIPTION_CAPACITY,
            )),
            Frame::Unsubscribed { id, subscription } => Some((
                *id,
                matches!(self.pending.as_ref().map(|p| &p.request.operation), Some(Operation::Unsubscribe { subscription: expected }) if expected == subscription),
            )),
            Frame::SubscriptionError {
                target: SubscriptionTarget::Id(id),
                ..
            } => Some((
                *id,
                matches!(
                    self.pending.as_ref().map(|p| &p.request.operation),
                    Some(Operation::Subscribe { .. })
                ),
            )),
            _ => None,
        };
        if let Some((id, valid)) = correlated
            && (!valid || self.pending.as_ref().is_none_or(|pending| pending.id != id))
        {
            return Err(TransportError::ValidateReply);
        }
        match frame {
            Frame::Response { id, reply } => {
                let pending = self.take_pending(id)?;
                let _ = pending.request.reply.send(Ok(Completed::Call(reply)));
            }
            Frame::Subscribed { id, subscription } => {
                let pending = self.take_pending(id)?;
                let Operation::Subscribe { event } = pending.request.operation else {
                    unreachable!("subscription reply kind was validated before taking the waiter");
                };
                let (state, receiver) = watch::channel(SubscriptionState::Pending);
                self.subscriptions.insert(
                    subscription.clone(),
                    Subscription {
                        event,
                        needs_reset: event.is_some(),
                        state,
                    },
                );
                let _ = pending
                    .request
                    .reply
                    .send(Ok(Completed::Subscribed(LocalSubscription {
                        id: subscription,
                        state: receiver,
                    })));
            }
            Frame::Unsubscribed { id, subscription } => {
                let pending = self.take_pending(id)?;
                if let Some(stream) = self.subscriptions.remove(&subscription) {
                    stream.state.send_replace(SubscriptionState::Closed);
                }
                let _ = pending.request.reply.send(Ok(Completed::Unsubscribed));
            }
            Frame::SubscriptionError {
                target: SubscriptionTarget::Id(id),
                code,
            } => {
                let pending = self.take_pending(id)?;
                let _ = pending
                    .request
                    .reply
                    .send(Err(LocalCallError::Subscribe { code }));
            }
            Frame::SubscriptionError {
                target: SubscriptionTarget::Subscription(id),
                code,
            } => {
                let stream = self
                    .subscriptions
                    .remove(&id)
                    .ok_or(TransportError::ValidateReply)?;
                stream.state.send_replace(SubscriptionState::Failed(code));
            }
            Frame::Reset {
                subscription,
                event,
                snapshot,
            } => {
                let stream = self.event_stream(&subscription, &event)?;
                stream.needs_reset = false;
                stream
                    .state
                    .send_replace(SubscriptionState::Value(snapshot));
            }
            Frame::Event {
                subscription,
                name,
                payload,
            } => {
                let stream = self.event_stream(&subscription, &name)?;
                if stream.needs_reset {
                    return Err(TransportError::ValidateReply);
                }
                stream.state.send_replace(SubscriptionState::Value(payload));
            }
            Frame::Lag {
                subscription,
                resync: true,
                ..
            } => {
                let stream = self
                    .subscriptions
                    .get_mut(&subscription)
                    .ok_or(TransportError::ValidateReply)?;
                if stream.event.is_none() || stream.needs_reset {
                    return Err(TransportError::ValidateReply);
                }
                stream.needs_reset = true;
                stream.state.send_replace(SubscriptionState::Lagged);
            }
            Frame::Data {
                subscription,
                stream,
                payload,
            } => {
                let subscription = self
                    .subscriptions
                    .get_mut(&subscription)
                    .ok_or(TransportError::ValidateReply)?;
                if subscription.event.is_some() || stream != "connection-details" {
                    return Err(TransportError::ValidateReply);
                }
                subscription
                    .state
                    .send_replace(SubscriptionState::Details(payload));
            }
            _ => return Err(TransportError::ValidateReply),
        }
        Ok(())
    }

    fn take_pending(&mut self, id: RequestId) -> Result<Pending, TransportError> {
        if self.pending.as_ref().is_none_or(|pending| pending.id != id) {
            return Err(TransportError::ValidateReply);
        }
        Ok(self.pending.take().expect("pending ID was validated"))
    }

    fn event_stream(&mut self, id: &str, event: &str) -> Result<&mut Subscription, TransportError> {
        let stream = self
            .subscriptions
            .get_mut(id)
            .ok_or(TransportError::ValidateReply)?;
        if stream.event.is_none_or(|name| name.as_str() != event) {
            return Err(TransportError::ValidateReply);
        }
        Ok(stream)
    }
}

pub(super) async fn run<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    mut requests: mpsc::Receiver<Request>,
    mut stop: watch::Receiver<bool>,
) {
    let mut transport = Transport {
        pending: None,
        subscriptions: HashMap::new(),
        last_id: 0,
    };
    let result = exchange(&mut stream, &mut requests, &mut stop, &mut transport).await;
    // Close admission and IO before notifying waiters; this handle can never be reused.
    requests.close();
    drop(stream);
    if let Some(pending) = transport.pending.take() {
        let _ = pending
            .request
            .reply
            .send(Err(LocalCallError::Exchange { source: result }));
    }
    for (_, subscription) in transport.subscriptions {
        subscription.state.send_replace(SubscriptionState::Closed);
    }
    while let Some(request) = requests.recv().await {
        let _ = request.reply.send(Err(LocalCallError::Submit));
    }
}

async fn deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    requests: &mut mpsc::Receiver<Request>,
    stop: &mut watch::Receiver<bool>,
    transport: &mut Transport,
) -> TransportError {
    let mut codec = Codec::new();
    let mut buffer = [0u8; 8192];
    loop {
        tokio::select! {
            biased;
            _ = stop.changed() => return TransportError::Disconnected,
            () = deadline(transport.pending.as_ref().and_then(|pending| pending.deadline)) => return TransportError::TimedOut,
            request = requests.recv(), if transport.pending.is_none() => {
                let Some(request) = request else { return TransportError::Disconnected; };
                if request.reply.is_closed() { continue; }
                let Some(next) = transport.last_id.checked_add(1) else {
                    let _ = request.reply.send(Err(LocalCallError::Submit));
                    return TransportError::ValidateReply;
                };
                transport.last_id = next;
                let id = RequestId::new(next).expect("incremented request ID is positive");
                if matches!(request.operation, Operation::Subscribe { .. }) && transport.subscriptions.len() >= SUBSCRIPTION_CAPACITY {
                    let _ = request.reply.send(Err(LocalCallError::Subscribe { code: SubscriptionErrorCode::ResourceLimit }));
                    continue;
                }
                let frame = match &request.operation {
                    Operation::Call { method, params } => Frame::Request { id, method: method.clone(), params: params.clone() },
                    Operation::Subscribe { event } => Frame::Subscribe { id, stream: if event.is_some() { "events" } else { "connection-details" }.into(), event: event.map(|name| name.as_str().into()) },
                    Operation::Unsubscribe { subscription } => Frame::Unsubscribe { id, subscription: subscription.clone() },
                };
                let until = request.timeout.map(|timeout| Instant::now() + timeout);
                transport.pending = Some(Pending { id, request, deadline: until });
                // The caller no longer owns this future. Only a terminal transport
                // deadline/shutdown may interrupt a partial write, and then IO closes.
                let result = tokio::select! {
                    biased;
                    _ = stop.changed() => return TransportError::Disconnected,
                    () = deadline(until) => return TransportError::TimedOut,
                    result = frame::write_frame(stream, &frame) => result,
                };
                if let Err(source) = result { return TransportError::Frame { source }; }
            }
            read = stream.read(&mut buffer) => {
                let count = match read {
                    Ok(0) => return match codec.finish() { Ok(()) => TransportError::Disconnected, Err(source) => TransportError::Frame { source } },
                    Ok(count) => count,
                    Err(source) => return TransportError::Frame { source: frame::FrameError::ReadFrame { source } },
                };
                let mut remaining = &buffer[..count];
                while !remaining.is_empty() {
                    let (consumed, frame) = match codec.feed(remaining) { Ok(value) => value, Err(source) => return TransportError::Frame { source } };
                    remaining = &remaining[consumed..];
                    if let Some(frame) = frame
                        && let Err(error) = transport.receive(frame) { return error; }
                }
            }
        }
    }
}
