//! The transport-neutral RPC layer every shell serves: the command registry
//! with its per-target allowlists, the event bus that frontends subscribe to,
//! the error mapping from application failures to the wire envelope, and the
//! shared commands that need no shell context.
//!
//! Transports are adapters over [`UnifiedRpc`]: the desktop shell binds Tauri
//! IPC, [`http`] serves the WebUI and `serve_local` the CLI. Nothing here
//! depends on a GUI.

// The rpc macro names the registry by its crate path, including inside this crate.
extern crate self as nyanpasu_rpc;

pub mod commands;
mod error;
mod events;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "local-server")]
mod local;
mod registry;
mod web_ticket;

pub use error::{IpcError, IpcErrorKind, application_error, application_error_ref};
pub use events::{EventBus, EventSink, RpcEvent};
#[cfg(feature = "local-server")]
pub use local::{LocalServeError, serve_local};
pub use registry::{
    CommandEntry, RegistryError, RpcDependencies, RpcFuture, RpcHandler, State, UnifiedRpc,
    WeakUnifiedRpc,
};
pub use web_ticket::{WebTicket, WebTicketIssuer, WebTickets, WebUiUnavailable};
