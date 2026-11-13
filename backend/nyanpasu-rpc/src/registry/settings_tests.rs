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
            "message": "preferences cannot write [\"hotkeys\", \"enable_service_mode\"]",
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

#[tokio::test]
async fn remote_access_is_not_writable_through_the_general_patch() {
    let fixture = Fixture::new(false).await;
    let client = &fixture.rpc.dependencies.client;
    let before = serde_json::to_value(client.get_app_config().await.unwrap()).unwrap();
    let version = client.configuration_status().source_versions.application;

    // Any `Some` value counts: an enabled, a default and a same-value section are all refused,
    // and the allowed field in the mixed patch is not applied either.
    for section in [
        json!({"http": {"enabled": true}}),
        json!({}),
        serde_json::to_value(&client.get_app_config().await.unwrap().remote_access).unwrap(),
    ] {
        let (status, error) = fixture
            .call(
                "patch_app_config",
                json!({"patch": {"remote_access": section, "proxy_layout_column": 9}}),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(error["kind"], "invalid_params");
        assert!(error["domain_error"].is_null());
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains("[\"remote_access\"]")
        );
    }
    assert_eq!(
        serde_json::to_value(client.get_app_config().await.unwrap()).unwrap(),
        before
    );
    assert_eq!(
        client.configuration_status().source_versions.application,
        version
    );

    // An explicit null writes nothing, and a misspelled or unknown key is ignored rather than
    // reaching the section.
    let (status, result) = fixture
        .call(
            "patch_app_config",
            json!({"patch": {
                "remote_access": null, "remoteAccess": {"http": {"enabled": true}},
                "proxy_layout_column": 5
            }}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let after = client.get_app_config().await.unwrap();
    assert!(!after.remote_access.http.enabled);
    assert_eq!(after.proxy_layout_column, 5);

    // The dedicated command is not an HTTP command.
    let (status, error) = fixture
        .call("set_remote_access", json!({"config": {}}))
        .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(error["kind"], "unsupported");
    fixture.shutdown().await;
}

/// Each privileged-field command: (command, scope, a body of its own class, a field of another class).
fn scope_boundary_cases() -> Vec<(&'static str, nyanpasu_rpc_protocol::Scope, Value, Value)> {
    use nyanpasu_rpc_protocol::Scope;
    vec![
        (
            "patch_app_config",
            Scope::Settings,
            json!({"patch": {"proxy_layout_column": 7}}),
            json!({"enable_system_proxy": false}),
        ),
        (
            "patch_system_settings",
            Scope::System,
            json!({"patch": {"proxy_guard_interval": 20}}),
            json!({"proxy_layout_column": 7}),
        ),
        (
            "patch_clash_config",
            Scope::Settings,
            json!({"patch": {"expand_include_all": false}}),
            json!({"enable_tun_mode": false}),
        ),
        (
            "patch_clash_system_config",
            Scope::System,
            json!({"patch": {"web_ui_list": ["https://example.test/ui"]}}),
            json!({"expand_include_all": false}),
        ),
        (
            "patch_runtime_overrides",
            Scope::Settings,
            json!({"patch": {"ipv6": true}}),
            json!({"mode": "global"}),
        ),
        (
            "patch_runtime_access",
            Scope::System,
            json!({"patch": {"secret": "a-new-secret"}}),
            json!({"mode": "global"}),
        ),
    ]
}

/// The catalog scope of a command is the class of the fields its request type can hold.
#[tokio::test]
async fn each_request_command_declares_the_scope_of_its_fields() {
    use nyanpasu_rpc_protocol::{COMMAND_CATALOG, CommandEffect, Scope};
    let scope = |name: &str| {
        let metadata = COMMAND_CATALOG
            .iter()
            .find(|command| command.name == name)
            .unwrap();
        assert_eq!(metadata.effect, CommandEffect::Mutation, "{name}");
        metadata.scope
    };
    for (name, expected, _, _) in scope_boundary_cases() {
        assert_eq!(scope(name), expected, "{name}");
    }
    assert_eq!(scope("set_proxy_mode"), Scope::Proxy);
    // The retry re-applies persisted desired state and writes no field, so it stays `system`:
    // a settings-only caller cannot retry, which fails closed.
    assert_eq!(scope("retry_configuration_effect"), Scope::System);
    assert_eq!(scope("retry_configuration_runtime"), Scope::System);
}

#[tokio::test]
async fn a_field_of_another_class_is_refused_with_nothing_committed() {
    let fixture = Fixture::new(false).await;
    let client = &fixture.rpc.dependencies.client;
    let app_path = fixture.rpc.dependencies.paths.application_config_path();
    let clash_path = fixture.rpc.dependencies.paths.clash_config_path();
    for (name, _, own, other) in scope_boundary_cases() {
        let mut mixed = own["patch"].clone();
        let other_field = other.as_object().unwrap().iter().next().unwrap();
        for (field, value) in [other_field] {
            mixed[field] = value.clone();
        }
        let app_before = serde_json::to_value(client.get_app_config().await.unwrap()).unwrap();
        let clash_before = serde_json::to_value(client.get_clash_config().await.unwrap()).unwrap();
        let versions = client.configuration_status().source_versions;
        let (app_bytes, clash_bytes) = (
            std::fs::read(&app_path).ok(),
            std::fs::read(&clash_path).ok(),
        );
        for transport in [RpcTransport::Http, RpcTransport::TauriIpc] {
            // The foreign field alone, and mixed with a field the command does accept.
            for patch in [other.clone(), mixed.clone()] {
                let error = fixture
                    .rpc
                    .call(
                        transport,
                        name,
                        RpcOwner::desktop("settings"),
                        json!({"patch": patch}),
                    )
                    .await
                    .unwrap_err();
                assert_eq!(error.kind, "invalid_params", "{name} {patch}");
                let field = other.as_object().unwrap().keys().next().unwrap();
                assert!(
                    error.message.contains("cannot write")
                        && error.message.contains(field.as_str()),
                    "{name}: {}",
                    error.message
                );
            }
        }
        assert_eq!(
            serde_json::to_value(client.get_app_config().await.unwrap()).unwrap(),
            app_before,
            "{name}"
        );
        assert_eq!(
            serde_json::to_value(client.get_clash_config().await.unwrap()).unwrap(),
            clash_before,
            "{name}"
        );
        let after = client.configuration_status().source_versions;
        assert_eq!(
            (after.application, after.clash),
            (versions.application, versions.clash)
        );
        assert!(
            client.configuration_status().recent_operations.is_empty(),
            "{name}"
        );
        assert_eq!(std::fs::read(&app_path).ok(), app_bytes, "{name}");
        assert_eq!(std::fs::read(&clash_path).ok(), clash_bytes, "{name}");
    }
    fixture.shutdown().await;
}

/// The whole override record is in no Clash request: replacing it also writes the proxy mode and
/// the ordinary overrides. Override refusals name the keys the wire uses.
#[tokio::test]
async fn the_whole_override_record_and_kebab_case_keys_are_refused_without_a_commit() {
    let fixture = Fixture::new(false).await;
    let client = &fixture.rpc.dependencies.client;
    let record =
        serde_json::to_value(client.get_clash_config().await.unwrap()).unwrap()["overrides"]
            .clone();
    let before = serde_json::to_value(client.get_clash_config().await.unwrap()).unwrap();
    let versions = client.configuration_status().source_versions;
    for name in ["patch_clash_config", "patch_clash_system_config"] {
        for patch in [
            json!({"overrides": record}),
            json!({"overrides": record, "web_ui_list": []}),
        ] {
            let (status, error) = fixture.call(name, json!({"patch": patch})).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {error}");
            assert!(
                error["message"].as_str().unwrap().contains("\"overrides\""),
                "{name}: {error}"
            );
        }
    }
    for (name, patch, key) in [
        (
            "patch_runtime_overrides",
            json!({"allow-lan": false}),
            "allow-lan",
        ),
        (
            "patch_runtime_access",
            json!({"log-level": "info"}),
            "log-level",
        ),
        (
            "patch_runtime_access",
            json!({"ipv6": false, "secret": "x"}),
            "ipv6",
        ),
    ] {
        let (status, error) = fixture.call(name, json!({"patch": patch})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}: {error}");
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains(&format!("{key:?}")),
            "{name}: {error}"
        );
    }
    assert_eq!(
        serde_json::to_value(client.get_clash_config().await.unwrap()).unwrap(),
        before
    );
    assert_eq!(
        client.configuration_status().source_versions.clash,
        versions.clash
    );
    assert!(client.configuration_status().recent_operations.is_empty());
    fixture.shutdown().await;
}

/// A fixture whose runtime baseline is confirmed, as after boot: a Clash config commit waits for
/// the runtime to accept it.
async fn booted_fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let endpoint = nyanpasu_core::client::test_support::TestControlEndpoint::succeeding();
    let args =
        crate::test_support::test_client_args_with_endpoint(&directory, endpoint.clone()).await;
    let fixture =
        Fixture::with_setup(false, crate::test_support::CAPABILITIES, directory, args).await;
    endpoint.prime(&fixture.rpc.dependencies.client).await;
    fixture
}

