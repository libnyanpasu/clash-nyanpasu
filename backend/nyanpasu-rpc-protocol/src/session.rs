//! Pure server-side admission and ordered-output state, with no dispatch or resource owners.
//!
//! The connection adapter closes on protocol errors, owns cleanup, and calls output
//! transitions at its sole writer after dropping obsolete queued frames. Successful
//! admission is permission to dispatch, not a claim that owner work has completed.

use std::collections::HashMap;

use snafu::Snafu;

use crate::{
    EventName,
    frame::{
        Feature, Frame, PROTOCOL_VERSION, RejectCode, RequestId, SubscriptionErrorCode,
        SubscriptionTarget,
    },
};

/// Only these five identities have complete values suitable for initial/lag reset.
pub const RECOVERABLE_EVENTS: [EventName; 5] = [
    EventName::ConfigurationStatusChanged,
    EventName::CoreStatusChanged,
    EventName::ServiceStatusChanged,
    EventName::AppUpdateStateChanged,
    EventName::CoreLogsChanged,
];

#[derive(Debug)]
enum Pending {
    Request,
    Subscribe(Stream),
    Unsubscribe(String),
}

#[derive(Debug)]
enum Stream {
    Events { event: String, needs_reset: bool },
    Details,
}

#[derive(Debug)]
enum Phase {
    Handshake,
    AcceptPending(Vec<Feature>),
    Ready(Vec<Feature>),
    Closed,
}

/// Terminal local protocol failure; the code alone is safe for outer rejection.
#[derive(Debug, Snafu)]
pub enum SessionError {
    /// Negotiation, correlation or output ordering failed; close the connection.
    #[snafu(display("local session rejected: {code:?}"))]
    ValidateSession { code: RejectCode },
}

impl SessionError {
    /// Send this outer rejection when the connection can still write safely.
    pub fn reject_code(&self) -> RejectCode {
        match self {
            Self::ValidateSession { code } => *code,
        }
    }
}

/// One connection's bounded protocol state. Never share it between connections.
#[derive(Debug)]
pub struct Session {
    supported: Vec<Feature>,
    phase: Phase,
    last_request: Option<RequestId>,
    next_subscription: u64,
    pending: HashMap<RequestId, Pending>,
    subscriptions: HashMap<String, Stream>,
    max_pending: usize,
    max_subscriptions: usize,
}

impl Session {
    /// Supply the server's supported features and connection resource bounds.
    /// RPC is required on both sides; a server lacking it rejects every hello.
    pub fn new(supported: Vec<Feature>, max_pending: usize, max_subscriptions: usize) -> Self {
        Self {
            supported,
            phase: Phase::Handshake,
            last_request: None,
            next_subscription: 1,
            pending: HashMap::new(),
            subscriptions: HashMap::new(),
            max_pending,
            max_subscriptions,
        }
    }

    /// Admit one decoded client frame. `Some` is a control reply, with no dispatch;
    /// `None` admits a request/subscription operation. Control errors are already
    /// validated; pass only accept through `send` at the writer before further
    /// requests. Capability checks and cleanup belong to RPC.
    ///
    /// # Errors
    /// Invalid handshake, direction, ordering or exhausted request capacity closes
    /// the session. The adapter must close the transport and clean up its resources.
    pub fn receive(&mut self, frame: &Frame) -> Result<Option<Frame>, SessionError> {
        let result = self.receive_inner(frame);
        if result.is_err() {
            self.close();
        }
        result
    }

