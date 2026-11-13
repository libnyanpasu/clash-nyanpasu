//! Neutral RPC envelopes, caller labels and shared event identities.

mod command;
mod command_catalog;
mod error;
mod event;
mod owner;
mod request;

pub use command::{CommandEffect, CommandKind, CommandMetadata, RpcTarget, ShellCapability};
pub use command_catalog::COMMAND_CATALOG;
pub use error::{RpcError, RpcValue};
pub use event::EventName;
/// The canonical scope value, defined by the application config domain.
pub use nyanpasu_config::application::Scope;
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
