//! Shared application RPC registry, audited dispatch and optional HTTP transport.

extern crate self as nyanpasu_rpc;

pub use nyanpasu_rpc_macro::rpc;

/// Declares an event, HTTP route or stream for the resource table:
/// `resource!(event "name", targets = [desktop, http], permission = system:read, sensitive,
/// capabilities(app_update))`.
///
/// It expands to nothing. The resource table generator reads the declaration, checks it and
/// writes its row; admission looks the row up by kind and name.
#[macro_export]
macro_rules! resource {
    ($($declaration:tt)*) => {};
}

pub mod audit;
pub mod commands;
pub mod error;
pub mod events;
pub mod forward;
#[cfg(feature = "http")]
pub mod http;
pub mod icons;
#[cfg(feature = "local-server")]
pub mod local;
pub mod permission;
pub mod ports;
pub mod privacy;
pub mod redaction;
pub mod registry;
#[cfg(all(feature = "http", any(test, feature = "test-support")))]
pub mod test_support;

#[cfg(test)]
mod connection_tests;
#[cfg(test)]
mod permission_baseline;
#[cfg(all(test, feature = "http"))]
mod privacy_tests;
#[cfg(test)]
mod receiving_tests;