#[tokio::test]
async fn each_request_command_commits_a_field_of_its_own_class() {
    let fixture = booted_fixture().await;
    for (name, _, own, _) in scope_boundary_cases() {
        let (status, result) = fixture.call(name, own).await;
        assert_eq!(status, StatusCode::OK, "{name}: {result}");
        assert!(
            matches!(
                result["status"].as_str(),
                Some("committed" | "committed_degraded")
            ),
            "{name}: {result}"
        );
    }
    let client = &fixture.rpc.dependencies.client;
    let app = client.get_app_config().await.unwrap();
    assert_eq!((app.proxy_layout_column, app.proxy_guard_interval), (7, 20));
    let clash = client.get_clash_config().await.unwrap();
    assert!(!clash.expand_include_all);
    assert_eq!(clash.web_ui_list, ["https://example.test/ui"]);
    assert!(clash.overrides.secret() == "a-new-secret");
    fixture.shutdown().await;
}

#[tokio::test]
async fn set_proxy_mode_writes_only_the_mode() {
    let fixture = booted_fixture().await;
    let client = &fixture.rpc.dependencies.client;
    let before = serde_json::to_value(client.get_clash_config().await.unwrap()).unwrap();
    let (status, result) = fixture
        .call("set_proxy_mode", json!({"mode": "global"}))
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    let mut after = serde_json::to_value(client.get_clash_config().await.unwrap()).unwrap();
    assert_eq!(after["overrides"]["mode"], "global");
    after["overrides"]["mode"] = before["overrides"]["mode"].clone();
    assert_eq!(after, before, "only the mode changed");
    let (status, error) = fixture
        .call("set_proxy_mode", json!({"mode": "bogus"}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    fixture.shutdown().await;
}
