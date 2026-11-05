//! Emits RPC dispatch metadata through the shell's tracing subscriber.

use crate::unified_rpc::{RpcAuditRecord, RpcAuditSink, RpcTransport};

pub(crate) struct TracingRpcAuditSink;

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
            RpcTransport::Http => emit!(tracing::Level::INFO),
            RpcTransport::TauriIpc => emit!(tracing::Level::DEBUG),
        }
    }
}
