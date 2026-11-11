//! Dedicated settings command scope through the production dispatch paths.

use axum::http::StatusCode;
use nyanpasu_rpc_protocol::RpcOwner;
use serde_json::{Value, json};

use crate::{audit::RpcTransport, test_support::Fixture};

#[tokio::test]
async fn current_failure_encoding_matches_the_contract_baseline() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../nyanpasu-rpc-protocol/tests/fixtures/contracts/2026-11-12.json"
    ))
    .unwrap();
    let fixture = Fixture::new(false).await;
    for command in nyanpasu_rpc_protocol::COMMAND_CATALOG
        .iter()
        .filter(|command| {
            command
                .targets
                .contains(&nyanpasu_rpc_protocol::RpcTarget::Local)
        })
    {
        let error = fixture
            .rpc
            .call(
                RpcTransport::Local,
                command.name,
                RpcOwner::desktop("fixtures"),
                json!(false),
            )
            .await
            .unwrap_err();
        let mut errors = vec![serde_json::to_value(error).unwrap()];
        if command.name != "get_configuration_status" {
            let error = if matches!(
                command.name,
                "list_log_files" | "open_log_session" | "query_logs" | "close_log_session"
            ) {
                crate::error::application_error(nyanpasu_logging::LogError::Unavailable)
            } else {
                crate::error::application_error(crate::error::IpcError::unknown(
                    std::io::Error::other("contract fixture failure"),
                ))
            };
            errors.push(serde_json::to_value(error).unwrap());
        }
        assert_eq!(
            json!(errors),
            fixtures[command.name]["failures"],
            "{}",
            command.name
        );
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn command_scope_is_identical_across_rpc_transports() {
    use nyanpasu_rpc_protocol::{COMMAND_CATALOG, RpcTarget};

    let fixture = Fixture::new(false).await;
    let local_fixtures: Value = serde_json::from_str(include_str!(
        "../../../nyanpasu-rpc-protocol/tests/fixtures/contracts/2026-11-12.json"
    ))
    .unwrap();
    for metadata in COMMAND_CATALOG
        .iter()
        .filter(|command| command.targets.len() > 1)
    {
        let command = &fixture.rpc.commands[metadata.name];
        assert_eq!(command.metadata, *metadata);
        let mut requests = vec![
            Value::Null,
            json!({}),
            json!(false),
            json!([]),
            json!({"unknown": true}),
        ];
        if let Some(local) = local_fixtures.get(metadata.name) {
            requests.push(local["request"].clone());
        }
        if metadata.name == "patch_app_config" {
            for patch in [
                json!({"proxy_layout_column": 7}),
                json!({"hotkeys": null, "enable_service_mode": null, "proxy_layout_column": 7}),
                json!({"hotkeys": []}),
                json!({"hotkeys": ["toggle_tun_mode,Control+Q"]}),
                json!({"enable_service_mode": false}),
                json!({"enable_service_mode": true}),
                json!({"proxy_layout_column": 7, "hotkeys": [], "enable_service_mode": false}),
            ] {
                requests.push(json!({"patch": patch}));
            }
        }
        for request in requests {
            let outcomes: Vec<_> = metadata
                .targets
                .iter()
                .map(|target| {
                    // Use the production preparation boundary without polling handlers:
                    // decoding every catalog entry must not start its business effects.
                    command
                        .neutral_handler(*target)
                        .and_then(|handler| {
                            handler(
                                fixture.rpc.dependencies.clone(),
                                RpcOwner::desktop("scope"),
                                request.clone(),
                            )
                        })
                        .map(|_| ())
                        .map_err(|error| error.kind)
                })
                .collect();
            assert!(
                outcomes.iter().all(|outcome| outcome == &outcomes[0]),
                "{} {request}: {outcomes:?}",
                metadata.name
            );
            if metadata.name == "patch_app_config" && request.get("patch").is_some() {
                let patch = &request["patch"];
                let restricted =
                    !patch["hotkeys"].is_null() || !patch["enable_service_mode"].is_null();
                assert_eq!(
                    outcomes[0],
                    if restricted {
                        Err("invalid_params")
                    } else {
                        Ok(())
                    }
                );
            }
            if metadata.name == "get_storage_item" && request.get("key").is_some() {
                assert_eq!(
                    metadata.targets,
                    &[RpcTarget::Desktop, RpcTarget::Http, RpcTarget::Local]
                );
                assert!(outcomes.iter().all(Result::is_ok));
            }
        }
    }
    for transport in [
        RpcTransport::TauriIpc,
        RpcTransport::Http,
        RpcTransport::Local,
    ] {
        let value = fixture
            .rpc
            .call(
                transport,
                "get_storage_item",
                RpcOwner::desktop("scope"),
                json!({"key": "scope-key"}),
            )
            .await
            .unwrap();
        assert_eq!(value["value"], Value::Null);
        let error = fixture
            .rpc
            .call(
                transport,
                "get_storage_item",
                RpcOwner::desktop("scope"),
                json!({"key": false}),
            )
            .await
            .unwrap_err();
        assert_eq!(error.kind, "invalid_params");
    }
    let (status, value) = fixture
        .call("get_storage_item", json!({"key": "scope-key"}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(value["value"].is_null());
    let (status, error) = fixture
        .call("get_storage_item", json!({"key": false}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["kind"], "invalid_params");
    let error = fixture
        .rpc
        .call(
            RpcTransport::Local,
            "patch_app_config",
            RpcOwner::desktop("scope"),
            json!({"patch": {"proxy_layout_column": 7}}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, "unsupported");
    fixture.shutdown().await;
}

#[tokio::test]
async fn app_patch_decode_rejects_dedicated_fields_over_http() {
    let fixture = Fixture::new(false).await;
    for patch in [
        json!({"hotkeys": []}),
        json!({"hotkeys": ["toggle_tun_mode,Control+Q"]}),
        json!({"enable_service_mode": false}),
        json!({"enable_service_mode": true}),
    ] {
        let (status, error) = fixture
            .call("patch_app_config", json!({"patch": patch}))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(error["kind"], "invalid_params");
        assert!(error["domain_error"].is_null());
    }
    assert!(
        fixture
            .rpc
            .dependencies
            .client
            .configuration_status()
            .recent_operations
            .is_empty()
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn mixed_app_patch_does_not_commit_any_field() {
    let fixture = Fixture::new(false).await;
    let client = &fixture.rpc.dependencies.client;
    let before = serde_json::to_value(client.get_app_config().await.unwrap()).unwrap();
    let version = client.configuration_status().source_versions.application;
    let path = fixture.rpc.dependencies.paths.application_config_path();
    let persisted = std::fs::read(&path).ok();
    for transport in [RpcTransport::Http, RpcTransport::TauriIpc] {
        let error = fixture.rpc.call(
            transport,
            "patch_app_config",
            RpcOwner::desktop("settings"),
            json!({"patch": {"proxy_layout_column": 97, "hotkeys": [], "enable_service_mode": false}}),
        ).await.unwrap_err();
        assert_eq!(error.kind, "invalid_params");
        assert_eq!(
            serde_json::to_value(client.get_app_config().await.unwrap()).unwrap(),
            before
        );
        assert_eq!(
            client.configuration_status().source_versions.application,
            version
        );
        assert!(client.configuration_status().recent_operations.is_empty());
        assert_eq!(std::fs::read(&path).ok(), persisted);
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn null_dedicated_fields_are_noops() {
    let fixture = Fixture::new(false).await;
    let before = fixture
        .rpc
        .dependencies
        .client
        .get_app_config()
        .await
        .unwrap();
    let (status, result) = fixture
        .call(
            "patch_app_config",
            json!({"patch": {
                "hotkeys": null, "enable_service_mode": null, "proxy_layout_column": 7
            }}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(matches!(
        result["status"].as_str(),
        Some("committed" | "committed_degraded")
    ));
    let after = fixture
        .rpc
        .dependencies
        .client
        .get_app_config()
        .await
        .unwrap();
    assert_eq!(after.hotkeys, before.hotkeys);
    assert_eq!(after.enable_service_mode, before.enable_service_mode);
    assert_eq!(after.proxy_layout_column, 7);
    fixture.shutdown().await;
}

#[tokio::test]
async fn invalid_patch_decode_maps_to_http_400_without_domain_error() {
    let fixture = Fixture::new(false).await;
    let (status, error) = fixture
        .call(
            "patch_app_config",
            json!({"patch": {
                "enable_service_mode": false, "hotkeys": []
            }}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        error,
        json!({
            "kind": "invalid_params",
            "message": "patch_app_config cannot write [\"hotkeys\", \"enable_service_mode\"]; use set_hotkeys for hotkeys and set_service_mode for enable_service_mode",
            "code": null, "retryable": null, "operation_id": null, "domain_error": null
        })
    );
    for method in ["set_hotkeys", "set_service_mode"] {
        let (status, error) = fixture.call(method, json!({})).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(error["kind"], "unsupported");
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn set_service_mode_uses_the_domain_facade() {
    let directory = tempfile::tempdir().unwrap();
    let endpoint = nyanpasu_core::client::test_support::TestControlEndpoint::succeeding();
    let args =
        crate::test_support::test_client_args_with_endpoint(&directory, endpoint.clone()).await;
    let fixture = Fixture::with_setup(false, &[], directory, args).await;
    let client = &fixture.rpc.dependencies.client;
    endpoint.prime(client).await;
    let before = client.get_app_config().await.unwrap();
    let version = client.configuration_status().source_versions.application;
    let result = fixture
        .rpc
        .call(
            RpcTransport::TauriIpc,
            "set_service_mode",
            RpcOwner::desktop("settings"),
            json!({"enabled": false}),
        )
        .await
        .unwrap();
    assert!(
        matches!(
            result["status"].as_str(),
            Some("committed" | "committed_degraded")
        ),
        "{result}"
    );
    assert_eq!(result["commits"][0]["domain"], "application");
    assert_eq!(
        client.configuration_status().source_versions.application,
        version + 1
    );
    let mut expected = serde_json::to_value(before.clone()).unwrap();
    expected["enable_service_mode"] = json!(false);
    assert_eq!(
        serde_json::to_value(client.get_app_config().await.unwrap()).unwrap(),
        expected
    );
    // The fixture has no installed service. Enabling must retain the facade's
    // pre-commit refusal rather than bypassing the runtime transaction.
    let error = fixture
        .rpc
        .call(
            RpcTransport::TauriIpc,
            "set_service_mode",
            RpcOwner::desktop("settings"),
            json!({"enabled": true}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, "application_error");
    assert!(error.domain_error.is_some());
    assert_eq!(
        serde_json::to_value(client.get_app_config().await.unwrap()).unwrap(),
        expected
    );
    assert_eq!(
        client.configuration_status().source_versions.application,
        version + 1
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn hotkeys_effect_retry_is_admitted_over_http() {
    let fixture = Fixture::new(false).await;
    let before = fixture
        .rpc
        .dependencies
        .client
        .get_app_config()
        .await
        .unwrap()
        .hotkeys;
    let (status, result) = fixture
        .call("retry_configuration_effect", json!({"kind": "hotkeys"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result, Value::Null);
    assert_eq!(
        fixture
            .rpc
            .dependencies
            .client
            .get_app_config()
            .await
            .unwrap()
            .hotkeys,
        before
    );
    fixture.shutdown().await;
}
