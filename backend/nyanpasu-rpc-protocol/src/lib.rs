//! Neutral RPC envelopes, caller labels and shared event identities.

mod command;
mod command_catalog;
mod error;
mod event;
mod owner;
mod request;

pub use command::{CommandKind, CommandMetadata, RpcTarget, ShellCapability};
pub use command_catalog::COMMAND_CATALOG;
pub use error::{RpcError, RpcValue};
pub use event::EventName;
pub use owner::RpcOwner;
pub use request::Request;

pub mod frame;
pub mod session;
pub mod values;
pub mod wire;

#[cfg(feature = "local-client")]
pub mod client;
