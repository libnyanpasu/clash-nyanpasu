//! Wire error envelopes and transparent JSON values, without application classification.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// Keep envelope comments ordinary: Specta includes doc attributes in bindings.
#[derive(Debug, Serialize, specta::Type)]
pub struct RpcError {
    pub kind: &'static str,
    pub message: String,
    pub code: Option<String>,
    pub retryable: Option<bool>,
    pub operation_id: Option<String>,
    pub domain_error: Option<Box<RpcValue>>,
}

/// Specta 0.0.12 cannot recursively export `serde_json::Value` directly.
/// Keep the JSON wire transparent while exporting it as TypeScript `any`.
#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct RpcValue(#[specta(type = specta_typescript::Any)] pub Value);

impl RpcError {
    /// Construct an envelope without domain metadata.
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            code: None,
            retryable: None,
            operation_id: None,
            domain_error: None,
        }
    }
    /// Report a parameter decoding failure.
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new("invalid_params", message)
    }

    /// Preserve the existing HTTP admission rejection.
    pub fn unsupported(name: &str) -> Self {
        Self::new(
            "unsupported",
            format!("command `{name}` is unavailable over HTTP"),
        )
    }

    /// Report a name absent from the command registry.
    pub fn unknown_method(name: &str) -> Self {
        Self::new("unknown_method", format!("unknown command `{name}`"))
    }
}
