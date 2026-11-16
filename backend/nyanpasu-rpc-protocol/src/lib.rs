//! Neutral RPC envelopes, caller labels and shared event identities.

mod command;
mod error;
mod event;
mod owner;
mod request;

pub use command::{CommandKind, ShellCapability};
pub use error::{ResponseVisibility, RpcError, RpcValue};
pub use event::EventName;
/// Canonical authentication and scope domain types shared across core and transports.
pub use nyanpasu_contracts::auth::{AuthContext, Principal, Scope, ScopeSet};
pub use nyanpasu_rpc_wire::{
    CommandEffect, DescribedCommand, RpcDescribeResponse, RpcTarget, ServerInfo, ServerKind,
};
pub use owner::{ClientNonce, ClientNonceError, MAX_CLIENT_NONCE_LEN, RpcOwner};
pub use request::Request;

pub use nyanpasu_rpc_wire::frame;
pub mod operation;
pub mod operations;
pub mod permission;
pub mod session;
pub mod types;
pub mod values;

#[cfg(feature = "local-client")]
pub mod client;

#[cfg(test)]
mod frame_tests;
