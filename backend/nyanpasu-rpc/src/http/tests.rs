use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use nyanpasu_rpc_protocol::ShellCapability;
use tower::ServiceExt as _;

use super::*;
use crate::EventBus;

async fn test_router(directory: &tempfile::TempDir, events: EventBus) -> Router {
    router(crate::registry::tests::test_rpc(directory, events, &ShellCapability::ALL).await)
}

async fn call(app: &Router, body: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::post("/bridge/rpc")
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn shared_commands_answer_with_bare_json_or_the_error_envelope() {
    let directory = tempfile::tempdir().unwrap();
    let app = test_router(&directory, EventBus::new(Vec::new())).await;

    let (status, profiles) = call(&app, r#"{"method":"get_profiles","params":{}}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert!(profiles.get("items").is_some(), "{profiles}");

    for (body, status, kind) in [
        (
            r#"{"method":"get_profiles","params":[]}"#,
            StatusCode::BAD_REQUEST,
            "invalid_params",
        ),
        (
            r#"{"method":"missing","params":{}}"#,
            StatusCode::NOT_FOUND,
            "unknown_method",
        ),
        (
            r#"{"method":"desktop_only_probe","params":{}}"#,
            StatusCode::NOT_IMPLEMENTED,
            "unsupported",
        ),
        (
            r#"{"method":"get_runtime_yaml","params":{}}"#,
            StatusCode::INTERNAL_SERVER_ERROR,
            "application_error",
        ),
    ] {
        let (actual, error) = call(&app, body).await;
        assert_eq!(actual, status, "{body}: {error}");
        assert_eq!(error["kind"], kind, "{body}: {error}");
    }
    let (_, missing_runtime) = call(&app, r#"{"method":"get_runtime_yaml","params":{}}"#).await;
    assert_eq!(missing_runtime["domain_error"]["kind"]["domain"], "runtime");
}

#[tokio::test]
async fn event_streams_filter_by_name_and_resynchronize_after_lag() {
    let directory = tempfile::tempdir().unwrap();
    let events = EventBus::new(Vec::new());
    let app = test_router(&directory, events.clone()).await;

    let response = app
        .clone()
        .oneshot(Request::get("/bridge/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let mut stream = response.into_body().into_data_stream();
    stream.next().await.unwrap().unwrap();
    for sequence in 0..300 {
        events.publish(EventName::StateChanged, serde_json::json!(sequence));
    }
    let gap = stream.next().await.unwrap().unwrap();
    assert!(std::str::from_utf8(&gap).unwrap().contains("event: resync"));
    let event = stream.next().await.unwrap().unwrap();
    assert!(
        std::str::from_utf8(&event)
            .unwrap()
            .contains(EventName::StateChanged.as_str())
    );

    let response = app
        .clone()
        .oneshot(
            Request::get("/bridge/events?name=unknown-event")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = app
        .oneshot(
            Request::get("/bridge/events?name=clash-ws-event")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let ready = body.next().await.unwrap().unwrap();
    assert!(std::str::from_utf8(&ready).unwrap().contains("ready"));
    events.publish(EventName::StorageResync, serde_json::json!({}));
    events.publish(EventName::ClashWs, serde_json::json!({"sequence": 1}));
    let frame = body.next().await.unwrap().unwrap();
    assert_eq!(
        std::str::from_utf8(&frame).unwrap(),
        "data: {\"sequence\":1}\n\n"
    );
}

#[tokio::test]
async fn connection_details_send_current_and_new_frames_and_release_demand() {
    use nyanpasu_core::clash::ws::ClashConnectionDetails;
    let frame = |sequence| {
        Some(Arc::new(ClashConnectionDetails {
            sequence,
            connections: nyanpasu_core::clash::connection_rates::ConnectionDetails::new(
                Arc::new(clash_api::ConnectionsSnapshot {
                    download_total: 0,
                    upload_total: 0,
                    connections: None,
                    memory: None,
                }),
                Vec::new(),
            ),
        }))
    };
    let (sender, receiver) = tokio::sync::watch::channel(frame(1));
    let response = Sse::new(connection_detail_events(receiver)).into_response();
    let mut body = response.into_body().into_data_stream();
    assert_eq!(sender.receiver_count(), 1);
    let first = body.next().await.unwrap().unwrap();
    assert!(
        std::str::from_utf8(&first)
            .unwrap()
            .contains("\"sequence\":1")
    );
    sender.send(frame(2)).unwrap();
    let next = body.next().await.unwrap().unwrap();
    assert!(
        std::str::from_utf8(&next)
            .unwrap()
            .contains("\"sequence\":2")
    );
    drop(body);
    assert_eq!(sender.receiver_count(), 0);
}
