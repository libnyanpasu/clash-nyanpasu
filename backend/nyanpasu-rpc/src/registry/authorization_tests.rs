//! Layer 08 shared authorization, discovery (`rpc_describe`), direct-route,
//! and SSE/event policy tests.

use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures::StreamExt as _;
use nyanpasu_rpc_protocol::{
    AuthContext, CommandEffect, EventName, Principal, RpcDescribeResponse, RpcTarget, Scope,
    ScopeSet, ServerInfo, ServerKind, ShellCapability,
};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt as _;
use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::layer::SubscriberExt as _;

use crate::{
    audit::{AuditPhase, RpcAudit, RpcTransport, SubscriptionOutcome},
    events::EventBus,
    http::{HttpSessionRevocation, listener::LoopbackListener, test_support::control_factory},
    ports::HostEnvironment,
    registry::{RpcCaller, RpcDependencies, UnifiedRpc},
    test_support::{
        AuditSink, CAPABILITIES, Capture, Fixture, SECRET, rpc_for_test_call_with_auth,
        test_client_args_with_endpoint, with_http_auth,
    },
};

fn auth_with_scopes(scopes: impl IntoIterator<Item = Scope>) -> AuthContext {
    AuthContext::new(
        Principal::Token {
            id: "tok_test_01".to_owned(),
            name: "scoped-token".to_owned(),
        },
        scopes.into_iter().collect(),
    )
}

#[tokio::test]
async fn scopes_are_independent_across_commands_and_direct_routes() {
    let fixture = Fixture::new(false).await;
    let router = fixture.rpc.clone().router();

    // `Proxy` cannot read logs (`Read`), sensitive logs (`SensitiveRead`),
    // system settings (`System`), or app settings (`Settings`), nor hit direct routes.
    let versions_before = fixture
        .rpc
        .dependencies
        .client
        .configuration_status()
        .source_versions;
    let app_before = fixture
        .rpc
        .dependencies
        .client
        .get_app_config()
        .await
        .unwrap();
    let proxy_only = auth_with_scopes([Scope::Proxy]);
    for (method, params) in [
        ("list_log_files", json!({ "source": "app" })),
        ("get_core_log_status", json!({})),
        (
            "patch_system_settings",
            json!({ "patch": { "proxy_guard_interval": app_before.proxy_guard_interval + 17 } }),
        ),
        (
            "patch_app_config",
            json!({ "patch": { "proxy_layout_column": app_before.proxy_layout_column + 1 } }),
        ),
    ] {
        let (status, body) =
            rpc_for_test_call_with_auth(&router, method, params, None, Some(proxy_only.clone()))
                .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method}: {body}");
        assert_eq!(body["kind"], "forbidden", "{method}: {body}");
    }
    assert_eq!(
        serde_json::to_value(
            fixture
                .rpc
                .dependencies
                .client
                .configuration_status()
                .source_versions
        )
        .unwrap(),
        serde_json::to_value(&versions_before).unwrap()
    );
    assert!(
        fixture
            .rpc
            .dependencies
            .client
            .configuration_status()
            .recent_operations
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(
            fixture
                .rpc
                .dependencies
                .client
                .get_app_config()
                .await
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&app_before).unwrap()
    );

    // Even holding `SensitiveRead` does not imply `Read`.
    let sensitive_only = auth_with_scopes([Scope::SensitiveRead]);
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "list_log_files",
        json!({ "source": "app" }),
        None,
        Some(sensitive_only.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["kind"], "forbidden");

    // Holding `Read` permits `list_log_files` and `/bridge/connection-details`,
    // while still refusing `SensitiveRead` (`/bridge/logs/archive` and `get_core_log_status`).
    let read_only = auth_with_scopes([Scope::Read]);
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "list_log_files",
        json!({ "source": "app" }),
        None,
        Some(read_only.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "get_core_log_status",
        json!({}),
        None,
        Some(read_only.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["kind"], "forbidden");

    let response = with_http_auth(router.clone(), read_only.clone())
        .oneshot(
            Request::get("/bridge/logs/archive")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    fixture.sink.records.lock().clear();
    let response = with_http_auth(router.clone(), proxy_only)
        .oneshot(
            Request::get("/bridge/connection-details")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    {
        let records = fixture.sink.records.lock();
        assert!(records.iter().any(|record| {
            matches!(
                record.phase,
                AuditPhase::Subscription {
                    outcome: SubscriptionOutcome::Failed
                }
            )
        }));
        assert!(!records.iter().any(|record| {
            matches!(
                record.phase,
                AuditPhase::Subscription {
                    outcome: SubscriptionOutcome::Established
                }
            )
        }));
    }

    // `/bridge/icons/*` requires `Scope::Read` before cookie/path parsing and keeps security
    // headers + `cache-control: no-store` on both `401` and `403`.
    for expected_auth in [None, Some(sensitive_only)] {
        let mut request = Request::get("/bridge/icons/tray?mode=normal")
            .body(Body::empty())
            .unwrap();
        let expected_status = match expected_auth {
            None => StatusCode::UNAUTHORIZED,
            Some(auth) => {
                request.extensions_mut().insert(auth);
                StatusCode::FORBIDDEN
            }
        };
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), expected_status);
        assert_eq!(response.headers()["cache-control"], "no-store");
        for (name, value) in crate::icons::SECURITY_HEADERS {
            assert_eq!(response.headers()[name], value);
        }
    }

    fixture.shutdown().await;
}

