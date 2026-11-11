//! Payload-free RPC audit records, lifecycle guard and production tracing sink.

use std::{
    hash::{BuildHasher as _, RandomState},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use nyanpasu_rpc_protocol::RpcOwner;
use nyanpasu_runtime_values::runtime::{CommitReceipt, MutationOutcome};

/// The injected audit sink, invocation counter and instance key for remote owners.
pub struct RpcAudit {
    sink: Arc<dyn RpcAuditSink>,
    owner_key: RandomState,
    next_invocation: AtomicU64,
}

impl RpcAudit {
    pub fn new(sink: Arc<dyn RpcAuditSink>) -> Self {
        Self {
            sink,
            owner_key: RandomState::new(),
            next_invocation: AtomicU64::new(1),
        }
    }

    /// Start one invocation's diagnostic chain, independent of response ownership.
    pub(crate) fn invocation(
        &self,
        transport: RpcTransport,
        owner: &RpcOwner,
        method: &'static str,
        request_id: Option<nyanpasu_rpc_protocol::frame::RequestId>,
    ) -> AuditInvocation {
        AuditInvocation {
            sink: self.sink.clone(),
            header: AuditHeader {
                invocation_id: self.next_invocation.fetch_add(1, Ordering::Relaxed),
                transport,
                method,
                owner: self.owner(transport, owner),
                request_id,
            },
            started: Instant::now(),
        }
    }

    /// IPC callers are window labels. An HTTP owner carries the session cookie, which
    /// authorizes log sessions, so records get a keyed hash that still correlates
    /// one session's calls but never reveals the cookie.
    fn owner(&self, transport: RpcTransport, owner: &RpcOwner) -> RpcAuditOwner {
        match transport {
            RpcTransport::TauriIpc | RpcTransport::TauriProtocol => {
                RpcAuditOwner(owner.label().to_owned())
            }
            RpcTransport::Http => RpcAuditOwner(format!(
                "http:{:016x}",
                self.owner_key.hash_one(owner.label())
            )),
            RpcTransport::Local => RpcAuditOwner(format!(
                "local:{:016x}",
                self.owner_key.hash_one(owner.label())
            )),
        }
    }
}

/// A caller identity that is safe to log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcAuditOwner(pub(crate) String);

impl RpcAuditOwner {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Synchronous observation only: implementations must not panic or queue work.
pub trait RpcAuditSink: Send + Sync + 'static {
    fn record(&self, record: &RpcAuditRecord);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcTransport {
    TauriIpc,
    TauriProtocol,
    Http,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcAuditOutcome {
    Success,
    Failure {
        kind: &'static str,
        code: Option<String>,
    },
    /// The dispatch future was dropped; owner-started work may still complete.
    NotObserved,
    Panicked,
}

/// Per-invocation correlation, never serialized onto a transport.
#[derive(Debug, Clone)]
pub struct AuditHeader {
    pub invocation_id: u64,
    pub transport: RpcTransport,
    pub method: &'static str,
    pub owner: RpcAuditOwner,
    pub request_id: Option<nyanpasu_rpc_protocol::frame::RequestId>,
}

/// Receipt metadata only. `NotReported` makes no claim about side effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionSummary {
    NotReported,
    Mutation {
        commits: Vec<CommitReceipt>,
        notifications_pending: bool,
        degradation_count: usize,
    },
}

/// Extract metadata before erasing the typed success value, without inspecting it.
#[must_use]
pub fn summarize_mutation<T>(outcome: &MutationOutcome<T>) -> CompletionSummary {
    match outcome {
        MutationOutcome::Committed {
            commits,
            notifications_pending,
            ..
        } => CompletionSummary::Mutation {
            commits: commits.clone(),
            notifications_pending: *notifications_pending,
            degradation_count: 0,
        },
        MutationOutcome::CommittedDegraded {
            commits,
            notifications_pending,
            degradations,
            ..
        } => CompletionSummary::Mutation {
            commits: commits.clone(),
            notifications_pending: *notifications_pending,
            degradation_count: degradations.len(),
        },
    }
}

/// Phases follow preparation and execution, rather than the returned error kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditPhase {
    Rejected {
        kind: &'static str,
        code: Option<String>,
    },
    Admitted,
    Finished {
        outcome: RpcAuditOutcome,
        completion: CompletionSummary,
    },
    Delivery {
        outcome: DeliveryOutcome,
    },
    ObservationEnded {
        reason: ObservationEnd,
    },
    Subscription {
        outcome: SubscriptionOutcome,
    },
}

/// Adapter facts only; a completed write does not prove client processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOutcome {
    WriteCompleted,
    WriteFailed,
    NotAttempted,
    Interrupted,
    /// HTTP constructed a response, without observing body delivery.
    ResponsePrepared,
}

/// Why a dispatch stopped observing its handler before receiving an outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationEnd {
    /// The future was dropped; owner-started business work may still complete.
    FutureDropped,
}

/// Subscription lifecycle facts, recorded after receiver teardown on termination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionOutcome {
    Established,
    Unsubscribed,
    Disconnected,
    Failed,
}

/// Local diagnostic metadata; duration is monotonic elapsed time since dispatch.
#[derive(Debug, Clone)]
pub struct RpcAuditRecord {
    pub header: AuditHeader,
    pub duration: Duration,
    pub phase: AuditPhase,
}

/// Cloneable correlation only. It owns no handler, response or transport resource.
#[derive(Clone)]
pub(crate) struct AuditInvocation {
    sink: Arc<dyn RpcAuditSink>,
    header: AuditHeader,
    started: Instant,
}

