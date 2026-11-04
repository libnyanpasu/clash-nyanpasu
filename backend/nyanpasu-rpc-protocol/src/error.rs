use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::RpcTarget;

// The failure envelope every transport returns. Its specta shape is part of
// the generated bindings, so the struct carries no doc comment: one would be
// exported into them.
#[derive(Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct RpcError {
    #[specta(type = String)]
    pub kind: RpcErrorKind,
    pub message: String,
    pub code: Option<String>,
    pub retryable: Option<bool>,
    pub operation_id: Option<String>,
    pub domain_error: Option<Box<RpcValue>>,
}

/// Specta 0.0.12 cannot recursively export `serde_json::Value` directly.
/// Keep the JSON wire transparent while exporting it as TypeScript `any`.
#[derive(Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct RpcValue(#[specta(type = specta_typescript::Any)] pub Value);

/// Why a call failed, as the `kind` string on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcErrorKind {
    /// The parameters did not decode into the command's arguments.
    InvalidParams,
    /// No command has this name.
    UnknownMethod,
    /// The command exists but is not exposed on the caller's transport.
    Unsupported,
    /// The command ran and failed; `domain_error`, `code`, `retryable` and
    /// `operation_id` carry what the domain reported.
    ApplicationError,
    /// A local-socket frame was malformed or spoke another protocol version.
    InvalidRequest,
}

impl RpcErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidParams => "invalid_params",
            Self::UnknownMethod => "unknown_method",
            Self::Unsupported => "unsupported",
            Self::ApplicationError => "application_error",
            Self::InvalidRequest => "invalid_request",
        }
    }
}

impl RpcError {
    /// A failure that carries no domain metadata.
    pub fn new(kind: RpcErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            code: None,
            retryable: None,
            operation_id: None,
            domain_error: None,
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(RpcErrorKind::InvalidParams, message)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(RpcErrorKind::InvalidRequest, message)
    }

    pub fn unknown_method(name: &str) -> Self {
        Self::new(
            RpcErrorKind::UnknownMethod,
            format!("unknown command `{name}`"),
        )
    }

    /// The command exists but did not opt into `target`.
    pub fn unsupported(name: &str, target: RpcTarget) -> Self {
        Self::new(
            RpcErrorKind::Unsupported,
            format!("command `{name}` is unavailable over {target}"),
        )
    }

    /// The HTTP status code the WebUI transport answers this failure with.
    pub fn http_status(&self) -> u16 {
        match self.kind {
            RpcErrorKind::InvalidParams | RpcErrorKind::InvalidRequest => 400,
            RpcErrorKind::UnknownMethod => 404,
            RpcErrorKind::Unsupported => 501,
            RpcErrorKind::ApplicationError => 500,
        }
    }
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for RpcError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_shape_round_trips_with_a_string_kind() {
        let mut error = RpcError::unsupported("open_that", RpcTarget::Http);
        error.domain_error = Some(Box::new(RpcValue(serde_json::json!({"kind": "x"}))));
        let wire = serde_json::to_value(&error).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({
                "kind": "unsupported",
                "message": "command `open_that` is unavailable over HTTP",
                "code": null,
                "retryable": null,
                "operation_id": null,
                "domain_error": {"kind": "x"},
            })
        );
        assert_eq!(serde_json::from_value::<RpcError>(wire).unwrap(), error);
    }

    #[test]
    fn statuses_follow_the_failure_kind() {
        for (error, status) in [
            (RpcError::invalid_params("x"), 400),
            (RpcError::invalid_request("x"), 400),
            (RpcError::unknown_method("x"), 404),
            (RpcError::unsupported("x", RpcTarget::Local), 501),
            (RpcError::new(RpcErrorKind::ApplicationError, "x"), 500),
        ] {
            assert_eq!(error.http_status(), status, "{error}");
        }
    }
}
