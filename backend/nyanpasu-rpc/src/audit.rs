//! Payload-free RPC audit records, lifecycle guard and production tracing sink.

use std::{
    hash::{BuildHasher as _, RandomState},
    sync::Arc,
    time::{Duration, Instant},
};

use nyanpasu_rpc_protocol::RpcOwner;

/// The injected audit sink and the per-process key for HTTP caller identities.
pub struct RpcAudit {
    sink: Arc<dyn RpcAuditSink>,
    owner_key: RandomState,
}

impl RpcAudit {
    pub fn new(sink: Arc<dyn RpcAuditSink>) -> Self {
        Self {
            sink,
            owner_key: RandomState::new(),
        }
    }

    /// IPC callers are window labels. An HTTP owner carries the session cookie, which
    /// authorizes log sessions, so records get a keyed hash that still correlates
    /// one session's calls but never reveals the cookie.
    fn owner(&self, transport: RpcTransport, owner: &RpcOwner) -> RpcAuditOwner {
        match transport {
            RpcTransport::TauriIpc => RpcAuditOwner(owner.label().to_owned()),
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
    Cancelled,
    Panicked,
}

/// Local metadata only; this record is never serialized onto a transport.
#[derive(Debug, Clone)]
pub struct RpcAuditRecord {
    pub transport: RpcTransport,
    pub command: &'static str,
    pub owner: RpcAuditOwner,
    pub duration: Duration,
    pub outcome: RpcAuditOutcome,
}

pub(crate) struct RpcAuditGuard<'a> {
    sink: &'a dyn RpcAuditSink,
    record: RpcAuditRecord,
    started: Instant,
    completed: bool,
}

impl<'a> RpcAuditGuard<'a> {
    pub(crate) fn new(audit: &'a RpcAudit, transport: RpcTransport, owner: &RpcOwner) -> Self {
        Self {
            sink: audit.sink.as_ref(),
            record: RpcAuditRecord {
                transport,
                command: "unknown_method",
                owner: audit.owner(transport, owner),
                duration: Duration::ZERO,
                outcome: RpcAuditOutcome::Cancelled,
            },
            started: Instant::now(),
            completed: false,
        }
    }

    pub(crate) fn set_command(&mut self, command: &'static str) {
        self.record.command = command;
    }

    pub(crate) fn record(&mut self, outcome: RpcAuditOutcome) {
        self.record.duration = self.started.elapsed();
        self.record.outcome = outcome;
        self.completed = true;
        self.sink.record(&self.record);
    }
}

impl Drop for RpcAuditGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.record(if std::thread::panicking() {
                RpcAuditOutcome::Panicked
            } else {
                RpcAuditOutcome::Cancelled
            });
        }
    }
}

/// Write payload-free dispatch metadata at the existing transport-specific level.
pub struct TracingRpcAuditSink;

impl RpcAuditSink for TracingRpcAuditSink {
    fn record(&self, record: &RpcAuditRecord) {
        macro_rules! emit {
            ($level:expr) => {
                tracing::event!(target: "nyanpasu::rpc_audit", $level,
                    transport = ?record.transport,
                    command = record.command,
                    owner = record.owner.as_str(),
                    duration_ns = record.duration.as_nanos() as u64,
                    outcome = ?record.outcome,
                )
            };
        }
        match record.transport {
            RpcTransport::Http | RpcTransport::Local => emit!(tracing::Level::INFO),
            RpcTransport::TauriIpc => emit!(tracing::Level::DEBUG),
        }
    }
}