#[tokio::test]
async fn unauthenticated_http_routes_fail_closed_with_401() {
    let fixture = Fixture::new(false).await;
    let router = fixture.rpc.clone().router();

    for request in [
        Request::post("/bridge/rpc")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "method": "rpc_describe", "params": {} })).unwrap(),
            ))
            .unwrap(),
        Request::post("/bridge/rpc")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "method": "nonexistent_probe_method", "params": {} }))
                    .unwrap(),
            ))
            .unwrap(),
        Request::post("/bridge/rpc")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_vec(&json!({ "method": "quit_application", "params": {} })).unwrap(),
            ))
            .unwrap(),
        Request::post("/bridge/rpc")
            .header("content-type", "application/json")
            .body(Body::from("{"))
            .unwrap(),
        Request::get("/bridge/logs/archive")
            .body(Body::empty())
            .unwrap(),
        Request::get("/bridge/connection-details")
            .body(Body::empty())
            .unwrap(),
        Request::get("/bridge/icons/tray?mode=normal")
            .body(Body::empty())
            .unwrap(),
        Request::get("/bridge/events").body(Body::empty()).unwrap(),
        Request::get(format!(
            "/bridge/events?name={}",
            EventName::StateChanged.as_str()
        ))
        .body(Body::empty())
        .unwrap(),
        Request::get("/bridge/events?name=nonexistent-event")
            .body(Body::empty())
            .unwrap(),
        Request::get(format!(
            "/bridge/events?name={}",
            EventName::WindowMessage.as_str()
        ))
        .body(Body::empty())
        .unwrap(),
    ] {
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    assert_eq!(fixture.rpc.dependencies.events.subscriber_count(), 0);
    fixture.shutdown().await;
}

