//! Dependency-light wire layer of the local RPC.
//!
//! Everything a client of a running shell needs and nothing it does not: bounded JSON
//! framing, the raw local client, the received error envelope and the `rpc_describe`
//! response. It carries no application model, so a command-line client can stand on it
//! without the profile, runtime or core graph that `nyanpasu-rpc-protocol` consumes.

mod describe;
pub mod error;
pub mod frame;

#[cfg(feature = "local-client")]
pub mod client;

pub use describe::{
    CommandEffect, DescribedCommand, RpcDescribeResponse, RpcTarget, ServerInfo, ServerKind,
};
pub use error::{RpcError, RpcErrorKind};
