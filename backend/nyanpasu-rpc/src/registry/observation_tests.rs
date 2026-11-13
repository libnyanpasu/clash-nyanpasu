//! Safe status summaries through the production dispatch path, beside the full values.

use axum::http::StatusCode;
use nyanpasu_contracts::logs::{CoreLogStatus, CoreLogStatusSummary};
use nyanpasu_core::client::logs::LogPageSummary;
use nyanpasu_logging::{LogPage, LogSession};
use nyanpasu_observers::logs::{CoreLogPage, CoreLogPageSummary};
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

    let full_core_log_status: CoreLogStatus =
        serde_json::from_value(ok(&fixture, "get_core_log_status").await).unwrap();
    assert_eq!(
        ok(&fixture, "get_core_log_status_summary").await,
        serde_json::to_value(CoreLogStatusSummary::from(&full_core_log_status)).unwrap()
    );

    let full_core_logs: CoreLogPage = serde_json::from_value(
        fixture
            .call(
                "query_core_logs",
                json!({"query":{"direction":"latest","cursor":null,"level":null,"keyword":"","limit":50}}),
            )
            .await
            .1,
    )
    .unwrap();
    assert_eq!(
        fixture
            .call(
                "query_core_logs_summary",
                json!({"query":{"direction":"latest","cursor":null,"level":null,"limit":50}}),
            )
            .await
            .1,
        serde_json::to_value(CoreLogPageSummary::from(&full_core_logs)).unwrap()
    );

    fixture.shutdown().await;
}

/// Safe log summaries project both app and core log pages without exposing raw lines,
/// message payloads, target strings, source names, or storage error text.
#[tokio::test]
async fn log_summary_queries_drop_diagnostic_text_and_secrets() {
    let fixture = Fixture::new(false).await;
    let app = crate::test_support::with_http_auth(
        fixture.rpc.clone().router(),
        crate::test_support::test_http_auth(),
    );
    let cookie = format!("nyanpasu_http_session={}", uuid::Uuid::new_v4());
    let app_log = fixture
        .rpc
        .dependencies
        .paths
        .app_logs_dir()
        .join("clash-nyanpasu.2026-10-01.app.log");
    std::fs::write(
        &app_log,
        concat!(
            "{\"timestamp\":\"1700000000000\",\"level\":\"WARN\",\"target\":\"secret-target-sentinel\",\"fields\":{\"message\":\"secret-message-sentinel\"}}\n",
            "unparsed secret-raw-sentinel line\n"
        ),
    )
    .unwrap();

    let (status, opened) = crate::test_support::rpc_for_test_call(
        &app,
        "open_log_session",
        json!({"source":"app","request":{"request_id":"req-1","file":"clash-nyanpasu.2026-10-01.app.log"}}),
        Some(&cookie),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{opened}");
    let session: LogSession = serde_json::from_value(opened).unwrap();

    let full_query = json!({
        "source": "app",
        "request": {
            "session": session.id,
            "filter": {"levels": [], "target": null, "from_ms": null, "to_ms": null, "text": null},
            "direction": "latest",
            "cursor": null,
            "limit": 50
        }
    });
    let summary_query = json!({
        "source": "app",
        "request": {
            "session": session.id,
            "filter": {"levels": [], "from_ms": null, "to_ms": null},
            "direction": "latest",
            "cursor": null,
            "limit": 50
        }
    });

    // Yield to the background indexing session worker until it commits the two seeded lines.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut full_page: Option<LogPage> = None;
    let mut last_page: Option<LogPage> = None;
    loop {
        let (code, body) = crate::test_support::rpc_for_test_call(
            &app,
            "query_logs",
            full_query.clone(),
            Some(&cookie),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        let parsed: LogPage = serde_json::from_value(body).unwrap();
        if !parsed.building && parsed.rows.len() == 2 {
            full_page = Some(parsed);
            break;
        }
        last_page = Some(parsed);
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::task::yield_now().await;
    }
    let full_page = full_page.unwrap_or_else(|| {
        panic!("app log index did not finish within deadline; last page: {last_page:?}")
    });
    let (code, summary_body) = crate::test_support::rpc_for_test_call(
        &app,
        "query_logs_summary",
        summary_query,
        Some(&cookie),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{summary_body}");
    let summary: LogPageSummary = serde_json::from_value(summary_body.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&summary).unwrap(),
        serde_json::to_value(LogPageSummary::from(&full_page)).unwrap()
    );

    let wire = summary_body.to_string();
    for sentinel in [
        "secret-target-sentinel",
        "secret-message-sentinel",
        "secret-raw-sentinel",
    ] {
        assert!(!wire.contains(sentinel), "{wire}");
    }

    let _ = crate::test_support::rpc_for_test_call(
        &app,
        "close_log_session",
        json!({"source":"app","session":session.id}),
        Some(&cookie),
    )
    .await;

    let (code, rejected_core) = crate::test_support::rpc_for_test_call(
        &app,
        "query_core_logs_summary",
        json!({"query":{"direction":"latest","cursor":null,"level":null,"keyword":"secret","limit":10}}),
        Some(&cookie),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{rejected_core}");
    assert_eq!(rejected_core["kind"], "invalid_params", "{rejected_core}");

    for filter_json in [
        json!({"levels": [], "text": "secret-message-sentinel"}),
        json!({"levels": [], "target": "secret-target-sentinel"}),
    ] {
        let (code, rejected_app) = crate::test_support::rpc_for_test_call(
            &app,
            "query_logs_summary",
            json!({
                "source": "app",
                "request": {
                    "session": session.id,
                    "filter": filter_json,
                    "direction": "latest",
                    "cursor": null,
                    "limit": 10
                }
            }),
            Some(&cookie),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{rejected_app}");
        assert_eq!(rejected_app["kind"], "invalid_params", "{rejected_app}");
    }

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
        "get_core_log_status_summary",
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