#[tokio::test]
async fn rpc_describe_is_callable_without_read_and_reflects_capabilities_and_scopes() {
    let fixture = Fixture::with_capabilities(false, &[ShellCapability::OsIntegration]).await;
    let router = fixture.rpc.clone().router();

    // Zero scopes still permits `rpc_describe` (own-only exception), while reporting
    // business commands as `allowed: false`.
    let empty_auth = auth_with_scopes([]);
    let (status, body) =
        rpc_for_test_call_with_auth(&router, "rpc_describe", json!({}), None, Some(empty_auth))
            .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let described: RpcDescribeResponse = serde_json::from_value(body).unwrap();
    assert_eq!(described.protocol, 1);
    assert_eq!(described.server.kind, ServerKind::Desktop);
    assert_eq!(described.server.version, "0.0.0-test");
    assert_eq!(described.transport, RpcTarget::Http);
    assert_eq!(described.scopes, ScopeSet::empty());

    let rpc_describe_entry = described
        .commands
        .iter()
        .find(|cmd| cmd.name == "rpc_describe")
        .unwrap();
    assert_eq!(rpc_describe_entry.effect, CommandEffect::Query);
    assert_eq!(rpc_describe_entry.scope, Scope::Read);
    assert!(rpc_describe_entry.allowed);

    let list_files = described
        .commands
        .iter()
        .find(|cmd| cmd.name == "list_log_files")
        .unwrap();
    assert!(!list_files.allowed);

    // Grant `Read`: `list_log_files` becomes `allowed: true`, while `get_system_accent_color`
    // (`AccentColor` missing) and `patch_system_settings` (`System` missing) stay `allowed: false`.
    let read_auth = auth_with_scopes([Scope::Read]);
    let (status, body) =
        rpc_for_test_call_with_auth(&router, "rpc_describe", json!({}), None, Some(read_auth))
            .await;
    assert_eq!(status, StatusCode::OK);
    let described: RpcDescribeResponse = serde_json::from_value(body).unwrap();
    assert!(
        described
            .commands
            .iter()
            .find(|cmd| cmd.name == "list_log_files")
            .unwrap()
            .allowed
    );
    assert!(
        !described
            .commands
            .iter()
            .find(|cmd| cmd.name == "get_system_accent_color")
            .unwrap()
            .allowed
    );
    assert!(
        !described
            .commands
            .iter()
            .find(|cmd| cmd.name == "patch_system_settings")
            .unwrap()
            .allowed
    );

    // Desktop native linkage is reflected in `UnifiedRpc::describe` after `mark_native_linked`.
    let mut rpc = fixture.rpc.clone();
    let full = AuthContext::desktop("main");
    let before = rpc.describe(RpcTarget::Desktop, &full);
    assert!(
        !before
            .commands
            .iter()
            .find(|cmd| cmd.name == "quit_application")
            .unwrap()
            .allowed
    );
    rpc.mark_native_linked(["quit_application"]);
    // Without `Lifecycle` capability on this fixture it remains `false`; once capability is
    // present it flips to `true`.
    let full_cap_fixture = Fixture::new(false).await;
    let mut full_rpc = full_cap_fixture.rpc.clone();
    assert!(
        !full_rpc
            .describe(RpcTarget::Desktop, &full)
            .commands
            .iter()
            .find(|cmd| cmd.name == "quit_application")
            .unwrap()
            .allowed
    );
    full_rpc.mark_native_linked(["quit_application"]);
    assert!(
        full_rpc
            .describe(RpcTarget::Desktop, &full)
            .commands
            .iter()
            .find(|cmd| cmd.name == "quit_application")
            .unwrap()
            .allowed
    );

    // Calling `rpc_describe` through `UnifiedRpc::call` on Desktop returns Desktop metadata.
    let value = full_rpc
        .call(
            RpcTransport::TauriIpc,
            "rpc_describe",
            RpcCaller::desktop("main"),
            json!({}),
        )
        .await
        .unwrap();
    let desktop_described: RpcDescribeResponse = serde_json::from_value(value).unwrap();
    assert_eq!(desktop_described.transport, RpcTarget::Desktop);
    assert_eq!(
        desktop_described.principal,
        Principal::Desktop {
            window: "main".to_owned(),
        }
    );

    // Headless ServerInfo is reflected cleanly.
    let headless = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: HostEnvironment { is_appimage: false },
            client: fixture.rpc.dependencies.client.clone(),
            storage: fixture.rpc.dependencies.storage.clone(),
            paths: fixture.rpc.dependencies.paths.clone(),
            events: fixture.rpc.dependencies.events.clone(),
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
        },
        ServerInfo::headless("2.0.0-test"),
        CAPABILITIES,
    )
    .unwrap();
    let described_headless =
        headless.describe(RpcTarget::Local, &AuthContext::local_socket("1000"));
    assert_eq!(described_headless.server.kind, ServerKind::Headless);
    assert_eq!(described_headless.server.version, "2.0.0-test");

    full_cap_fixture.shutdown().await;
    fixture.shutdown().await;
}

