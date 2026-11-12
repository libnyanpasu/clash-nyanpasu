//! Local transport adapters; endpoint ownership is independent of RPC dispatch.

use nyanpasu_single_instance::endpoint::Namespace;

pub mod server;
mod subscriptions;

/// Where desktop, server and CLI find the local RPC listener of one data directory.
pub const ENDPOINT_NAMESPACE: Namespace = Namespace::new("nyanpasu-rpc");
