//! Shared values and ports for consumers and implementations outside the application core.

pub mod bundle_metadata;
pub mod core_version;
pub mod net;
pub mod profiles;

/// An original infrastructure error preserved with its complete source chain.
pub type BoxedCause = Box<dyn std::error::Error + Send + Sync + 'static>;