#[tokio::test]
async fn http_events_enforce_target_capability_scope_filtering_and_revocation() {
    let fixture = Fixture::with_capabilities(false, &[ShellCapability::OsIntegration]).await;
    let router = fixture.rpc.clone().router();
    let full_http = AuthContext::session("sess_ev_full");
    let read_only = auth_with_scopes([Scope::Read]);

    // Desktop-only events are refused as `501 unsupported` even with full scopes.
    for desktop_event in [
        EventName::WindowMessage.as_str(),
        EventName::SchemeRequestReceived.as_str(),
    ] {
        let mut req = Request::get(format!("/bridge/events?name={desktop_event}"))
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(full_http.clone());
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
        assert_eq!(fixture.rpc.dependencies.events.subscriber_count(), 0);
    }

    // Missing `AppUpdate` capability refuses `app-update-state-changed` as `501 unsupported`
    // before subscribing.
    let mut req = Request::get(format!(
        "/bridge/events?name={}",
        EventName::AppUpdateStateChanged.as_str()
    ))
    .body(Body::empty())
    .unwrap();
    req.extensions_mut().insert(full_http);
    let response = router.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    assert_eq!(fixture.rpc.dependencies.events.subscriber_count(), 0);

    // `SensitiveRead` event rejected as `403 forbidden` for `Scope::Read` before subscribing.
    let mut req = Request::get(format!(
        "/bridge/events?name={}",
        EventName::CoreLogsChanged.as_str()
    ))
    .body(Body::empty())
    .unwrap();
    req.extensions_mut().insert(read_only.clone());
    let response = router.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(fixture.rpc.dependencies.events.subscriber_count(), 0);

    // Unnamed `/bridge/events` with `Scope::Read` skips `SensitiveRead`, desktop-only, and
    // missing-capability events, while forwarding `Scope::Read` events and closing on revocation.
    let revocation = CancellationToken::new();
    let mut req = Request::get("/bridge/events").body(Body::empty()).unwrap();
    req.extensions_mut().insert(read_only);
    req.extensions_mut()
        .insert(HttpSessionRevocation(revocation.clone()));
    let response = router.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut body = response.into_body().into_data_stream();
    let ready = body.next().await.unwrap().unwrap();
    assert!(std::str::from_utf8(&ready).unwrap().contains("ready"));

    fixture.rpc.dependencies.events.publish(
        EventName::StorageValueChanged.as_str(),
        json!({ "key": "k", "value": SECRET }),
    );
    fixture.rpc.dependencies.events.publish(
        EventName::WindowMessage.as_str(),
        json!({ "message": SECRET }),
    );
    fixture.rpc.dependencies.events.publish(
        EventName::AppUpdateStateChanged.as_str(),
        json!({ "state": SECRET }),
    );
    fixture
        .rpc
        .dependencies
        .events
        .publish(EventName::StateChanged.as_str(), json!({ "ok": true }));

    let delivered = body.next().await.unwrap().unwrap();
    let text = std::str::from_utf8(&delivered).unwrap();
    assert!(text.contains(EventName::StateChanged.as_str()), "{text}");
    assert!(!text.contains(SECRET), "{text}");

    revocation.cancel();
    assert!(body.next().await.is_none());

    // Named SSE enforces per-frame policy and closes on revocation.
    let named_revocation = CancellationToken::new();
    let mut named_req = Request::get(format!(
        "/bridge/events?name={}",
        EventName::StateChanged.as_str()
    ))
    .body(Body::empty())
    .unwrap();
    named_req
        .extensions_mut()
        .insert(auth_with_scopes([Scope::Read]));
    named_req
        .extensions_mut()
        .insert(HttpSessionRevocation(named_revocation.clone()));
    let named_response = router.clone().oneshot(named_req).await.unwrap();
    assert_eq!(named_response.status(), StatusCode::OK);
    let mut named_body = named_response.into_body().into_data_stream();
    let ready = named_body.next().await.unwrap().unwrap();
    assert!(std::str::from_utf8(&ready).unwrap().contains("ready"));
    fixture
        .rpc
        .dependencies
        .events
        .publish(EventName::StateChanged.as_str(), json!({ "named": true }));
    let delivered = named_body.next().await.unwrap().unwrap();
    assert!(std::str::from_utf8(&delivered).unwrap().contains("named"));
    named_revocation.cancel();
    assert!(named_body.next().await.is_none());

    fixture.shutdown().await;
}

