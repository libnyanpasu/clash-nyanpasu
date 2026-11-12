//! Shared application RPC registry, audited dispatch and optional HTTP transport.

extern crate self as nyanpasu_rpc;

pub mod audit;
pub mod commands;
pub mod error;
pub mod events;
pub mod forward;
#[cfg(feature = "http")]
pub mod http;
pub mod registry;
#[cfg(all(feature = "http", any(test, feature = "test-support")))]
pub mod test_support;
