//! The existing method/params request envelope decoded by transport adapters.

use serde::Deserialize;
use serde_json::Value;

/// A call names a command and defaults omitted parameters to JSON null.
#[derive(Deserialize)]
pub struct Request {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[cfg(test)]
mod tests {
    use crate::{Request, RpcError, RpcOwner, RpcValue};
    use serde_json::{Value, json};

    #[test]
    fn protocol_envelope_preserves_existing_wire_shape() {
        let request: Request = serde_json::from_value(json!({
            "method": "get_status",
            "params": {"camelCase": [null, true, 7, "value"]},
            "extra": "ignored",
        }))
        .unwrap();
        assert_eq!(request.method, "get_status");
        assert_eq!(
            request.params,
            json!({"camelCase": [null, true, 7, "value"]})
        );
        let request: Request = serde_json::from_value(json!({"method": "get_status"})).unwrap();
        assert_eq!(request.params, Value::Null);

        for (error, kind, message) in [
            (
                RpcError::invalid_params("bad input"),
                "invalid_params",
                "bad input",
            ),
            (
                RpcError::unsupported("open_window"),
                "unsupported",
                "command `open_window` is unavailable over HTTP",
            ),
            (
                RpcError::unknown_method("missing"),
                "unknown_method",
                "unknown command `missing`",
            ),
            (
                RpcError::new("application_error", "failed"),
                "application_error",
                "failed",
            ),
        ] {
            assert_eq!(
                serde_json::to_value(error).unwrap(),
                json!({
                    "kind": kind,
                    "message": message,
                    "code": null,
                    "retryable": null,
                    "operation_id": null,
                    "domain_error": null,
                }),
            );
        }

        let mut error = RpcError::new("application_error", "failed");
        error.code = Some("retry_code".to_owned());
        error.retryable = Some(true);
        error.operation_id = Some("operation-1".to_owned());
        error.domain_error = Some(Box::new(RpcValue(json!({"kind": "domain", "value": [1]}))));
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "kind": "application_error",
                "message": "failed",
                "code": "retry_code",
                "retryable": true,
                "operation_id": "operation-1",
                "domain_error": {"kind": "domain", "value": [1]},
            }),
        );
        let value: RpcValue = serde_json::from_value(json!([null, {"key": "value"}])).unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            json!([null, {"key": "value"}])
        );
        assert_eq!(RpcOwner::desktop("main").label(), "main");
        assert_eq!(
            RpcOwner::new("http:session".to_owned()).label(),
            "http:session"
        );
    }
}