#[tokio::test]
async fn audit_records_public_principal_and_never_logs_secret_token_name_or_http_cookie() {
    let fixture = Fixture::new(true).await;
    let capture = Capture::default();
    let auth = AuthContext::new(
        Principal::Token {
            id: "tok_public_01".to_owned(),
            name: SECRET.to_owned(),
        },
        [].into(),
    );

    async {
        let (status, body) = rpc_for_test_call_with_auth(
            &fixture.rpc.clone().router(),
            "rpc_describe",
            json!({}),
            None,
            Some(auth.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["principal"]["id"], "tok_public_01");
        assert_eq!(body["principal"]["name"], SECRET);

        let (status, _) = rpc_for_test_call_with_auth(
            &fixture.rpc.clone().router(),
            "list_log_files",
            json!({ "source": "app" }),
            None,
            Some(auth),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    .with_subscriber(tracing_subscriber::registry().with(capture.clone()))
    .await;

    {
        let records = fixture.sink.records.lock();
        assert!(!records.is_empty());
        assert!(
            records
                .iter()
                .all(|record| record.header.principal.as_deref() == Some("token:tok_public_01"))
        );
        assert!(!format!("{:?}", *records).contains(SECRET));
    }
    assert!(!format!("{:?}", *capture.0.lock()).contains(SECRET));

    fixture.shutdown().await;
}

#[tokio::test]
async fn real_bootstrap_http_session_assigns_non_secret_session_principal() {
    let directory = tempfile::tempdir().unwrap();
    let routes = Arc::new(crate::http::routes::RpcHttpRoutes::default());
    let mut args = test_client_args_with_endpoint(
        &directory,
        nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
    )
    .await;
    args.host_adapters.http_server = Some(control_factory(None, routes.clone()));
    let paths = args.paths.clone();
    let client = nyanpasu_core::client::NyanpasuClient::try_new(args)
        .await
        .unwrap();
    let sink = Arc::new(AuditSink::default());
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(sink.clone()),
            client: client.clone(),
            storage: nyanpasu_core::storage::Storage::try_new(
                &directory.path().join("web-storage.redb"),
            )
            .unwrap(),
            paths,
            events: EventBus::new(),
        },
        ServerInfo::desktop("2.0.0-real"),
        CAPABILITIES,
    )
    .unwrap();
    routes.install(&rpc).unwrap();

    let outcome = client
        .set_remote_access(nyanpasu_config::application::RemoteAccessConfig {
            http: nyanpasu_config::application::HttpAccessConfig {
                enabled: true,
                ..Default::default()
            },
        })
        .await
        .unwrap();
    assert!(outcome.degradations().is_empty());

    let launch_url = client.http_launch_url().await.unwrap().unwrap();
    let parsed = url::Url::parse(&launch_url).unwrap();
    let token = parsed
        .query_pairs()
        .find_map(|(k, v)| (k == "access_token").then(|| v.into_owned()))
        .unwrap();

    let http_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let bootstrap = http_client.get(&launch_url).send().await.unwrap();
    assert_eq!(bootstrap.status(), reqwest::StatusCode::SEE_OTHER);
    let cookies = bootstrap
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| {
            value
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("; ");
    let session_cookie = cookies
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("nyanpasu_http_session="))
        .unwrap()
        .to_owned();

    let rpc_url = format!(
        "http://{}/bridge/rpc",
        parsed.host_str().unwrap().to_owned() + ":" + &parsed.port().unwrap().to_string()
    );
    let describe_resp = http_client
        .post(&rpc_url)
        .header("cookie", &cookies)
        .json(&json!({ "method": "rpc_describe", "params": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(describe_resp.status(), reqwest::StatusCode::OK);
    let described: RpcDescribeResponse = describe_resp.json().await.unwrap();
    match &described.principal {
        Principal::Session { id } => {
            assert!(id.starts_with("sess_"), "{id}");
            assert!(!id.contains(&token));
            assert!(!id.contains(&session_cookie));
        }
        other => panic!("expected Principal::Session, got {other:?}"),
    }

    {
        let records = sink.records.lock();
        assert!(records.iter().any(|record| {
            record.header.method == "rpc_describe"
                && record
                    .header
                    .principal
                    .as_deref()
                    .is_some_and(|label| label.starts_with("session:sess_"))
        }));
        let rendered = format!("{:?}", *records);
        assert!(!rendered.contains(&token));
        assert!(!rendered.contains(&session_cookie));
    }
    client.request_shutdown();
    client.wait_shutdown().await;
}

#[tokio::test]
async fn headless_loopback_middleware_enforces_401_and_assigns_non_secret_session_principal() {
    let fixture = Fixture::new(false).await;
    let headless_rpc = UnifiedRpc {
        dependencies: Arc::clone(&fixture.rpc.dependencies),
        server: ServerInfo::headless("2.0.0-headless"),
        provided: fixture.rpc.provided.clone(),
        commands: fixture.rpc.commands.clone(),
    };
    let loopback = LoopbackListener::bind(headless_rpc.router(), None)
        .await
        .unwrap();
    let launch_url = loopback.url().to_owned();
    let parsed = url::Url::parse(&launch_url).unwrap();
    let token = parsed
        .query_pairs()
        .find_map(|(k, v)| (k == "access_token").then(|| v.into_owned()))
        .unwrap();
    let rpc_url = format!(
        "http://{}/bridge/rpc",
        parsed.host_str().unwrap().to_owned() + ":" + &parsed.port().unwrap().to_string()
    );
    let stop = CancellationToken::new();
    let server = tokio::spawn(loopback.serve(stop.clone()));

    let http_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // Unauthenticated call through the headless loopback middleware fails closed with 401.
    let unauthenticated = http_client
        .post(&rpc_url)
        .json(&json!({ "method": "rpc_describe", "params": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), reqwest::StatusCode::UNAUTHORIZED);

    let bootstrap = http_client.get(&launch_url).send().await.unwrap();
    assert_eq!(bootstrap.status(), reqwest::StatusCode::SEE_OTHER);
    let cookies = bootstrap
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| {
            value
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("; ");
    let session_cookie = cookies
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("nyanpasu_http_session="))
        .unwrap()
        .to_owned();

    let describe_resp = http_client
        .post(&rpc_url)
        .header("cookie", &cookies)
        .json(&json!({ "method": "rpc_describe", "params": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(describe_resp.status(), reqwest::StatusCode::OK);
    let described: RpcDescribeResponse = describe_resp.json().await.unwrap();
    assert_eq!(described.server.kind, ServerKind::Headless);
    match &described.principal {
        Principal::Session { id } => {
            assert!(id.starts_with("sess_"), "{id}");
            assert!(!id.contains(&token));
            assert!(!id.contains(&session_cookie));
        }
        other => panic!("expected Principal::Session, got {other:?}"),
    }

    stop.cancel();
    server.await.unwrap().unwrap();
    {
        let records = fixture.sink.records.lock();
        let rendered = format!("{:?}", *records);
        assert!(!rendered.contains(&token));
        assert!(!rendered.contains(&session_cookie));
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn scoped_session_test_anonymous_and_system_only_principals_follow_explicit_scopes() {
    let fixture = Fixture::new(false).await;
    let router = fixture.rpc.clone().router();

    // 1. Explicitly scoped `Session`: zero scopes permits own `rpc_describe` only;
    //    `Read`-only `Session` permits `Read` queries and denies `SensitiveRead`/`Settings`.
    let session_empty = AuthContext::new(
        Principal::Session {
            id: "sess_empty_01".to_owned(),
        },
        ScopeSet::empty(),
    );
    fixture.sink.records.lock().clear();
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "rpc_describe",
        json!({}),
        None,
        Some(session_empty.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let described: RpcDescribeResponse = serde_json::from_value(body).unwrap();
    assert_eq!(
        described.principal,
        Principal::Session {
            id: "sess_empty_01".to_owned(),
        }
    );
    assert_eq!(described.scopes, ScopeSet::empty());
    assert!(
        described
            .commands
            .iter()
            .find(|cmd| cmd.name == "rpc_describe")
            .unwrap()
            .allowed
    );
    assert!(
        !described
            .commands
            .iter()
            .find(|cmd| cmd.name == "list_log_files")
            .unwrap()
            .allowed
    );
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "list_log_files",
        json!({ "source": "app" }),
        None,
        Some(session_empty),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["kind"], "forbidden");
    assert!(
        fixture
            .sink
            .records
            .lock()
            .iter()
            .all(|record| record.header.principal.as_deref() == Some("session:sess_empty_01"))
    );

    let session_read = AuthContext::new(
        Principal::Session {
            id: "sess_scoped_01".to_owned(),
        },
        [Scope::Read].into(),
    );
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "rpc_describe",
        json!({}),
        None,
        Some(session_read.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let described: RpcDescribeResponse = serde_json::from_value(body).unwrap();
    assert_eq!(
        described.principal,
        Principal::Session {
            id: "sess_scoped_01".to_owned(),
        }
    );
    assert_eq!(described.scopes, [Scope::Read].into());
    assert!(
        described
            .commands
            .iter()
            .find(|cmd| cmd.name == "list_log_files")
            .unwrap()
            .allowed
    );
    assert!(
        !described
            .commands
            .iter()
            .find(|cmd| cmd.name == "get_core_log_status")
            .unwrap()
            .allowed
    );
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "list_log_files",
        json!({ "source": "app" }),
        None,
        Some(session_read.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for (method, params) in [
        ("get_core_log_status", json!({})),
        (
            "patch_app_config",
            json!({ "patch": { "proxy_layout_column": 4 } }),
        ),
    ] {
        let (status, body) =
            rpc_for_test_call_with_auth(&router, method, params, None, Some(session_read.clone()))
                .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method}: {body}");
        assert_eq!(body["kind"], "forbidden");
    }

    // 2. Test-only resolved `Anonymous` (production anonymous strategy remains unavailable):
    //    follows the own-only `rpc_describe` exception and explicit `ScopeSet` checks.
    let anon_empty = AuthContext::new(Principal::Anonymous, ScopeSet::empty());
    fixture.sink.records.lock().clear();
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "rpc_describe",
        json!({}),
        None,
        Some(anon_empty.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let described: RpcDescribeResponse = serde_json::from_value(body).unwrap();
    assert_eq!(described.principal, Principal::Anonymous);
    assert_eq!(described.scopes, ScopeSet::empty());
    assert!(
        described
            .commands
            .iter()
            .find(|cmd| cmd.name == "rpc_describe")
            .unwrap()
            .allowed
    );
    assert!(
        !described
            .commands
            .iter()
            .find(|cmd| cmd.name == "list_log_files")
            .unwrap()
            .allowed
    );
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "list_log_files",
        json!({ "source": "app" }),
        None,
        Some(anon_empty),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["kind"], "forbidden");
    assert!(
        fixture
            .sink
            .records
            .lock()
            .iter()
            .all(|record| record.header.principal.as_deref() == Some("anonymous"))
    );

    let anon_read = AuthContext::new(Principal::Anonymous, [Scope::Read].into());
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "list_log_files",
        json!({ "source": "app" }),
        None,
        Some(anon_read.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "get_core_log_status",
        json!({}),
        None,
        Some(anon_read),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["kind"], "forbidden");

    // 3. `System`-only `Token`: `Scope::System` does NOT implicitly grant `Read`,
    //    `SensitiveRead`, or `Settings`, while permitting `rpc_describe` and `Scope::System`.
    let system_only = auth_with_scopes([Scope::System]);
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "rpc_describe",
        json!({}),
        None,
        Some(system_only.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let described: RpcDescribeResponse = serde_json::from_value(body).unwrap();
    assert_eq!(described.scopes, [Scope::System].into());
    for (name, expected_allowed) in [
        ("rpc_describe", true),
        ("list_log_files", false),
        ("get_core_log_status", false),
        ("patch_app_config", false),
        ("patch_system_settings", true),
    ] {
        assert_eq!(
            described
                .commands
                .iter()
                .find(|cmd| cmd.name == name)
                .unwrap()
                .allowed,
            expected_allowed,
            "unexpected describe availability for {name}"
        );
    }

    for (method, params) in [
        ("list_log_files", json!({ "source": "app" })),
        ("get_core_log_status", json!({})),
        (
            "patch_app_config",
            json!({ "patch": { "proxy_layout_column": 4 } }),
        ),
    ] {
        let (status, body) =
            rpc_for_test_call_with_auth(&router, method, params, None, Some(system_only.clone()))
                .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method}: {body}");
        assert_eq!(body["kind"], "forbidden");
    }
    for path in [
        "/bridge/connection-details",
        "/bridge/logs/archive",
        "/bridge/icons/tray?mode=normal",
    ] {
        let response = with_http_auth(router.clone(), system_only.clone())
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }

    let app_before = fixture
        .rpc
        .dependencies
        .client
        .get_app_config()
        .await
        .unwrap();
    let updated_interval = app_before.proxy_guard_interval + 11;
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "patch_system_settings",
        json!({ "patch": { "proxy_guard_interval": updated_interval } }),
        None,
        Some(system_only),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let app_after = fixture
        .rpc
        .dependencies
        .client
        .get_app_config()
        .await
        .unwrap();
    assert_eq!(app_after.proxy_guard_interval, updated_interval);

    fixture.shutdown().await;
}
