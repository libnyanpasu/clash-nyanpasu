//! The RPC wire contract: what a shell and its clients exchange without
//! linking the application. It holds the error envelope, the transport targets
//! a command opts into, the shell capabilities a command needs, caller
//! identities, event names and the local-socket frames.
//!
//! The registry that dispatches commands lives in `nyanpasu-rpc`, above core;
//! a client such as the CLI links only this crate.

#[cfg(feature = "local-client")]
mod client;
mod error;
mod event;
pub mod frame;
mod owner;
mod target;

#[cfg(feature = "local-client")]
pub use client::{LocalCallError, LocalClient};
pub use error::{RpcError, RpcErrorKind, RpcValue};
pub use event::{EventName, UnknownEventName};
pub use owner::RpcOwner;
pub use target::{RpcTarget, ShellCapability};
