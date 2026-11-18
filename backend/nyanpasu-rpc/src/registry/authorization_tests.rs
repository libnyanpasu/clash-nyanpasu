//! Layer 08 shared authorization, discovery (`rpc_describe`), direct-route,
//! and SSE/event policy tests.

use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures::StreamExt as _;
use nyanpasu_contracts::auth::{Access, Grant, Permission, Permissions, Resource, Role};
use nyanpasu_rpc_protocol::{
    AuthContext, CommandEffect, EventName, Principal, RpcDescribeResponse, RpcTarget, ServerInfo,
    ServerKind, ShellCapability,
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

/// A token holding exactly `held`.
fn token_holding(held: &[(Resource, Access)]) -> AuthContext {
    AuthContext::new(
        Principal::Token {
            id: "tok_test_01".to_owned(),
            name: "custom-token".to_owned(),
        },
        Grant::Custom(
            held.iter()
                .fold(Permissions::NONE, |permissions, (resource, access)| {
                    permissions.with(*resource, *access)
                }),
        ),
    )
}

/// A token holding `role`.
fn token_with_role(role: Role) -> AuthContext {
    AuthContext::new(
        Principal::Token {
            id: "tok_test_01".to_owned(),
            name: "role-token".to_owned(),
        },
        Grant::Role(role),
    )
}

/// An HTTP browser session holding every permission, as the owner does.
fn full_session(id: &str) -> AuthContext {
    AuthContext::new(
        Principal::Session { id: id.to_owned() },
        Grant::Role(Role::Owner),
    )
}

#[tokio::test]
async fn permissions_are_independent_across_commands_and_direct_routes() {
    let fixture = Fixture::new(false).await;
    let router = fixture.rpc.clone().router();

    // `proxy:write` cannot read logs (`logs:read`), patch system settings (`system:write`) or
    // app settings (`settings:write`), nor hit direct routes of other resources.
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
    let proxy_only = token_holding(&[(Resource::Proxy, Access::Write)]);
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

    // Reading one resource does not reach another.
    let profile_reader = token_holding(&[(Resource::Profile, Access::Read)]);
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "list_log_files",
        json!({ "source": "app" }),
        None,
        Some(profile_reader.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["kind"], "forbidden");
    assert_eq!(body["message"], "missing permission logs:read");

    // `logs:read` permits `list_log_files` and the sensitive `get_core_log_status`: the marking
    // takes no part in admission.
    let logs_reader = token_holding(&[(Resource::Logs, Access::Read)]);
    for method in ["list_log_files", "get_core_log_status"] {
        let params = if method == "list_log_files" {
            json!({ "source": "app" })
        } else {
            json!({})
        };
        let (status, body) =
            rpc_for_test_call_with_auth(&router, method, params, None, Some(logs_reader.clone()))
                .await;
        assert_eq!(status, StatusCode::OK, "{method}: {body}");
    }

    let response = with_http_auth(router.clone(), profile_reader.clone())
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

    // `/bridge/icons/*` requires `system:read` before cookie/path parsing and keeps security
    // headers + `cache-control: no-store` on both `401` and `403`.
    for expected_auth in [None, Some(profile_reader)] {
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
async fn rpc_describe_is_callable_without_permissions_and_reflects_capabilities() {
    let fixture = Fixture::with_capabilities(false, &[ShellCapability::OsIntegration]).await;
    let router = fixture.rpc.clone().router();

    // No permission still permits `rpc_describe` (`meta:read`), while reporting business
    // commands as `allowed: false`.
    let empty_auth = token_holding(&[]);
    let (status, body) =
        rpc_for_test_call_with_auth(&router, "rpc_describe", json!({}), None, Some(empty_auth))
            .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let described: RpcDescribeResponse = serde_json::from_value(body).unwrap();
    assert_eq!(described.protocol, 2);
    assert_eq!(described.server.kind, ServerKind::Desktop);
    assert_eq!(described.server.version, "0.0.0-test");
    assert_eq!(described.transport, RpcTarget::Http);
    assert_eq!(described.role, None);
    assert_eq!(described.permissions, Permissions::NONE);

    let rpc_describe_entry = described
        .commands
        .iter()
        .find(|cmd| cmd.name == "rpc_describe")
        .unwrap();
    assert_eq!(rpc_describe_entry.effect, CommandEffect::Query);
    assert_eq!(rpc_describe_entry.permission, Permission::META_READ);
    assert!(!rpc_describe_entry.sensitive);
    assert!(rpc_describe_entry.allowed);
    // Every command reports its sensitive marking.
    assert!(
        described
            .commands
            .iter()
            .find(|cmd| cmd.name == "get_profiles")
            .unwrap()
            .sensitive
    );

    let list_files = described
        .commands
        .iter()
        .find(|cmd| cmd.name == "list_log_files")
        .unwrap();
    assert!(!list_files.allowed);

    // The viewer role: `list_log_files` becomes `allowed: true`, while `get_system_accent_color`
    // (`AccentColor` missing) and `patch_system_settings` (`system:write` missing) stay
    // `allowed: false`.
    let read_auth = token_with_role(Role::Viewer);
    let (status, body) =
        rpc_for_test_call_with_auth(&router, "rpc_describe", json!({}), None, Some(read_auth))
            .await;
    assert_eq!(status, StatusCode::OK);
    let described: RpcDescribeResponse = serde_json::from_value(body).unwrap();
    assert_eq!(described.role, Some(Role::Viewer));
    assert_eq!(described.permissions, Role::Viewer.permissions());
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
    assert_eq!(described_headless.role, Some(Role::Owner));
    assert_eq!(described_headless.permissions, Role::Owner.permissions());
    assert_eq!(described_headless.server.kind, ServerKind::Headless);
    assert_eq!(described_headless.server.version, "2.0.0-test");

    full_cap_fixture.shutdown().await;
    fixture.shutdown().await;
}

#[tokio::test]
async fn http_events_enforce_target_capability_permission_filtering_and_revocation() {
    let fixture = Fixture::with_capabilities(false, &[ShellCapability::OsIntegration]).await;
    let router = fixture.rpc.clone().router();
    let full_http = full_session("sess_ev_full");
    // Settings read only: the storage and state events, not logs or system events.
    let settings_reader = token_holding(&[(Resource::Settings, Access::Read)]);

    // Desktop-only events are refused as `501 unsupported` even with every permission.
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

    // A `logs:read` event is rejected as `403 forbidden` for a settings reader before
    // subscribing.
    let mut req = Request::get(format!(
        "/bridge/events?name={}",
        EventName::CoreLogsChanged.as_str()
    ))
    .body(Body::empty())
    .unwrap();
    req.extensions_mut().insert(settings_reader.clone());
    let response = router.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(fixture.rpc.dependencies.events.subscriber_count(), 0);

    // Unnamed `/bridge/events` for a settings reader skips events of other resources,
    // desktop-only and missing-capability events, while forwarding its own and closing on
    // revocation.
    let revocation = CancellationToken::new();
    let mut req = Request::get("/bridge/events").body(Body::empty()).unwrap();
    req.extensions_mut().insert(settings_reader);
    req.extensions_mut()
        .insert(HttpSessionRevocation(revocation.clone(), Arc::new(None)));
    let response = router.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut body = response.into_body().into_data_stream();
    let ready = body.next().await.unwrap().unwrap();
    assert!(std::str::from_utf8(&ready).unwrap().contains("ready"));

    fixture.rpc.dependencies.events.publish(
        EventName::CoreLogsChanged.as_str(),
        json!({ "line": SECRET }),
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
        .insert(token_with_role(Role::Viewer));
    named_req.extensions_mut().insert(HttpSessionRevocation(
        named_revocation.clone(),
        Arc::new(None),
    ));
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
        Grant::Custom(Permissions::NONE),
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
    let authenticator = Arc::new(crate::http::auth::RpcHttpAuthenticator::default());
    let mut args = test_client_args_with_endpoint(
        &directory,
        nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
    )
    .await;
    args.host_adapters.http_server =
        Some(control_factory(None, routes.clone(), authenticator.clone()));
    let paths = args.paths.clone();
    let client = nyanpasu_core::client::NyanpasuClient::try_new(args)
        .await
        .unwrap();
    authenticator
        .install(Arc::new(
            crate::http::auth::NyanpasuClientHttpAuthenticator::new(&client),
        ))
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
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(outcome.degradations().is_empty());

    let launch_url = client.http_launch_url().await.unwrap().unwrap();
    let parsed = url::Url::parse(&launch_url).unwrap();
    let code = parsed
        .query_pairs()
        .find_map(|(k, v)| (k == "code").then(|| v.into_owned()))
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
        .find_map(|part| part.strip_prefix("nyanpasu_session="))
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
            assert!(!id.contains(&code));
            assert!(!id.contains(&session_cookie));
        }
        other => panic!("expected Principal::Session, got {other:?}"),
    }
    // A launch code opens an `admin` session: everything but remote administration.
    assert_eq!(described.role, Some(Role::Admin));
    assert_eq!(described.permissions, Role::Admin.permissions());

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
        assert!(!rendered.contains(&code));
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
    let authenticator = Arc::new(crate::http::auth::NyanpasuClientHttpAuthenticator::new(
        &fixture.rpc.dependencies.client,
    ));
    let loopback = LoopbackListener::bind(
        headless_rpc.router(),
        None,
        nyanpasu_config::application::HttpAccessConfig {
            enabled: true,
            ..Default::default()
        },
        authenticator,
    )
    .await
    .unwrap();
    let launch_url = fixture
        .rpc
        .dependencies
        .client
        .launch_url_for(loopback.origin())
        .await
        .unwrap();
    let parsed = url::Url::parse(&launch_url).unwrap();
    let code = parsed
        .query_pairs()
        .find_map(|(k, v)| (k == "code").then(|| v.into_owned()))
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
        .find_map(|part| part.strip_prefix("nyanpasu_session="))
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
            assert!(!id.contains(&code));
            assert!(!id.contains(&session_cookie));
        }
        other => panic!("expected Principal::Session, got {other:?}"),
    }

    stop.cancel();
    server.await.unwrap().unwrap();
    {
        let records = fixture.sink.records.lock();
        let rendered = format!("{:?}", *records);
        assert!(!rendered.contains(&code));
        assert!(!rendered.contains(&session_cookie));
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn session_anonymous_and_system_only_callers_follow_their_permissions() {
    let fixture = Fixture::new(false).await;
    let router = fixture.rpc.clone().router();

    // 1. A `Session` without permissions may only describe itself; a viewer `Session` reads
    //    every domain, sensitive queries included, and writes nothing.
    let session_empty = AuthContext::new(
        Principal::Session {
            id: "sess_empty_01".to_owned(),
        },
        Grant::Custom(Permissions::NONE),
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
    assert_eq!(described.permissions, Permissions::NONE);
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
        Grant::Role(Role::Viewer),
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
    assert_eq!(described.role, Some(Role::Viewer));
    for (name, allowed) in [
        ("list_log_files", true),
        ("get_core_log_status", true),
        ("patch_app_config", false),
    ] {
        assert_eq!(
            described
                .commands
                .iter()
                .find(|cmd| cmd.name == name)
                .unwrap()
                .allowed,
            allowed,
            "{name}"
        );
    }
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "list_log_files",
        json!({ "source": "app" }),
        None,
        Some(session_read.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "get_core_log_status",
        json!({}),
        None,
        Some(session_read.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "patch_app_config",
        json!({ "patch": { "proxy_layout_column": 4 } }),
        None,
        Some(session_read.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["kind"], "forbidden");

    // 2. The anonymous role holds only capability discovery unless permissions are configured.
    let anon_empty = AuthContext::anonymous(Permissions::NONE);
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
    assert_eq!(described.role, Some(Role::Anonymous));
    assert_eq!(described.permissions, Permissions::NONE);
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

    let anon_logs = AuthContext::anonymous(Permissions::NONE.with(Resource::Logs, Access::Read));
    let (status, body) = rpc_for_test_call_with_auth(
        &router,
        "list_log_files",
        json!({ "source": "app" }),
        None,
        Some(anon_logs.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) =
        rpc_for_test_call_with_auth(&router, "get_proxies", json!({}), None, Some(anon_logs)).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["kind"], "forbidden");

    // 3. A `system:write` token: writing one resource includes reading it, but grants nothing on
    //    another resource.
    let system_only = token_holding(&[(Resource::System, Access::Write)]);
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
    assert_eq!(
        described.permissions,
        Permissions::NONE.with(Resource::System, Access::Write)
    );
    for (name, expected_allowed) in [
        ("rpc_describe", true),
        ("list_log_files", false),
        ("get_core_log_status", false),
        ("patch_app_config", false),
        ("patch_system_settings", true),
        ("get_core_status", true),
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
    for path in ["/bridge/connection-details", "/bridge/logs/archive"] {
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

#[tokio::test]
async fn remote_admin_commands_are_never_http_and_work_on_desktop() {
    use nyanpasu_config::application::{HttpAccessConfig, HttpAccessStrategy, RemoteAccessConfig};
    use nyanpasu_rpc_protocol::RpcOwner;

    use crate::http::{
        HttpServerClient, auth::NyanpasuClientHttpAuthenticator, routes::RpcHttpRoutes,
    };

    let directory = tempfile::tempdir().unwrap();
    let routes = Arc::new(RpcHttpRoutes::default());
    let authenticator = Arc::new(crate::http::auth::RpcHttpAuthenticator::default());
    let mut args = test_client_args_with_endpoint(
        &directory,
        nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
    )
    .await;
    args.host_adapters.http_server =
        Some(control_factory(None, routes.clone(), authenticator.clone()));
    let paths = args.paths.clone();
    let client = nyanpasu_core::client::NyanpasuClient::try_new(args)
        .await
        .unwrap();
    authenticator
        .install(Arc::new(NyanpasuClientHttpAuthenticator::new(&client)))
        .unwrap();
    let sink = Arc::new(AuditSink::default());
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(sink),
            client: client.clone(),
            storage: nyanpasu_core::storage::Storage::try_new(
                &directory.path().join("storage.redb"),
            )
            .unwrap(),
            paths,
            events: EventBus::new(),
        },
        ServerInfo::desktop("2.0.0-admin"),
        CAPABILITIES,
    )
    .unwrap();
    routes.install(&rpc).unwrap();

    let remote_admin_methods = [
        ("list_remote_access_tokens", json!({})),
        (
            "create_remote_access_token",
            json!({
                "request": {
                    "name": "admin-check",
                    "grant": { "role": "viewer" },
                }
            }),
        ),
        (
            "revoke_remote_access_token",
            json!({ "id": "tok_missing_01" }),
        ),
        ("list_remote_browser_sessions", json!({})),
        (
            "revoke_remote_browser_session",
            json!({ "id": "sess_missing_01" }),
        ),
        ("create_http_launch_url", json!({})),
        (
            "set_remote_access",
            json!({
                "config": RemoteAccessConfig::default(),
            }),
        ),
        ("reset_remote_access_tls", json!({})),
    ];

    // 1. Even an HTTP caller holding every permission gets `unsupported` (`501`), and
    //    `rpc_describe` omits the
    //    commands there and reports `allowed: true` on Desktop. The local socket offers the same
    //    commands except the launch URL, a browser credential that stays with the desktop; the
    //    local socket's own behavior is covered by
    //    `the_local_socket_administers_remote_access_and_http_never_does`.
    let full_http = full_session("sess_full_check");
    let http_desc = rpc.describe(RpcTarget::Http, &full_http);
    let local_ctx = AuthContext::local_socket("1000");
    let local_desc = rpc.describe(RpcTarget::Local, &local_ctx);
    let desktop_desc = rpc.describe(RpcTarget::Desktop, &AuthContext::desktop("main"));
    let router = rpc.clone().router();

    for (method, params) in &remote_admin_methods {
        assert!(
            http_desc.commands.iter().all(|c| c.name != *method),
            "HTTP describe must omit desktop-only {method}"
        );
        assert_eq!(
            local_desc.commands.iter().any(|c| c.name == *method),
            *method != "create_http_launch_url",
            "Local describe of {method}"
        );
        assert!(
            desktop_desc
                .commands
                .iter()
                .find(|c| c.name == *method)
                .unwrap()
                .allowed,
            "Desktop describe must allow {method}"
        );

        let (status, body) = rpc_for_test_call_with_auth(
            &router,
            method,
            params.clone(),
            None,
            Some(full_http.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{method}: {body}");
        assert_eq!(body["kind"], "unsupported", "{method}: {body}");

        if *method == "create_http_launch_url" {
            let local_err = rpc
                .call(
                    RpcTransport::Local,
                    method,
                    RpcCaller::new(local_ctx.clone(), RpcOwner::local_connection("test", 1)),
                    params.clone(),
                )
                .await
                .unwrap_err();
            assert_eq!(local_err.kind, "unsupported", "{method}: {local_err:?}");
        }

        let empty_desktop = AuthContext::new(
            Principal::Desktop {
                window: "main".to_owned(),
            },
            Grant::Custom(Permissions::NONE),
        );
        let forbidden_err = rpc
            .call(
                RpcTransport::TauriIpc,
                method,
                RpcCaller::new(empty_desktop, RpcOwner::desktop("main")),
                params.clone(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            forbidden_err.kind, "forbidden",
            "{method}: {forbidden_err:?}"
        );
    }

    // 2. Desktop `RpcCaller::desktop("main")` exercises the full admin lifecycle end-to-end:
    //    enable HTTP -> create launch URL -> login -> list/revoke browser session (closes SSE) ->
    //    create/list token -> token login -> revoke token (cascades to derived session SSE).
    let desktop = || RpcCaller::desktop("main");
    let config = RemoteAccessConfig {
        http: HttpAccessConfig {
            enabled: true,
            strategy: HttpAccessStrategy::LocalAuthenticated,
            ..Default::default()
        },
        ..Default::default()
    };
    rpc.call(
        RpcTransport::TauriIpc,
        "set_remote_access",
        desktop(),
        json!({ "config": config }),
    )
    .await
    .unwrap();

    let launch_url: String = serde_json::from_value(
        rpc.call(
            RpcTransport::TauriIpc,
            "create_http_launch_url",
            desktop(),
            json!({}),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let parsed_launch = url::Url::parse(&launch_url).unwrap();
    let base_url = format!(
        "http://{}:{}",
        parsed_launch.host_str().unwrap(),
        parsed_launch.port().unwrap()
    );

    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    // Launch URL exchanges for a session cookie and opens an SSE stream.
    let exchange = http.get(&launch_url).send().await.unwrap();
    assert_eq!(exchange.status(), reqwest::StatusCode::SEE_OTHER);
    let cookie_pair = exchange
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let mut sse_launch = http
        .get(format!("{base_url}/bridge/events"))
        .header("cookie", &cookie_pair)
        .send()
        .await
        .unwrap()
        .bytes_stream();
    assert!(sse_launch.next().await.unwrap().is_ok());

    let sessions: Vec<nyanpasu_contracts::auth::BrowserSessionSummary> = serde_json::from_value(
        rpc.call(
            RpcTransport::TauriIpc,
            "list_remote_browser_sessions",
            desktop(),
            json!({}),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(sessions.len(), 1);
    let launch_sess_id = sessions[0].id.clone();

    rpc.call(
        RpcTransport::TauriIpc,
        "revoke_remote_browser_session",
        desktop(),
        json!({ "id": launch_sess_id }),
    )
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), sse_launch.next())
            .await
            .unwrap()
            .is_none(),
        "revoking browser session via desktop RPC must close its active SSE stream"
    );

    // Create a named token via desktop RPC; list omits the secret.
    let created = rpc
        .call(
            RpcTransport::TauriIpc,
            "create_remote_access_token",
            desktop(),
            json!({
                "request": {
                    "name": "cli-helper",
                    "grant": { "permissions": { "proxy": "write", "logs": "read" } },
                }
            }),
        )
        .await
        .unwrap();
    let created_token = created["token"].as_str().unwrap().to_owned();
    let created_summary: nyanpasu_contracts::auth::AccessTokenSummary =
        serde_json::from_value(created["summary"].clone()).unwrap();
    let listed: Vec<nyanpasu_contracts::auth::AccessTokenSummary> = serde_json::from_value(
        rpc.call(
            RpcTransport::TauriIpc,
            "list_remote_access_tokens",
            desktop(),
            json!({}),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created_summary.id);
    assert!(!format!("{listed:?}").contains(&created_token));

    // Exchange token via POST /bridge/login, open SSE with the derived session cookie,
    // and verify revoking the token cancels the derived session's SSE stream.
    let login_res = http
        .post(format!("{base_url}/bridge/login"))
        .json(&json!({ "token": created_token }))
        .send()
        .await
        .unwrap();
    assert_eq!(login_res.status(), reqwest::StatusCode::OK);
    let derived_cookie = login_res
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let mut sse_token = http
        .get(format!("{base_url}/bridge/events"))
        .header("cookie", &derived_cookie)
        .send()
        .await
        .unwrap()
        .bytes_stream();
    assert!(sse_token.next().await.unwrap().is_ok());

    rpc.call(
        RpcTransport::TauriIpc,
        "revoke_remote_access_token",
        desktop(),
        json!({ "id": created_summary.id }),
    )
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), sse_token.next())
            .await
            .unwrap()
            .is_none(),
        "revoking a token via desktop RPC must cascade and close derived session SSE streams"
    );

    let after_revoke: Vec<nyanpasu_contracts::auth::AccessTokenSummary> = serde_json::from_value(
        rpc.call(
            RpcTransport::TauriIpc,
            "list_remote_access_tokens",
            desktop(),
            json!({}),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert!(after_revoke.is_empty());

    // Keep reference to HttpServerClient type checked in test module.
    let _typecheck: Option<HttpServerClient> = None;

    client.request_shutdown();
    client.wait_shutdown().await;
}

#[tokio::test]
async fn http_owner_isolation_supports_sse_client_id_query_and_rejects_invalid_nonce() {
    let fixture = Fixture::new(false).await;
    let router = fixture.rpc.clone().router();
    let token_auth = token_with_role(Role::Viewer);

    // Same token with two distinct `?client_id=` SSE query nonces produces distinct owner hashes.
    let mut req_alpha_query = Request::get("/bridge/events?client_id=tab-alpha")
        .body(Body::empty())
        .unwrap();
    req_alpha_query.extensions_mut().insert(token_auth.clone());
    let resp_alpha = router.clone().oneshot(req_alpha_query).await.unwrap();
    assert_eq!(resp_alpha.status(), StatusCode::OK);

    let mut req_alpha_header = Request::get("/bridge/events")
        .header("x-nyanpasu-client-id", "tab-alpha")
        .body(Body::empty())
        .unwrap();
    req_alpha_header.extensions_mut().insert(token_auth.clone());
    let resp_alpha_hdr = router.clone().oneshot(req_alpha_header).await.unwrap();
    assert_eq!(resp_alpha_hdr.status(), StatusCode::OK);

    let mut req_beta_query = Request::get("/bridge/events?client_id=tab-beta")
        .body(Body::empty())
        .unwrap();
    req_beta_query.extensions_mut().insert(token_auth.clone());
    let resp_beta = router.clone().oneshot(req_beta_query).await.unwrap();
    assert_eq!(resp_beta.status(), StatusCode::OK);

    // `/bridge/connection-details` also accepts `?client_id=` fallback and `X-Nyanpasu-Client-Id`.
    let mut conn_alpha = Request::get("/bridge/connection-details?client_id=tab-alpha")
        .body(Body::empty())
        .unwrap();
    conn_alpha.extensions_mut().insert(token_auth.clone());
    assert_eq!(
        router.clone().oneshot(conn_alpha).await.unwrap().status(),
        StatusCode::OK
    );

    let mut conn_beta = Request::get("/bridge/connection-details?client_id=tab-beta")
        .body(Body::empty())
        .unwrap();
    conn_beta.extensions_mut().insert(token_auth.clone());
    assert_eq!(
        router.clone().oneshot(conn_beta).await.unwrap().status(),
        StatusCode::OK
    );

    // Malformed nonce (e.g. containing ':') returns `400 invalid_params`.
    let mut bad_nonce = Request::get("/bridge/events?client_id=bad:id")
        .body(Body::empty())
        .unwrap();
    bad_nonce.extensions_mut().insert(token_auth);
    let bad_resp = router.oneshot(bad_nonce).await.unwrap();
    assert_eq!(bad_resp.status(), StatusCode::BAD_REQUEST);
    let bad_body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(bad_resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(bad_body["kind"], "invalid_params");

    {
        let records = fixture.sink.records.lock();
        let event_owners: Vec<&str> = records
            .iter()
            .filter(|r| {
                r.header.method == "events_subscribe"
                    && matches!(
                        r.phase,
                        AuditPhase::Subscription {
                            outcome: SubscriptionOutcome::Established
                        }
                    )
            })
            .map(|r| r.header.owner.as_str())
            .collect();
        assert_eq!(event_owners.len(), 3, "{event_owners:?}");
        assert_eq!(
            event_owners[0], event_owners[1],
            "X-Nyanpasu-Client-Id and ?client_id= with same nonce must yield identical owners"
        );
        assert_ne!(
            event_owners[0], event_owners[2],
            "distinct client_id nonces under the same token must isolate owners"
        );

        let conn_owners: Vec<&str> = records
            .iter()
            .filter(|r| {
                r.header.method == "connection_details_subscribe"
                    && matches!(
                        r.phase,
                        AuditPhase::Subscription {
                            outcome: SubscriptionOutcome::Established
                        }
                    )
            })
            .map(|r| r.header.owner.as_str())
            .collect();
        assert_eq!(conn_owners.len(), 2, "{conn_owners:?}");
        assert_eq!(conn_owners[0], event_owners[0]);
        assert_eq!(conn_owners[1], event_owners[2]);
    }

    fixture.shutdown().await;
}

#[tokio::test]
async fn set_remote_access_rejects_anonymous_permissions_beyond_the_operator_role() {
    use nyanpasu_config::application::{HttpAccessConfig, HttpAccessStrategy, RemoteAccessConfig};
    use nyanpasu_core::client::SetRemoteAccessError;

    let fixture = Fixture::new(false).await;
    let client = fixture.rpc.dependencies.client.clone();
    let initial = client.remote_access_config();
    let local_open = |anonymous_permissions| RemoteAccessConfig {
        http: HttpAccessConfig {
            enabled: true,
            strategy: HttpAccessStrategy::LocalOpen,
            anonymous_permissions,
            ..Default::default()
        },
        ..Default::default()
    };

    for (resource, access) in [
        (Resource::System, Access::Write),
        (Resource::Logs, Access::Write),
        (Resource::Remote, Access::Read),
    ] {
        let err = client
            .set_remote_access(local_open(
                Permissions::NONE
                    .with(Resource::Proxy, Access::Read)
                    .with(resource, access),
            ))
            .await
            .unwrap_err();
        assert!(
            matches!(err, SetRemoteAccessError::Client { .. }),
            "{err:?}"
        );
        assert!(
            format!("{err:?}").contains("AnonymousBeyondOperator"),
            "{err:?}"
        );
        assert_eq!(client.remote_access_config(), initial);
    }

    // Also verify rejected via UnifiedRpc without committing.
    let rpc_err = fixture
        .rpc
        .call(
            RpcTransport::TauriIpc,
            "set_remote_access",
            RpcCaller::desktop("main"),
            json!({
                "config": local_open(Permissions::NONE.with(Resource::System, Access::Write))
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(rpc_err.kind, "application_error", "{rpc_err:?}");
    assert_eq!(client.remote_access_config(), initial);

    fixture.shutdown().await;
}

#[tokio::test]
async fn the_local_socket_administers_remote_access_and_http_never_does() {
    use nyanpasu_config::application::{HttpAccessConfig, RemoteAccessConfig};
    use nyanpasu_rpc_protocol::RpcOwner;

    let fixture = Fixture::new(false).await;
    let local = |grant: Grant| {
        RpcCaller::new(
            AuthContext::new(
                Principal::LocalSocket {
                    user: "1000".to_owned(),
                },
                grant,
            ),
            RpcOwner::local_connection("test", 1),
        )
    };
    let call = |caller: RpcCaller, method: &'static str, params: serde_json::Value| {
        fixture
            .rpc
            .call(RpcTransport::Local, method, caller, params)
    };

    // The same-user principal is the owner, `remote:write` included.
    let owner = || local(Grant::Role(Role::Owner));
    let listed: Vec<nyanpasu_contracts::auth::AccessTokenSummary> = serde_json::from_value(
        call(owner(), "list_remote_access_tokens", json!({}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(listed.is_empty());

    let created = call(
        owner(),
        "create_remote_access_token",
        json!({"request": {"name": "cli-made", "grant": {"role": "viewer"}}}),
    )
    .await
    .unwrap();
    let secret = created["token"].as_str().unwrap().to_owned();
    let summary: nyanpasu_contracts::auth::AccessTokenSummary =
        serde_json::from_value(created["summary"].clone()).unwrap();
    let listed: Vec<nyanpasu_contracts::auth::AccessTokenSummary> = serde_json::from_value(
        call(owner(), "list_remote_access_tokens", json!({}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(listed.len(), 1);
    assert!(!format!("{listed:?}").contains(&secret));

    // No bearer token may hold `remote` or the owner role.
    for grant in [
        json!({"permissions": {"remote": "read"}}),
        json!({"role": "owner"}),
    ] {
        let refused = call(
            owner(),
            "create_remote_access_token",
            json!({"request": {"name": "wide", "grant": grant}}),
        )
        .await
        .unwrap_err();
        assert_eq!(refused.kind, "application_error", "{refused:?}");
    }

    let sessions: Vec<nyanpasu_contracts::auth::BrowserSessionSummary> = serde_json::from_value(
        call(owner(), "list_remote_browser_sessions", json!({}))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(sessions.is_empty());

    // A change the HTTP listener cannot follow is committed with a degradation, as on desktop.
    let degraded = call(
        owner(),
        "set_remote_access",
        json!({"config": RemoteAccessConfig {
            http: HttpAccessConfig { enabled: true, ..Default::default() },
            ..Default::default()
        }}),
    )
    .await
    .unwrap();
    assert_eq!(degraded["status"], "committed_degraded", "{degraded}");
    let status = call(owner(), "get_http_server_status", json!({}))
        .await
        .unwrap();
    assert_eq!(status["enabled"], false, "{status}");
    // This fixture has no local-socket listener, which the status reports as an error.
    let unsupported = call(owner(), "get_local_socket_status", json!({}))
        .await
        .unwrap_err();
    assert_eq!(unsupported.kind, "application_error", "{unsupported:?}");

    call(
        owner(),
        "revoke_remote_access_token",
        json!({"id": summary.id}),
    )
    .await
    .unwrap();

    // Without `remote`, the same commands are forbidden, whatever the transport exposes.
    let without_admin = Grant::Role(Role::Admin);
    for (method, params) in [
        ("list_remote_access_tokens", json!({})),
        ("get_local_socket_status", json!({})),
        (
            "set_remote_access",
            json!({"config": RemoteAccessConfig::default()}),
        ),
        ("reset_remote_access_tls", json!({})),
    ] {
        let denied = call(local(without_admin), method, params)
            .await
            .unwrap_err();
        assert_eq!(denied.kind, "forbidden", "{method}: {denied:?}");
    }

    // Over HTTP the commands are not offered at all, even to a caller holding every permission.
    let session = full_session("sess_full_check");
    let router = fixture.rpc.clone().router();
    for method in [
        "list_remote_access_tokens",
        "create_remote_access_token",
        "revoke_remote_access_token",
        "list_remote_browser_sessions",
        "revoke_remote_browser_session",
        "set_remote_access",
        "reset_remote_access_tls",
        "get_local_socket_status",
    ] {
        let (status, body) =
            rpc_for_test_call_with_auth(&router, method, json!({}), None, Some(session.clone()))
                .await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{method}: {body}");
        assert_eq!(body["kind"], "unsupported", "{method}: {body}");
    }

    fixture.shutdown().await;
}
