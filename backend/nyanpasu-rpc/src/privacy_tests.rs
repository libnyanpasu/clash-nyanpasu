//! Dispatched error and transport-parse privacy canary tests across every
//! non-`SensitiveRead` scope and transport boundary.

use std::{io, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use nyanpasu_config::application::{HttpAccessConfig, RemoteAccessConfig};
use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
use nyanpasu_observers::logs::CoreLogError;
use nyanpasu_rpc_protocol::{
    AuthContext, Principal, ResponseVisibility, RpcError, RpcOwner, Scope,
};
use nyanpasu_runtime_values::{
    mutation::{CommitAborted, RuntimeAftermath},
    profiles::ProfilesError,
    runtime_error::{CoreFailure, RestoreFailure, RuntimeError},
};
use serde_json::json;
use tower::ServiceExt as _;

use crate::{
    audit::RpcTransport,
    error::{CoreLogCommandError, IpcError},
    privacy::application_error_for,
    registry::RpcCaller,
    test_support::{Fixture, rpc_for_test_call_with_auth, with_http_auth},
};

fn auth_with_scopes(scopes: impl IntoIterator<Item = Scope>) -> AuthContext {
    AuthContext::new(
        Principal::Token {
            id: "tok_privacy_01".to_owned(),
            name: "scoped-token".to_owned(),
        },
        scopes.into_iter().collect(),
    )
}

fn desktop_caller(scopes: impl IntoIterator<Item = Scope>) -> RpcCaller {
    RpcCaller::new(
        AuthContext::new(
            Principal::Desktop {
                window: "main".to_owned(),
            },
            scopes.into_iter().collect(),
        ),
        RpcOwner::desktop("main"),
    )
}

fn assert_no_canary(wire: &serde_json::Value, secrets: &[&str]) {
    let serialized = serde_json::to_string(wire).unwrap();
    for secret in secrets {
        assert!(
            !serialized.contains(secret),
            "public wire must never leak `{secret}`: {serialized}"
        );
    }
    assert!(
        wire.pointer("/domain_error/detail").is_none(),
        "public wire must omit domain_error.detail: {serialized}"
    );
}

#[tokio::test]
async fn dispatched_command_errors_project_safely_across_all_non_sensitive_scopes() {
    let fixture = Fixture::new(false).await;
    let router = fixture.rpc.clone().router();

    // 1. Scope::Read (`get_profile_sync_status_summary` with secret canary UID)
    let read_secret = "https://user:pass@secret-read-canary.invalid/uid";
    for (scopes, is_full) in [
        (vec![Scope::Read], false),
        (vec![Scope::Read, Scope::SensitiveRead], true),
    ] {
        let (status, body) = rpc_for_test_call_with_auth(
            &router,
            "get_profile_sync_status_summary",
            json!({ "uid": read_secret }),
            None,
            Some(auth_with_scopes(scopes)),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        assert_eq!(body["kind"], "application_error", "{body}");
        if is_full {
            assert_eq!(body["domain_error"]["kind"]["domain"], "profile_sync_read");
            assert_eq!(
                body["domain_error"]["kind"]["error"]["kind"],
                "unknown_profile"
            );
            assert!(body["domain_error"]["detail"].is_string());
        } else {
            assert_no_canary(&body, &[read_secret, "secret-read-canary", "user:pass"]);
            assert_eq!(body["domain_error"]["kind"]["domain"], "public");
            assert_eq!(
                body["domain_error"]["kind"]["error"]["domain"],
                "profile_sync_read"
            );
            assert_eq!(
                body["domain_error"]["kind"]["error"]["error"]["kind"],
                "unknown_profile"
            );
        }
    }

    // 2. Scope::Proxy (`clash_api_get_proxy_delay` with secret URL/proxy name)
    let proxy_secret_url = "https://user:pass@secret-proxy-canary.invalid/delay?token=proxyleak";
    let proxy_secret_name = "secret-node-canary";
    for (scopes, is_full) in [
        (vec![Scope::Proxy], false),
        (vec![Scope::Proxy, Scope::SensitiveRead], true),
    ] {
        let (status, body) = rpc_for_test_call_with_auth(
            &router,
            "clash_api_get_proxy_delay",
            json!({ "name": proxy_secret_name, "url": proxy_secret_url }),
            None,
            Some(auth_with_scopes(scopes)),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        assert_eq!(body["kind"], "application_error", "{body}");
        if is_full {
            assert!(body["domain_error"]["detail"].is_string());
        } else {
            assert_no_canary(
                &body,
                &[
                    proxy_secret_url,
                    "secret-proxy-canary",
                    proxy_secret_name,
                    "proxyleak",
                    "user:pass",
                ],
            );
            assert_eq!(body["domain_error"]["kind"]["domain"], "public");
        }
    }

    // 3. Scope::Profile (`import_profile` with malformed secret URL)
    let profile_secret_url = "not a url https://user:pass@secret-profile-canary.invalid/sub?tok=1";
    for (scopes, is_full) in [
        (vec![Scope::Profile], false),
        (vec![Scope::Profile, Scope::SensitiveRead], true),
    ] {
        let (status, body) = rpc_for_test_call_with_auth(
            &router,
            "import_profile",
            json!({ "url": profile_secret_url }),
            None,
            Some(auth_with_scopes(scopes)),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        if is_full {
            assert_eq!(body["domain_error"]["kind"]["domain"], "profiles");
            assert!(
                serde_json::to_string(&body)
                    .unwrap()
                    .contains("secret-profile-canary")
            );
            assert!(body["domain_error"]["detail"].is_string());
        } else {
            assert_no_canary(
                &body,
                &[
                    profile_secret_url,
                    "secret-profile-canary",
                    "user:pass",
                    "tok=1",
                ],
            );
            assert_eq!(body["domain_error"]["kind"]["domain"], "public");
            assert_eq!(body["domain_error"]["kind"]["error"]["domain"], "profiles");
            assert_eq!(
                body["domain_error"]["kind"]["error"]["error"]["kind"],
                "invalid_subscription_url"
            );
        }
    }

    // 4. Scope::Settings (`audit_echo` unclassified IO failure with canary text)
    let settings_secret = "disk-secret-canary-/home/user/.config/secret";
    for (scopes, is_full) in [
        (vec![Scope::Settings], false),
        (vec![Scope::Settings, Scope::SensitiveRead], true),
    ] {
        let (status, body) = rpc_for_test_call_with_auth(
            &router,
            "audit_echo",
            json!({ "value": settings_secret, "fail": true }),
            None,
            Some(auth_with_scopes(scopes)),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
        if is_full {
            assert_eq!(body["message"], settings_secret);
        } else {
            assert_no_canary(&body, &[settings_secret, "disk-secret-canary"]);
            assert_eq!(body["message"], "internal error");
            assert_eq!(body["domain_error"]["kind"]["domain"], "unknown");
            assert_eq!(body["domain_error"]["message"], "internal error");
        }
    }

    // 5. Scope::System (`set_hotkeys` on Desktop dispatch with invalid canary hotkey string)
    let hotkey_secret = "Ctrl+Alt+secret-hotkey-canary";
    for (scopes, is_full) in [
        (vec![Scope::System], false),
        (vec![Scope::System, Scope::SensitiveRead], true),
    ] {
        let error = fixture
            .rpc
            .call(
                RpcTransport::TauriIpc,
                "set_hotkeys",
                desktop_caller(scopes),
                json!({ "hotkeys": [hotkey_secret] }),
            )
            .await
            .unwrap_err();
        let body = serde_json::to_value(&error).unwrap();
        if is_full {
            assert_eq!(body["domain_error"]["kind"]["domain"], "config");
            assert!(
                serde_json::to_string(&body)
                    .unwrap()
                    .contains("secret-hotkey-canary")
            );
            assert!(body["domain_error"]["detail"].is_string());
        } else {
            assert_no_canary(&body, &[hotkey_secret, "secret-hotkey-canary"]);
            assert_eq!(body["domain_error"]["kind"]["domain"], "public");
            assert_eq!(body["domain_error"]["kind"]["error"]["domain"], "config");
            assert_eq!(
                body["domain_error"]["kind"]["error"]["error"]["kind"],
                "validate_hotkeys"
            );
        }
    }

    // 6. Scope::RemoteAdmin (`set_remote_access` on Desktop dispatch with port 0)
    let mut remote_config = RemoteAccessConfig::default();
    remote_config.http = HttpAccessConfig {
        port: Some(0),
        ..remote_config.http
    };
    for (scopes, is_full) in [
        (vec![Scope::RemoteAdmin], false),
        (vec![Scope::RemoteAdmin, Scope::SensitiveRead], true),
    ] {
        let error = fixture
            .rpc
            .call(
                RpcTransport::TauriIpc,
                "set_remote_access",
                desktop_caller(scopes),
                json!({ "config": remote_config }),
            )
            .await
            .unwrap_err();
        let body = serde_json::to_value(&error).unwrap();
        if is_full {
            assert_eq!(body["domain_error"]["kind"]["domain"], "config");
            assert_eq!(
                body["domain_error"]["kind"]["error"]["kind"],
                "invalid_remote_access"
            );
            assert!(body["domain_error"]["detail"].is_string());
        } else {
            assert_no_canary(&body, &[]);
            assert_eq!(body["domain_error"]["kind"]["domain"], "public");
            assert_eq!(body["domain_error"]["kind"]["error"]["domain"], "config");
            assert_eq!(
                body["domain_error"]["kind"]["error"]["error"]["kind"],
                "invalid_remote_access"
            );
            assert_eq!(
                body["domain_error"]["kind"]["error"]["error"]["reason"],
                "port_zero"
            );
        }
    }

    // 7. Typed `CoreLogCommandError` projection (used by `clear_core_logs` on `Scope::Settings`)
    //    emits public `CoreLogRead` `IpcError` on `Public` visibility and rich `CoreLogError` on `Full`.
    let core_log_secret =
        CoreLogError::Unavailable("sqlite path /home/secret/core-logs.db failed".to_owned());
    let settings_cmd_error: CoreLogCommandError = core_log_secret.clone().into();
    let public_envelope = application_error_for(ResponseVisibility::Public, &settings_cmd_error);
    let public_json = serde_json::to_value(&public_envelope).unwrap();
    assert_no_canary(&public_json, &["/home/secret/core-logs.db", "sqlite path"]);
    assert_eq!(public_json["domain_error"]["kind"]["domain"], "public");
    assert_eq!(
        public_json["domain_error"]["kind"]["error"]["domain"],
        "core_log_read"
    );
    assert_eq!(
        public_json["domain_error"]["kind"]["error"]["error"]["kind"],
        "unavailable"
    );
    let full_envelope = application_error_for(ResponseVisibility::Full, &settings_cmd_error);
    let full_json = serde_json::to_value(&full_envelope).unwrap();
    assert_eq!(full_json["domain_error"]["kind"], "unavailable");
    assert_eq!(
        full_json["domain_error"]["message"],
        "sqlite path /home/secret/core-logs.db failed"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn transport_parse_route_and_admission_ordering_enforce_public_privacy() {
    let fixture = Fixture::new(false).await;
    let router = fixture.rpc.clone().router();
    let read_only = auth_with_scopes([Scope::Read]);
    let full_auth = auth_with_scopes([Scope::Read, Scope::SensitiveRead]);

    // 1. Admission ordering: missing HTTP auth (401) precedes malformed body / unknown method / errors.
    let unauth = router
        .clone()
        .oneshot(
            Request::post("/bridge/rpc")
                .header("content-type", "application/json")
                .body(Body::from("{secret-unauth-body"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

    // 2. Malformed HTTP JSON body hides snippet on Read-only (Public) and preserves on Full.
    for (auth, is_full) in [(&read_only, false), (&full_auth, true)] {
        let response = with_http_auth(router.clone(), auth.clone())
            .oneshot(
                Request::post("/bridge/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from("{secret-malformed-json-canary"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(body["kind"], "invalid_params");
        if is_full {
            assert!(
                body["message"]
                    .as_str()
                    .unwrap()
                    .contains("key must be a string")
                    || body["message"].as_str().unwrap().contains("EOF")
                    || body["message"].as_str().unwrap().len() > 20
            );
        } else {
            assert_no_canary(&body, &["secret-malformed-json-canary"]);
            assert_eq!(body["message"], "invalid parameters");
        }
    }

    // 3. Valid JSON envelope with offending parameter key/value hides value on Public.
    for (auth, is_full) in [(&read_only, false), (&full_auth, true)] {
        let (status, body) = rpc_for_test_call_with_auth(
            &router,
            "list_log_files",
            json!({ "source": { "secret_param_canary": 1 } }),
            None,
            Some(auth.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(body["kind"], "invalid_params");
        if is_full {
            assert!(
                body["message"]
                    .as_str()
                    .unwrap()
                    .contains("secret_param_canary")
            );
        } else {
            assert_no_canary(&body, &["secret_param_canary"]);
            assert_eq!(body["message"], "invalid parameters");
        }
    }

    // 4. Unknown method hides offending name on Public and preserves on Full.
    let secret_method = "secret_method_canary_fn";
    for (auth, is_full) in [(&read_only, false), (&full_auth, true)] {
        let (status, body) = rpc_for_test_call_with_auth(
            &router,
            secret_method,
            json!({}),
            None,
            Some(auth.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body["kind"], "unknown_method");
        if is_full {
            assert!(body["message"].as_str().unwrap().contains(secret_method));
        } else {
            assert_no_canary(&body, &[secret_method]);
        }
    }

    // 5. `/bridge/events` rejects invalid query / unknown event without echoing secret canary on Public.
    let secret_event = "secret_event_canary";
    let unknown_event = with_http_auth(router.clone(), read_only.clone())
        .oneshot(
            Request::get(format!("/bridge/events?name={secret_event}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unknown_event.status(), StatusCode::NOT_FOUND);
    let unknown_event_body: serde_json::Value = serde_json::from_slice(
        &to_bytes(unknown_event.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(unknown_event_body["kind"], "unknown_method");
    assert_no_canary(&unknown_event_body, &[secret_event]);

    let bad_query = with_http_auth(router, read_only)
        .oneshot(
            Request::get("/bridge/events?name=first_secret&name=secret_query_canary")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_query.status(), StatusCode::BAD_REQUEST);
    let bad_query_body: serde_json::Value =
        serde_json::from_slice(&to_bytes(bad_query.into_body(), usize::MAX).await.unwrap())
            .unwrap();
    assert_eq!(bad_query_body["kind"], "invalid_params");
    assert_no_canary(&bad_query_body, &["first_secret", "secret_query_canary"]);

    fixture.shutdown().await;
}

#[test]
fn nested_commit_aborted_projection_scrubs_paths_and_urls_but_keeps_core_and_aftermath() {
    #[derive(Debug)]
    struct WrappedCoreCommit {
        core: CoreError,
        ipc: IpcError,
    }

    impl std::fmt::Display for WrappedCoreCommit {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}: {}", self.core, self.ipc)
        }
    }

    impl std::error::Error for WrappedCoreCommit {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.core)
        }
    }

    let secret_path = "/home/alice/.config/nyanpasu/profiles/secret.yaml";
    let secret_url = "https://alice:hunter2@sub.secret.invalid/clash?tok=999";
    let op_id = OperationId::generate();
    let core_failure = CoreFailure {
        kind: Some(CoreErrorKind::InvalidConfig),
        message: format!("failed loading {secret_path} from {secret_url}"),
        retryable: true,
        operation_id: Some(op_id.to_string()),
    };
    let rollback_failure = RestoreFailure::ReadStatus {
        failure: CoreFailure {
            kind: Some(CoreErrorKind::BackendUnavailable),
            message: format!("cannot reach {secret_url}"),
            retryable: true,
            operation_id: Some("inner-op".to_owned()),
        },
    };
    let commit_err = ProfilesError::Commit {
        uid: Some(serde_json::from_value(json!("secret-uid")).unwrap()),
        path: Some(serde_json::from_value(json!("secret/managed-profile.yaml")).unwrap()),
        source: CommitAborted::RuntimeFailed {
            errors: vec![
                Arc::new(RuntimeError::ApplyRuntime {
                    failure: core_failure,
                }),
                Arc::new(RuntimeError::SourceSettled),
            ],
            runtime: RuntimeAftermath::RollbackFailed {
                detail: Arc::new(RuntimeError::RestoreFailed {
                    failure: rollback_failure,
                }),
            },
            source: Box::new(io::Error::other(format!("raw cause {secret_url}"))),
        },
        cleanup_failures: vec![ProfilesError::ShuttingDown],
    };
    let ipc = IpcError::from(commit_err);

    let public_rpc: RpcError = application_error_for(ResponseVisibility::Public, &ipc);
    let wire = serde_json::to_value(&public_rpc).unwrap();
    assert_no_canary(
        &wire,
        &[
            secret_path,
            secret_url,
            "alice",
            "hunter2",
            "sub.secret.invalid",
            "secret-uid",
        ],
    );
    assert_eq!(wire["domain_error"]["kind"]["domain"], "public");
    assert_eq!(wire["domain_error"]["kind"]["error"]["domain"], "profiles");
    assert_eq!(
        wire["domain_error"]["kind"]["error"]["error"]["kind"],
        "commit"
    );
    assert_eq!(
        wire["domain_error"]["kind"]["error"]["error"]["source"]["kind"],
        "runtime_failed"
    );
    assert_eq!(
        wire["domain_error"]["kind"]["error"]["error"]["source"]["runtime"],
        "rollback_failed"
    );
    assert_eq!(
        wire["domain_error"]["kind"]["error"]["error"]["source"]["runtime_causes"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        wire["domain_error"]["kind"]["error"]["error"]["source"]["aftermath_cause"]["kind"],
        "restore_failed"
    );

    // Rich caller still receives full diagnostics and `detail`.
    let full_rpc = application_error_for(ResponseVisibility::Full, &ipc);
    let full_wire = serde_json::to_value(&full_rpc).unwrap();
    assert!(
        serde_json::to_string(&full_wire)
            .unwrap()
            .contains("hunter2")
    );
    assert!(full_wire["domain_error"]["detail"].is_string());

    // Source chain carrying `CoreError` preserves code, retryable, and operation_id on Public.
    let wrapped = WrappedCoreCommit {
        core: CoreError {
            kind: Some(CoreErrorKind::InvalidConfig),
            message: format!("secret core message {secret_url}"),
            retryable: true,
            operation_id: Some(op_id),
        },
        ipc,
    };
    let core_only_public = application_error_for(ResponseVisibility::Public, &wrapped);
    let core_wire = serde_json::to_value(&core_only_public).unwrap();
    assert_no_canary(&core_wire, &[secret_url, "hunter2"]);
    assert_eq!(core_only_public.code.as_deref(), Some("invalid_config"));
    assert_eq!(core_only_public.retryable, Some(true));
    assert_eq!(core_only_public.operation_id, Some(op_id.to_string()));
    assert_eq!(core_wire["domain_error"]["kind"]["domain"], "public");
    assert_eq!(core_wire["domain_error"]["kind"]["error"]["domain"], "core");

    // Unclassified source without CoreError emits `unknown` + fixed message.
    let plain = io::Error::other(format!("raw io error on {secret_path}"));
    let public_plain = application_error_for(ResponseVisibility::Public, &plain);
    let plain_wire = serde_json::to_value(&public_plain).unwrap();
    assert_no_canary(&plain_wire, &[secret_path]);
    assert_eq!(plain_wire["message"], "internal error");
    assert_eq!(plain_wire["domain_error"]["kind"]["domain"], "unknown");
}
