//! Shared upgrade fixtures for real-storage integration tests in core.

/// Application configuration written by version 1.6.1.
pub const LEGACY_CONFIG: &str = include_str!("fixtures/v1_6_1/nyanpasu-config.yaml");
/// Profile configuration written by version 1.6.1.
pub const LEGACY_PROFILES: &str = include_str!("fixtures/v1_6_1/profiles.yaml");
/// Expected legacy configuration shape after upgrading to version 2.0.
pub const EXPECTED_CONFIG: &str = include_str!("fixtures/v2_0_expected/nyanpasu-config.yaml");
/// Expected profile configuration shape after upgrading to version 2.0.
pub const EXPECTED_PROFILES: &str = include_str!("fixtures/v2_0_expected/profiles.yaml");
