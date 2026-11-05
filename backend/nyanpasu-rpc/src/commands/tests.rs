//! The WebUI command set and the shared commands' behavior on a shell that
//! provides no capability, as the headless server is.

use nyanpasu_core::storage::WebStorage as _;
use nyanpasu_rpc_protocol::{RpcOwner, RpcTarget};
use serde_json::{Value, json};

use crate::{EventBus, UnifiedRpc, registry::tests::test_rpc};

async fn headless(directory: &tempfile::TempDir) -> UnifiedRpc {
    test_rpc(directory, EventBus::new(Vec::new()), &[]).await
}

async fn http(rpc: &UnifiedRpc, method: &str, params: Value) -> Value {
    rpc.call(
        RpcTarget::Http,
        method,
        RpcOwner::http_session(uuid::Uuid::nil()),
        params,
    )
    .await
    .unwrap_or_else(|error| panic!("{method}: {error}"))
}

/// Exposing a command to every browser session needs the review `rpc(http)`
/// asks for, so this list changes only deliberately.
#[tokio::test]
async fn the_http_allowlist_is_the_reviewed_webui_command_set() {
    let directory = tempfile::tempdir().unwrap();
    let rpc = headless(&directory).await;
    let mut expected = vec![
        // Profiles and the runtime.
        "activate_profile",
        "create_profile",
        "delete_profile",
        "get_profiles",
        "get_runtime_yaml",
        "inspect_runtime",
        "read_profile_file",
        "replace_profile_definition",
        "save_profile_file",
        "set_global_transforms",
        // Core status and logs.
        "get_core_log_status",
        "get_core_status",
        "query_core_logs",
        // Application and Clash configuration.
        "get_app_config",
        "get_clash_config",
        "get_clash_info",
        "get_config_features",
        "get_configuration_status",
        "patch_app_config",
        "patch_clash_config",
        "patch_runtime_overrides",
        "retry_configuration_effect",
        "retry_configuration_runtime",
        // Frontend key-value storage.
        "clear_storage",
        "get_all_storage_items",
        "get_storage_item",
        "remove_storage_item",
        "set_storage_item",
        // The running core: proxies, rules, providers, connections and its binary.
        "change_clash_core",
        "clash_api_delete_connections",
        "clash_api_get_configs",
        "clash_api_get_group_delay",
        "clash_api_get_providers_proxies",
        "clash_api_get_providers_rules",
        "clash_api_get_proxy_delay",
        "clash_api_get_rules",
        "clash_api_get_version",
        "clash_api_healthcheck_proxy_provider",
        "clash_api_update_providers_rules",
        "clear_proxy_fixed",
        "fetch_latest_core_versions",
        "get_core_agent_skills",
        "get_core_version",
        "get_proxies",
        "get_sys_proxy",
        "inspect_updater",
        "mutate_proxies",
        "restart_sidecar",
        "select_proxy",
        "update_proxy_provider",
        // The live Clash streams and the traffic history.
        "clear_clash_ws_history",
        "get_clash_ws_snapshot",
        "get_traffic_summary",
        "query_traffic_active_connection_ids",
        "query_traffic_closed_connections",
        "query_traffic_report",
        "query_traffic_usage",
        "query_traffic_usage_by_keys",
        "set_clash_ws_recording",
        // Logs and the frontend error reporter.
        "clear_core_logs",
        "close_log_session",
        "get_core_log",
        "list_log_files",
        "open_log_session",
        "query_logs",
        "report_frontend_events",
        // Profile import, layout, sync and inspection.
        "create_profile_folder",
        "create_proxy_group_folder",
        "get_profile_sync_logs",
        "get_profile_sync_runs",
        "get_profile_sync_status",
        "import_profile",
        "inspect_applied_runtime",
        "inspect_runtime_node",
        "patch_profile_metadata",
        "patch_remote_profile_options",
        "reorder_profiles_by_list",
        "set_profile_layout",
        "set_profile_valid_fields",
        "set_proxy_group_layout",
        "update_profile",
        // The host.
        "get_cached_icon",
        "is_portable",
        "probe_direct_egress",
        "status_service",
    ];
    expected.sort_unstable();
    let http = rpc
        .command_names(RpcTarget::Http)
        .into_iter()
        .filter(|name| !name.ends_with("_probe"))
        .collect::<Vec<_>>();
    assert_eq!(http, expected);
}

#[tokio::test]
async fn saving_a_setting_commits_and_reports_the_effects_the_host_cannot_apply() {
    let directory = tempfile::tempdir().unwrap();
    let rpc = headless(&directory).await;

    let config = http(&rpc, "get_app_config", Value::Null).await;
    assert!(config.is_object(), "{config}");
    // `patch_app_config` may reach the system proxy, hotkeys and the tray, which a
    // headless shell lacks; it is still served.
    let outcome = http(&rpc, "patch_app_config", json!({"patch": {}})).await;
    assert!(outcome.is_object(), "{outcome}");
}