impl AuditInvocation {
    pub(crate) fn record(&self, phase: AuditPhase) {
        self.sink.record(&RpcAuditRecord {
            header: self.header.clone(),
            duration: self.started.elapsed(),
            phase,
        });
    }
}

pub(crate) struct RpcAuditGuard<'a> {
    invocation: &'a AuditInvocation,
    completed: bool,
    admitted: bool,
}

impl<'a> RpcAuditGuard<'a> {
    pub(crate) fn new(invocation: &'a AuditInvocation) -> Self {
        Self {
            invocation,
            completed: false,
            admitted: false,
        }
    }

    pub(crate) fn admit(&mut self) {
        self.admitted = true;
        self.record(AuditPhase::Admitted);
    }

    pub(crate) fn record(&mut self, phase: AuditPhase) {
        self.completed = !matches!(phase, AuditPhase::Admitted);
        self.invocation.record(phase);
    }
}

impl Drop for RpcAuditGuard<'_> {
    fn drop(&mut self) {
        if self.admitted && !self.completed {
            let outcome = if std::thread::panicking() {
                RpcAuditOutcome::Panicked
            } else {
                RpcAuditOutcome::NotObserved
            };
            let unobserved = outcome == RpcAuditOutcome::NotObserved;
            self.record(AuditPhase::Finished {
                outcome,
                completion: CompletionSummary::NotReported,
            });
            if unobserved {
                self.invocation.record(AuditPhase::ObservationEnded {
                    reason: ObservationEnd::FutureDropped,
                });
            }
        }
    }
}

/// Write payload-free dispatch metadata at the existing transport-specific level.
pub struct TracingRpcAuditSink;

impl RpcAuditSink for TracingRpcAuditSink {
    fn record(&self, record: &RpcAuditRecord) {
        let reason = match record.phase {
            AuditPhase::ObservationEnded {
                reason: ObservationEnd::FutureDropped,
            } => Some("future_dropped"),
            _ => None,
        };
        let (phase, outcome, kind, code, completion) = match &record.phase {
            AuditPhase::Rejected { kind, code } => {
                ("rejected", None, Some(*kind), code.as_deref(), None)
            }
            AuditPhase::Admitted => ("admitted", None, None, None, None),
            AuditPhase::Delivery { outcome } => (
                "delivery",
                Some(match outcome {
                    DeliveryOutcome::WriteCompleted => "write_completed",
                    DeliveryOutcome::WriteFailed => "write_failed",
                    DeliveryOutcome::NotAttempted => "not_attempted",
                    DeliveryOutcome::Interrupted => "interrupted",
                    DeliveryOutcome::ResponsePrepared => "response_prepared",
                }),
                None,
                None,
                None,
            ),
            AuditPhase::ObservationEnded {
                reason: ObservationEnd::FutureDropped,
            } => (
                "observation_ended",
                Some("outcome_unobserved"),
                None,
                None,
                None,
            ),
            AuditPhase::Subscription { outcome } => (
                "subscription",
                Some(match outcome {
                    SubscriptionOutcome::Established => "established",
                    SubscriptionOutcome::Unsubscribed => "unsubscribed",
                    SubscriptionOutcome::Disconnected => "disconnected",
                    SubscriptionOutcome::Failed => "failed",
                }),
                None,
                None,
                None,
            ),
            AuditPhase::Finished {
                outcome,
                completion,
            } => {
                let (outcome, kind, code) = match outcome {
                    RpcAuditOutcome::Success => ("success", None, None),
                    RpcAuditOutcome::Failure { kind, code } => {
                        ("failure", Some(*kind), code.as_deref())
                    }
                    RpcAuditOutcome::NotObserved => ("not_observed", None, None),
                    RpcAuditOutcome::Panicked => ("panicked", None, None),
                };
                ("finished", Some(outcome), kind, code, Some(completion))
            }
        };
        let (completion, commits, notifications_pending, degradation_count) = match completion {
            Some(CompletionSummary::Mutation {
                commits,
                notifications_pending,
                degradation_count,
            }) => (
                Some("mutation"),
                Some(serde_json::to_string(commits).expect("commit receipt metadata serializes")),
                Some(*notifications_pending),
                Some(*degradation_count as u64),
            ),
            Some(CompletionSummary::NotReported) => (Some("not_reported"), None, None, None),
            None => (None, None, None, None),
        };
        macro_rules! emit {
            ($level:expr) => {
                tracing::event!(target: "nyanpasu::rpc_audit", $level,
                    invocation_id = record.header.invocation_id,
                    transport = match record.header.transport { RpcTransport::Http => "Http", RpcTransport::Local => "Local", RpcTransport::TauriIpc => "TauriIpc", RpcTransport::TauriProtocol => "TauriProtocol" },
                    command = record.header.method,
                    owner = record.header.owner.as_str(),
                    request_id = record.header.request_id.map(|id| serde_json::to_string(&id).expect("request ID serializes")),
                    duration_ns = record.duration.as_nanos() as u64,
                    phase, outcome, reason, kind, code, completion,
                    commits = commits.as_deref(), notifications_pending, degradation_count,
                )
            };
        }
        match record.header.transport {
            RpcTransport::Http | RpcTransport::Local => emit!(tracing::Level::INFO),
            RpcTransport::TauriIpc | RpcTransport::TauriProtocol => emit!(tracing::Level::DEBUG),
        }
    }
}
