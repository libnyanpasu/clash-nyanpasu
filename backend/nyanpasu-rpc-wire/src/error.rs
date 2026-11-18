//! The error envelope as a client receives it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An owned error envelope; the domain payload is decoded according to its command.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcError {
    pub kind: String,
    pub message: String,
    pub code: Option<String>,
    pub retryable: Option<bool>,
    pub operation_id: Option<String>,
    pub domain_error: Option<Value>,
}

impl RpcError {
    /// Decode a present domain payload using the receiving type for its operation.
    ///
    /// Absence stays `None`; an incompatible payload returns the serde error.
    pub fn decode_domain_error<T: serde::de::DeserializeOwned>(
        &self,
    ) -> Result<Option<T>, serde_json::Error> {
        self.domain_error
            .clone()
            .map(serde_json::from_value)
            .transpose()
    }

    /// The classification a client may branch on.
    pub fn error_kind(&self) -> RpcErrorKind {
        RpcErrorKind::from_wire(&self.kind)
    }
}

/// The error classes a shell's transport and admission layers emit.
///
/// Spellings are the wire `kind` strings; anything else is [`Self::Other`], so a newer
/// shell's kind never fails to decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcErrorKind {
    /// The request parameters did not decode.
    InvalidParams,
    /// The command is not exposed on this transport.
    Unsupported,
    /// The caller failed transport authentication.
    Unauthorized,
    /// The caller is authenticated but lacks the command's scope.
    Forbidden,
    /// The command name is not registered.
    UnknownMethod,
    /// The command ran and reported a domain failure.
    ApplicationError,
    /// A kind this client does not classify.
    Other,
}

impl RpcErrorKind {
    /// Classifies a wire `kind` string.
    pub fn from_wire(kind: &str) -> Self {
        match kind {
            "invalid_params" => Self::InvalidParams,
            "unsupported" => Self::Unsupported,
            "unauthorized" => Self::Unauthorized,
            "forbidden" => Self::Forbidden,
            "unknown_method" => Self::UnknownMethod,
            "application_error" => Self::ApplicationError,
            _ => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn kinds_use_the_spellings_a_shell_emits() {
        for (wire, kind) in [
            ("invalid_params", RpcErrorKind::InvalidParams),
            ("unsupported", RpcErrorKind::Unsupported),
            ("unauthorized", RpcErrorKind::Unauthorized),
            ("forbidden", RpcErrorKind::Forbidden),
            ("unknown_method", RpcErrorKind::UnknownMethod),
            ("application_error", RpcErrorKind::ApplicationError),
            ("unauthenticated", RpcErrorKind::Other),
            ("something_new", RpcErrorKind::Other),
        ] {
            assert_eq!(RpcErrorKind::from_wire(wire), kind, "{wire}");
        }
    }

    #[test]
    fn an_envelope_decodes_with_only_its_required_fields() {
        let error: RpcError =
            serde_json::from_value(json!({ "kind": "forbidden", "message": "no" })).unwrap();
        assert_eq!(error.error_kind(), RpcErrorKind::Forbidden);
        assert_eq!(error.decode_domain_error::<Value>().unwrap(), None);
    }
}
