//! Caller-scoped RPC self-description command adapter.

use nyanpasu_rpc_protocol::RpcDescribeResponse;

use crate::registry::RpcDescribeContext;

/// Returns protocol, server, transport, own principal/scopes, and per-target
/// command availability for the resolved caller.
///
/// Any resolved caller may invoke `rpc_describe`: it requires `meta:read`, which every resolved
/// caller holds (enforced by [`crate::permission::Authority::admit`]); it returns own
/// discovery metadata only and never discloses another principal or grants
/// business data access.
#[nyanpasu_rpc::rpc(
    local,
    http,
    query,
    operation = nyanpasu_rpc_protocol::operations::RpcDescribe,
    permission = meta:read,
)]
#[specta::specta]
pub fn rpc_describe(context: RpcDescribeContext) -> RpcDescribeResponse {
    context.into_response()
}
