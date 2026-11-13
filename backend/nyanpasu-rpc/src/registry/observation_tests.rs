//! Safe status summaries through the production dispatch path, beside the full values.

use axum::http::StatusCode;
use nyanpasu_runtime_values::{
    configuration::ConfigurationStatus,
    core_status::CoreStatusInfo,
    observation::{ConfigurationStatusSummary, CoreStatusSummary, ServiceStatusSummary},
};
use serde_json::{Value, json};

use crate::test_support::Fixture;

async fn ok(fixture: &Fixture, command: &str) -> Value {
    let (status, body) = fixture.call(command, json!({})).await;
    assert_eq!(status, StatusCode::OK, "{command}: {body}");
    body
}

/// Each summary query answers with the projection of the value its full
/// counterpart returns, so a consumer never sees a state the protected view
/// does not hold.
#[tokio::test]
async fn summary_queries_project_the_full_values() {
    let fixture = Fixture::new(false).await;

    let full: ConfigurationStatus =
        serde_json::from_value(ok(&fixture, "get_configuration_status").await).unwrap();
    assert_eq!(
        ok(&fixture, "get_configuration_status_summary").await,
        serde_json::to_value(ConfigurationStatusSummary::from(&full)).unwrap()
    );

    let full: CoreStatusInfo =
        serde_json::from_value(ok(&fixture, "get_core_status").await).unwrap();
    assert_eq!(
        ok(&fixture, "get_core_status_summary").await,
        serde_json::to_value(CoreStatusSummary::from(&full)).unwrap()
    );

    // The full service query is a different shape; the summary is built from the owner's value.
    let summary = ok(&fixture, "get_service_status_summary").await;
    let expected = ServiceStatusSummary::from(&fixture.rpc.dependencies.client.service_status());
    assert_eq!(summary, serde_json::to_value(expected).unwrap());
    assert!(summary["server_reported"].is_boolean());
    assert!(summary.get("server").is_none());

    fixture.shutdown().await;
}

/// The summary answers carry no diagnostic text field at all.
#[tokio::test]
async fn summary_queries_have_no_diagnostic_text_keys() {
    fn keys(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    out.push(key.clone());
                    keys(value, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|item| keys(item, out)),
            _ => (),
        }
    }
    let fixture = Fixture::new(false).await;
    for command in [
        "get_configuration_status_summary",
        "get_core_status_summary",
        "get_service_status_summary",
    ] {
        let mut names = Vec::new();
        keys(&ok(&fixture, command).await, &mut names);
        for forbidden in [
            "message",
            "reason",
            "error",
            "server",
            "controller_url",
            "secret",
        ] {
            assert!(
                !names.iter().any(|name| name == forbidden),
                "{command}: {names:?}"
            );
        }
    }
    fixture.shutdown().await;
}
