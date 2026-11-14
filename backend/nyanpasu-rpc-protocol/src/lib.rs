//! Neutral RPC envelopes, caller labels and shared event identities.

mod command;
mod command_catalog;
mod describe;
mod error;
mod event;
mod owner;
mod request;

pub use command::{CommandEffect, CommandKind, CommandMetadata, RpcTarget, ShellCapability};
pub use command_catalog::COMMAND_CATALOG;
pub use describe::{DescribedCommand, RpcDescribeResponse, ServerInfo, ServerKind};
pub use error::{ResponseVisibility, RpcError, RpcValue};
pub use event::EventName;
/// Canonical authentication and scope domain types shared across core and transports.
pub use nyanpasu_contracts::auth::{AuthContext, Principal, Scope, ScopeSet};
pub use owner::RpcOwner;
pub use request::Request;

pub mod frame;
pub mod operation;
pub mod operations;
pub mod session;
pub mod types;
pub mod values;

#[cfg(feature = "local-client")]
pub mod client;
