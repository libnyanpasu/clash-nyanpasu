mod api;
pub mod endpoint;
pub use api::clash_api_backend;

#[cfg(test)]
mod api_tests;

pub use endpoint::{LocalEndpoint, ServiceEndpoint};
