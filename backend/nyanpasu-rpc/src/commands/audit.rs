//! Thin adapters for persisted RPC audit queries.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_observers::audit::{AuditCursor, AuditFilter, AuditPage};

use crate::{error::Result, registry::RpcState};

/// Reads persisted remote audit rows without flushing the diagnostic channel.
#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn query_rpc_audit(
    client: RpcState<'_, NyanpasuClient>,
    filter: AuditFilter,
    before: Option<AuditCursor>,
    limit: usize,
) -> Result<AuditPage> {
    Ok(client.query_rpc_audit(filter, before, limit).await?)
}