    fn receive_inner(&mut self, frame: &Frame) -> Result<Option<Frame>, SessionError> {
        self.ensure(
            !matches!(self.phase, Phase::Closed),
            RejectCode::InvalidFrame,
        )?;
        if let Frame::Hello {
            versions,
            required_features,
            optional_features,
        } = frame
        {
            self.ensure(
                matches!(self.phase, Phase::Handshake),
                RejectCode::InvalidFrame,
            )?;
            // Version 1 is the only implemented version, hence the highest common one.
            self.ensure(
                versions.contains(&PROTOCOL_VERSION),
                RejectCode::UnsupportedVersion,
            )?;
            self.ensure(
                required_features.iter().any(|name| name == "rpc")
                    && self.supported.contains(&Feature::Rpc)
                    && required_features.iter().all(|name| {
                        self.supported
                            .iter()
                            .any(|feature| feature.as_str() == name)
                    }),
                RejectCode::UnsupportedFeature,
            )?;
            let features: Vec<_> = [Feature::Rpc, Feature::Events, Feature::ConnectionDetails]
                .into_iter()
                .filter(|feature| {
                    self.supported.contains(feature)
                        && (required_features
                            .iter()
                            .any(|name| name == feature.as_str())
                            || optional_features
                                .iter()
                                .any(|name| name == feature.as_str()))
                })
                .collect();
            self.phase = Phase::AcceptPending(features.clone());
            return Ok(Some(Frame::Accept {
                version: PROTOCOL_VERSION,
                features,
            }));
        }
        self.ensure(
            matches!(self.phase, Phase::Ready(_)),
            RejectCode::InvalidFrame,
        )?;
        let id = match frame {
            Frame::Request { id, .. }
            | Frame::Subscribe { id, .. }
            | Frame::Unsubscribe { id, .. } => *id,
            _ => {
                return Err(SessionError::ValidateSession {
                    code: RejectCode::InvalidFrame,
                });
            }
        };
        self.ensure(
            self.last_request.is_none_or(|last| id > last),
            RejectCode::InvalidRequestId,
        )?;
        self.last_request = Some(id);
        let pending = match frame {
            Frame::Request { .. } => Pending::Request,
            Frame::Subscribe { stream, event, .. } => {
                let (feature, stream) = match (stream.as_str(), event.as_deref()) {
                    ("events", Some(event))
                        if RECOVERABLE_EVENTS.iter().any(|name| name.as_str() == event) =>
                    {
                        (
                            Feature::Events,
                            Stream::Events {
                                event: event.to_owned(),
                                needs_reset: true,
                            },
                        )
                    }
                    ("connection-details", None) => (Feature::ConnectionDetails, Stream::Details),
                    _ => {
                        return Ok(Some(Self::subscription_error(
                            id,
                            SubscriptionErrorCode::UnknownStream,
                        )));
                    }
                };
                if !matches!(&self.phase, Phase::Ready(features) if features.contains(&feature)) {
                    return Ok(Some(Self::subscription_error(
                        id,
                        SubscriptionErrorCode::FeatureNotNegotiated,
                    )));
                }
                let pending_subscriptions = self
                    .pending
                    .values()
                    .filter(|pending| matches!(pending, Pending::Subscribe(_)))
                    .count();
                if self.subscriptions.len() + pending_subscriptions >= self.max_subscriptions
                    || self.pending.len() >= self.max_pending
                {
                    return Ok(Some(Self::subscription_error(
                        id,
                        SubscriptionErrorCode::ResourceLimit,
                    )));
                }
                Pending::Subscribe(stream)
            }
            Frame::Unsubscribe { subscription, .. } => {
                // The adapter stops production and drops queued frames before its ack.
                self.subscriptions.remove(subscription);
                Pending::Unsubscribe(subscription.clone())
            }
            _ => unreachable!("client frame direction was validated above"),
        };
        self.ensure(
            self.pending.len() < self.max_pending,
            RejectCode::InvalidFrame,
        )?;
        self.pending.insert(id, pending);
        Ok(None)
    }

    fn subscription_error(id: RequestId, code: SubscriptionErrorCode) -> Frame {
        Frame::SubscriptionError {
            target: SubscriptionTarget::Id(id),
            code,
        }
    }

    /// Complete establishment and generate its never-reused opaque ID. Call only
    /// at the ordered writer; event streams still require an initial reset.
    ///
    /// # Errors
    /// Rejects unrelated/completed IDs, closed sessions and subscription ID exhaustion.
    pub fn subscribed(&mut self, id: RequestId) -> Result<Frame, SessionError> {
        if !matches!(self.phase, Phase::Ready(_))
            || self.next_subscription == u64::MAX
            || !matches!(self.pending.get(&id), Some(Pending::Subscribe(_)))
        {
            self.close();
            return Err(SessionError::ValidateSession {
                code: RejectCode::InvalidFrame,
            });
        }
        let Some(Pending::Subscribe(stream)) = self.pending.remove(&id) else {
            unreachable!("subscription pending kind was validated");
        };
        let subscription = format!("s{}", self.next_subscription);
        self.next_subscription += 1;
        self.subscriptions.insert(subscription.clone(), stream);
        Ok(Frame::Subscribed { id, subscription })
    }

