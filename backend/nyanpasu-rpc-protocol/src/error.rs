//! Wire error envelopes and transparent JSON values, without application classification.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Controls whether an outgoing RPC error or response may include host-private
/// diagnostics such as filesystem paths, URLs, credentials, or raw error chains.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseVisibility {
    /// Privileged caller with `Scope::SensitiveRead`.
    Full,
    /// Caller without `Scope::SensitiveRead`.
    Public,
}

impl ResponseVisibility {
    /// Resolve caller visibility from an authenticated context, failing closed to `Public`.
    pub fn for_auth(auth: Option<&crate::AuthContext>) -> Self {
        match auth {
            Some(auth) if auth.has_sensitive_read() => Self::Full,
            _ => Self::Public,
        }
    }
}

// Keep envelope comments ordinary: Specta includes doc attributes in bindings.
#[derive(Debug, Clone, Serialize, specta::Type)]
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
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
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

    /// Report a caller who failed transport authentication.
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new("unauthorized", message)
    }

    /// Report an authenticated caller lacking the required scope.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new("forbidden", message)
    }

    /// Report a parameter decoding failure respecting caller visibility.
    pub fn invalid_params_for(visibility: ResponseVisibility, message: impl Into<String>) -> Self {
        match visibility {
            ResponseVisibility::Full => Self::invalid_params(message),
            ResponseVisibility::Public => Self::invalid_params("invalid parameters"),
        }
    }

    /// Report a name absent from the command registry.
    pub fn unknown_method(name: &str) -> Self {
        Self::new("unknown_method", format!("unknown command `{name}`"))
    }

    /// Report an unknown command while withholding unvalidated input from non-privileged callers.
    pub fn unknown_method_for(visibility: ResponseVisibility, name: &str) -> Self {
        match visibility {
            ResponseVisibility::Full => Self::unknown_method(name),
            ResponseVisibility::Public => Self::new("unknown_method", "unknown command"),
        }
    }
}