#[tokio::test]
async fn frontend_storage_is_namespaced_away_from_the_application_entries() {
    let directory = tempfile::tempdir().unwrap();
    let rpc = headless(&directory).await;
    let storage = &rpc.dependencies().storage;
    storage.set_item("internal", &"kept").unwrap();

    let revision = http(
        &rpc,
        "set_storage_item",
        json!({"key": "css", "value": "\"a{}\""}),
    )
    .await;
    assert!(revision.is_number() || revision.is_object(), "{revision}");
    let item = http(&rpc, "get_storage_item", json!({"key": "css"})).await;
    assert_eq!(item["value"], "\"a{}\"");
    // The debug listing shows the stored JSON, which encodes the string again.
    let stored = serde_json::to_string("\"a{}\"").unwrap();
    assert_eq!(
        http(&rpc, "get_all_storage_items", Value::Null).await,
        json!([{"key": "css", "value": stored}])
    );

    http(&rpc, "clear_storage", Value::Null).await;
    let item = http(&rpc, "get_storage_item", json!({"key": "css"})).await;
    assert_eq!(item["value"], Value::Null);
    let kept: nyanpasu_core::storage::StorageItem<String> = storage.get_item("internal").unwrap();
    assert_eq!(kept.value.as_deref(), Some("kept"));
}

#[tokio::test]
async fn host_queries_answer_without_a_desktop() {
    let directory = tempfile::tempdir().unwrap();
    let rpc = headless(&directory).await;

    assert_eq!(http(&rpc, "is_portable", Value::Null).await, json!(false));
    let service = http(&rpc, "status_service", Value::Null).await;
    assert!(service["status"].is_string(), "{service}");
}

/// A command the host cannot fully serve answers with its own failure, not
/// as an unknown or unsupported method.
async fn reaches_the_client(rpc: &UnifiedRpc, method: &str, params: Value) {
    let result = rpc
        .call(
            RpcTarget::Http,
            method,
            RpcOwner::http_session(uuid::Uuid::nil()),
            params,
        )
        .await;
    if let Err(error) = result {
        assert_eq!(
            error.kind,
            nyanpasu_rpc_protocol::RpcErrorKind::ApplicationError,
            "{method}: {error}"
        );
    }
}

#[tokio::test]
async fn core_queries_reach_the_client_while_the_core_is_down() {
    let directory = tempfile::tempdir().unwrap();
    let rpc = headless(&directory).await;

    for (method, params) in [
        ("get_proxies", Value::Null),
        ("mutate_proxies", Value::Null),
        ("clash_api_get_version", Value::Null),
        ("clash_api_get_configs", Value::Null),
        ("clash_api_get_rules", Value::Null),
        ("clash_api_get_providers_rules", Value::Null),
        ("clash_api_get_providers_proxies", Value::Null),
        ("clash_api_delete_connections", json!({"id": null})),
        ("clash_api_get_group_delay", json!({"group": "g"})),
        ("select_proxy", json!({"group": "g", "name": "n"})),
        ("get_core_agent_skills", json!({"coreType": "mihomo"})),
        ("get_clash_ws_snapshot", Value::Null),
        ("get_traffic_summary", Value::Null),
        ("clear_clash_ws_history", json!({"kind": "traffic"})),
    ] {
        reaches_the_client(&rpc, method, params).await;
    }
}

#[tokio::test]
async fn a_log_session_belongs_to_the_browser_session_that_opened_it() {
    let directory = tempfile::tempdir().unwrap();
    let rpc = headless(&directory).await;
    let owner = |seed| RpcOwner::http_session(uuid::Uuid::from_u128(seed));
    let call = |owner: RpcOwner, method: &'static str, params: Value| {
        let rpc = rpc.clone();
        async move { rpc.call(RpcTarget::Http, method, owner, params).await }
    };

    let opened = call(
        owner(1),
        "open_log_session",
        json!({"source": "app", "request": {"request_id": "r", "file": null}}),
    )
    .await
    .unwrap();
    let session = opened["id"].as_str().unwrap();

    let close = json!({"source": "app", "session": session});
    let stranger = call(owner(2), "close_log_session", close.clone())
        .await
        .unwrap_err();
    assert_eq!(
        stranger.kind,
        nyanpasu_rpc_protocol::RpcErrorKind::ApplicationError,
        "{stranger}"
    );
    call(owner(1), "close_log_session", close).await.unwrap();
}

#[tokio::test]
async fn profile_folders_commit_through_the_client_for_a_browser_session() {
    let directory = tempfile::tempdir().unwrap();
    let rpc = headless(&directory).await;

    let folder = http(
        &rpc,
        "create_profile_folder",
        json!({"name": "work", "members": []}),
    )
    .await;
    assert!(folder["value"].is_string(), "{folder}");
    let profiles = http(&rpc, "get_profiles", Value::Null).await;
    assert!(
        profiles.to_string().contains("work"),
        "the folder is listed: {profiles}"
    );
}
