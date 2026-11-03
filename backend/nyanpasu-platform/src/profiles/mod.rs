//! Subscription HTTP adapters and profile application integration tests.

mod subscription;
pub mod subscription_http;

#[cfg(test)]
mod integration_tests;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