    /// Validate an output at the sole ordered writer, before serializing it.
    /// Pass accept returned by `receive` here; other control replies returned by
    /// `receive` or `subscribed` are already validated. No event/data may pass
    /// after unsubscribe or failure.
    ///
    /// # Errors
    /// Invalid correlation, wrong streams or missing reset closes the session.
    pub fn send(&mut self, frame: &Frame) -> Result<(), SessionError> {
        let result = self.send_inner(frame);
        if result.is_err() {
            self.close();
        }
        result
    }

    fn send_inner(&mut self, frame: &Frame) -> Result<(), SessionError> {
        if let Frame::Accept { version, features } = frame {
            self.ensure(*version == PROTOCOL_VERSION
                && matches!(&self.phase, Phase::AcceptPending(expected) if expected == features),
                RejectCode::InvalidFrame)?;
            self.phase = Phase::Ready(features.clone());
            return Ok(());
        }
        self.ensure(
            matches!(self.phase, Phase::Ready(_)),
            RejectCode::InvalidFrame,
        )?;
        let valid = match frame {
            Frame::Response { id, .. } => matches!(self.pending.remove(id), Some(Pending::Request)),
            Frame::Unsubscribed { id, subscription } => {
                if matches!(self.pending.remove(id), Some(Pending::Unsubscribe(expected)) if expected == *subscription)
                {
                    // Establishment may have completed since receiving unsubscribe.
                    self.subscriptions.remove(subscription);
                    // Skip acknowledged IDs even if establishment has not completed.
                    // The allocation high-water mark bounds retirement bookkeeping.
                    if let Some(number) = subscription
                        .strip_prefix('s')
                        .and_then(|number| number.parse::<u64>().ok())
                        && *subscription == format!("s{number}")
                    {
                        self.next_subscription =
                            self.next_subscription.max(number.saturating_add(1));
                    }
                    true
                } else {
                    false
                }
            }
            Frame::SubscriptionError {
                target: SubscriptionTarget::Id(id),
                ..
            } => matches!(self.pending.remove(id), Some(Pending::Subscribe(_))),
            Frame::SubscriptionError {
                target: SubscriptionTarget::Subscription(subscription),
                ..
            } => self.subscriptions.remove(subscription).is_some(),
            Frame::Reset {
                subscription,
                event,
                ..
            } => match self.subscriptions.get_mut(subscription) {
                Some(Stream::Events {
                    event: expected,
                    needs_reset,
                }) if expected == event => {
                    *needs_reset = false;
                    true
                }
                _ => false,
            },
            Frame::Lag {
                subscription,
                resync: true,
                ..
            } => match self.subscriptions.get_mut(subscription) {
                Some(Stream::Events { needs_reset, .. }) if !*needs_reset => {
                    *needs_reset = true;
                    true
                }
                _ => false,
            },
            Frame::Event {
                subscription, name, ..
            } => {
                matches!(self.subscriptions.get(subscription), Some(Stream::Events { event, needs_reset: false }) if event == name)
            }
            Frame::Data {
                subscription,
                stream,
                ..
            } => {
                stream == "connection-details"
                    && matches!(self.subscriptions.get(subscription), Some(Stream::Details))
            }
            _ => false,
        };
        self.ensure(valid, RejectCode::InvalidFrame)
    }

    fn ensure(&self, condition: bool, code: RejectCode) -> Result<(), SessionError> {
        if condition {
            Ok(())
        } else {
            Err(SessionError::ValidateSession { code })
        }
    }

    /// End protocol admission. The adapter separately releases all resource owners;
    /// late completions still require cleanup even though protocol state is gone.
    pub fn close(&mut self) {
        self.phase = Phase::Closed;
        self.pending.clear();
        self.subscriptions.clear();
    }
}
