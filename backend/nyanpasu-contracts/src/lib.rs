//! Shared values and ports for consumers and implementations outside the application core.

pub mod bundle_metadata;
pub mod core_install;
pub mod core_version;
pub mod elevation;
pub mod service;
pub mod system_dns;
pub mod system_proxy;

pub mod net;
#[cfg(test)]
mod os_port_contract_tests;
pub mod profiles;

/// An original infrastructure error preserved with its complete source chain.
pub type BoxedCause = Box<dyn std::error::Error + Send + Sync + 'static>;
