//! Executable request/result/error inventory, including native and mutually exclusive cfg declarations.
//! Every entry checks its complete Serde receiving closure, including external nested values.
//! Channel arguments are shell bindings, not values accepted by the local subscription protocol.

use crate::{
    EventName,
    wire::{self, receive},
};
use quote::ToTokens as _;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeSet;

fn request<T: Serialize + DeserializeOwned>() {}
fn response<T: DeserializeOwned>() {}

#[test]
fn every_registered_command_has_a_neutral_wire_closure() {
    native_values();
    let mut commands = Vec::new();
    macro_rules! command {
        ($name:literal, ($($request:ty),*), $result:ty, $error:ty) => {
            command!($name, [], ($($request),*), $result, $error);
        };
        ($name:literal, [$($guard:meta),*], ($($request:ty),*), $result:ty, $error:ty) => {{
            $(request::<$request>();)*
            response::<$result>();
            response::<$error>();
            // The transport envelope is present even for scalar and unit commands.
            response::<receive::RpcError>();
            commands.push(($name.to_owned(), vec![$(compact(stringify!($guard))),*]));
        }};
    }
    // R/commands/clash.rs
    command!(
        "clash_api_get_proxy_delay",
        (String, Option<String>, Option<String>, Option<String>),
        clash_api::Delay,
        receive::IpcError
    );
    // R/commands/clash.rs
    command!(
        "clash_api_get_configs",
        (),
        crate::wire::clash::ClashConfig,
        receive::IpcError
    );
    // R/commands/clash.rs
    command!(
        "clash_api_delete_connections",
        (Option<String>),
        (),
        receive::IpcError
    );
    // R/commands/clash.rs
    command!(
        "clash_api_get_version",
        (),
        crate::wire::clash::ClashVersion,
        receive::IpcError
    );
    // R/commands/clash.rs
    command!(
        "clash_api_get_rules",
        (),
        crate::wire::clash::RulesRes,
        receive::IpcError
    );
    // R/commands/clash.rs
    command!(
        "clash_api_get_providers_rules",
        (),
        crate::wire::clash::ProvidersRulesRes,
        receive::IpcError
    );
    // R/commands/clash.rs
    command!(
        "clash_api_update_providers_rules",
        (String),
        (),
        receive::IpcError
    );
    // R/commands/clash.rs
    command!("clash_api_get_group_delay", (String, Option<String>, Option<String>), indexmap::IndexMap<clash_api::ProxyName, u16>, receive::IpcError);
    // R/commands/clash.rs
    command!("clash_api_get_providers_proxies", (), indexmap::IndexMap<clash_api::ProviderName, crate::wire::clash::ProxyProviderSummary>, receive::IpcError);
    // R/commands/clash.rs
    command!(
        "get_proxies",
        (),
        crate::wire::clash::Proxies,
        receive::IpcError
    );
    // R/commands/clash.rs
    command!(
        "mutate_proxies",
        (),
        crate::wire::clash::Proxies,
        receive::IpcError
    );
    // R/commands/clash.rs
    command!(
        "select_proxy",
        (String, String),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/clash.rs
    command!(
        "clear_proxy_fixed",
        (String),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/clash.rs
    command!("update_proxy_provider", (String), (), receive::IpcError);
    // R/commands/clash.rs
    command!(
        "clash_api_healthcheck_proxy_provider",
        (String),
        (),
        receive::IpcError
    );
    // R/commands/host.rs
    command!("get_core_dir", (), String, receive::IpcError);
    // R/commands/host.rs
    command!("is_appimage", (), bool, receive::IpcError);
    // R/commands/host.rs
    command!("get_service_install_prompt", (), String, receive::IpcError);
    // R/commands/host.rs
    command!(
        "get_system_accent_color",
        (),
        Option<String>,
        receive::IpcError
    );
    // R/commands/icons.rs
    command!(
        "get_cached_icon",
        (String),
        crate::wire::host::IconData,
        receive::IpcError
    );
    // R/commands/icons.rs
    command!(
        "get_tray_icon",
        (crate::values::TrayIcon),
        crate::wire::host::IconData,
        receive::IpcError
    );
    // R/commands/icons.rs
    command!(
        "set_tray_icon_from_bytes",
        (crate::values::TrayIcon, String),
        (),
        receive::IpcError
    );
    // R/commands/icons.rs
    command!(
        "is_tray_icon_set",
        (crate::values::TrayIcon),
        bool,
        receive::IpcError
    );
    // R/commands/icons.rs
    command!(
        "get_widget_icon",
        (crate::values::TrayIcon),
        crate::wire::host::IconData,
        receive::IpcError
    );
    // R/commands/icons.rs
    command!(
        "is_widget_icon_set",
        (crate::values::TrayIcon),
        bool,
        receive::IpcError
    );
    // R/commands/icons.rs
    command!(
        "set_widget_icon_from_bytes",
        (crate::values::TrayIcon, String),
        (),
        receive::IpcError
    );
    // R/commands/logs.rs
    command!(
        "list_log_files",
        (crate::wire::logs::LogSource),
        Vec<nyanpasu_logging::LogFileInfo>,
        nyanpasu_logging::LogError
    );
    // R/commands/logs.rs
    command!(
        "open_log_session",
        (crate::wire::logs::LogSource, nyanpasu_logging::OpenLogs),
        nyanpasu_logging::LogSession,
        nyanpasu_logging::LogError
    );
    // R/commands/logs.rs
    command!(
        "query_logs",
        (crate::wire::logs::LogSource, nyanpasu_logging::QueryLogs),
        nyanpasu_logging::LogPage,
        nyanpasu_logging::LogError
    );
    // R/commands/logs.rs
    command!(
        "close_log_session",
        (crate::wire::logs::LogSource, String),
        (),
        nyanpasu_logging::LogError
    );
    // R/commands/logs.rs
    command!(
        "report_frontend_events",
        (crate::wire::logs::FrontendEventBatch),
        (),
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "get_profiles",
        (),
        nyanpasu_config::profile::Profiles,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!("is_portable", (), bool, receive::IpcError);
    // R/commands/profiles.rs
    command!("enhance_profiles", (), (), receive::IpcError);
    // R/commands/profiles.rs
    command!(
        "import_profile",
        (
            String,
            Option<String>,
            Option<nyanpasu_config::profile::RemoteProfileOptionsPatch>,
            Option<nyanpasu_config::profile::TransformKind>,
            Option<nyanpasu_config::profile::FolderId>
        ),
        receive::MutationOutcome<nyanpasu_config::profile::ProfileId>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "create_profile",
        (crate::wire::profiles::NewProfileRequest, Option<String>),
        receive::MutationOutcome<nyanpasu_config::profile::ProfileId>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "reorder_profile",
        (
            nyanpasu_config::profile::ProfileId,
            nyanpasu_config::profile::ProfileId
        ),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "reorder_profiles_by_list",
        (Vec<nyanpasu_config::profile::ProfileId>),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "set_profile_layout",
        (nyanpasu_config::profile::ProfileLayout),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "set_proxy_group_layout",
        (
            nyanpasu_config::profile::ProfileId,
            nyanpasu_config::profile::ProxyGroupLayout
        ),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "create_proxy_group_folder",
        (nyanpasu_config::profile::ProfileId, String, Vec<String>),
        receive::MutationOutcome<nyanpasu_config::profile::FolderId>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "create_profile_folder",
        (String, Vec<nyanpasu_config::profile::ProfileId>),
        receive::MutationOutcome<nyanpasu_config::profile::FolderId>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "get_profile_sync_status",
        (nyanpasu_config::profile::ProfileId),
        crate::wire::profiles::ProfileSyncStatus,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "get_profile_sync_runs",
        (
            nyanpasu_config::profile::ProfileId,
            Option<nyanpasu_jobs::dto::RunCursorDto>
        ),
        nyanpasu_jobs::dto::RunPageDto,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "get_profile_sync_logs",
        (nyanpasu_config::profile::ProfileId, String, Option<String>),
        nyanpasu_jobs::dto::LogPageDto,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "update_profile",
        (
            nyanpasu_config::profile::ProfileId,
            Option<nyanpasu_config::profile::RemoteProfileOptionsPatch>
        ),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "delete_profile",
        (nyanpasu_config::profile::ProfileId),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "activate_profile",
        (Option<nyanpasu_config::profile::ProfileId>),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "set_global_transforms",
        (Vec<nyanpasu_config::profile::ProfileId>),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "set_profile_valid_fields",
        (Vec<String>),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "patch_profile_metadata",
        (
            nyanpasu_config::profile::ProfileId,
            nyanpasu_config::profile::ProfileMetadataPatch
        ),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "patch_remote_profile_options",
        (
            nyanpasu_config::profile::ProfileId,
            nyanpasu_config::profile::RemoteProfileOptionsPatch
        ),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "replace_profile_definition",
        (
            nyanpasu_config::profile::ProfileId,
            nyanpasu_config::profile::ProfileDefinition
        ),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "read_profile_file",
        (nyanpasu_config::profile::ProfileId),
        String,
        receive::IpcError
    );
    // R/commands/profiles.rs
    command!(
        "save_profile_file",
        (nyanpasu_config::profile::ProfileId, String),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "get_clash_info",
        (),
        crate::wire::runtime::ClashInfo,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "get_runtime_config",
        (),
        Option<serde_json::Value>,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!("get_runtime_yaml", (), String, receive::IpcError);
    // R/commands/runtime.rs
    command!(
        "inspect_runtime",
        (),
        Option<crate::wire::runtime::RuntimeInspection>,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "inspect_applied_runtime",
        (),
        Option<crate::wire::runtime::RuntimeInspection>,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "inspect_runtime_node",
        (String, u32),
        crate::wire::runtime::RuntimeInspectionContent,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!("get_runtime_exists", (), Vec<String>, receive::IpcError);
    // R/commands/runtime.rs
    command!(
        "get_postprocessing_output",
        (),
        nyanpasu_profile_transform::PostProcessingOutput,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "get_core_status",
        (),
        crate::wire::core::CoreStatusInfo,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "probe_direct_egress",
        (),
        crate::wire::runtime::DirectEgress,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "get_ip_lookup_providers",
        (),
        Vec<nyanpasu_config::application::IpLookupProviderInfo>,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "get_app_config",
        (),
        nyanpasu_config::application::NyanpasuAppConfig,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "patch_app_config",
        (nyanpasu_config::application::NyanpasuAppConfigPatch),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "get_clash_config",
        (),
        nyanpasu_config::clash::config::ClashConfig,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "get_config_features",
        (),
        nyanpasu_config::clash::features::ConfigFeatures,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "patch_clash_config",
        (nyanpasu_config::clash::config::ClashConfigPatch),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!(
        "patch_runtime_overrides",
        (nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!("get_hotkey_functions", (), Vec<String>, receive::IpcError);
    // R/commands/runtime.rs
    command!(
        "change_clash_core",
        (nyanpasu_config::application::ClashCore),
        (),
        receive::IpcError
    );
    // R/commands/runtime.rs
    command!("restart_sidecar", (), (), receive::IpcError);
    // R/commands/runtime.rs
    command!(
        "get_sys_proxy",
        (),
        crate::wire::host::GetSysProxyResponse,
        receive::IpcError
    );
    // R/commands/status.rs
    command!(
        "status_service",
        (),
        crate::wire::host::ServiceStatusInfo,
        receive::IpcError
    );
    // R/commands/status.rs
    command!(
        "get_configuration_status",
        (),
        crate::wire::configuration::ConfigurationStatus,
        receive::IpcError
    );
    // R/commands/status.rs
    command!("retry_configuration_runtime", (), (), receive::IpcError);
    // R/commands/status.rs
    command!(
        "retry_configuration_effect",
        (crate::wire::configuration::EffectKind),
        (),
        receive::IpcError
    );
    // R/commands/status.rs
    command!(
        "get_debug_http_status",
        (),
        crate::wire::configuration::DebugHttpStatus,
        receive::IpcError
    );
    // R/commands/storage.rs
    command!(
        "get_storage_item",
        (String),
        crate::wire::storage::StorageItem<String>,
        receive::IpcError
    );
    // R/commands/storage.rs
    command!(
        "set_storage_item",
        (String, String),
        crate::wire::storage::StorageRevision,
        receive::IpcError
    );
    // R/commands/storage.rs
    command!(
        "remove_storage_item",
        (String),
        crate::wire::storage::StorageRevision,
        receive::IpcError
    );
    // R/commands/storage.rs
    command!("get_hotkeys", (), Vec<String>, receive::IpcError);
    // R/commands/storage.rs
    command!(
        "set_hotkeys",
        (Vec<String>),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // R/commands/storage.rs
    command!(
        "get_all_storage_items",
        (),
        Vec<crate::wire::host::StorageEntry>,
        receive::IpcError
    );
    // R/commands/storage.rs
    command!("clear_storage", (), (), receive::IpcError);
    // R/commands/traffic.rs
    command!(
        "get_clash_ws_snapshot",
        (),
        crate::wire::streams::ClashWsSnapshot,
        receive::IpcError
    );
    // R/commands/traffic.rs
    command!(
        "query_core_logs",
        (crate::wire::logs::CoreLogQuery),
        crate::wire::logs::CoreLogPage,
        wire::logs::CoreLogError
    );
    // R/commands/traffic.rs
    command!(
        "get_core_log",
        (crate::wire::logs::CoreLogCursor),
        crate::wire::logs::CoreLogRecord,
        wire::logs::CoreLogError
    );
    // R/commands/traffic.rs
    command!(
        "get_core_log_status",
        (),
        crate::wire::logs::CoreLogStatus,
        wire::logs::CoreLogError
    );
    // R/commands/traffic.rs
    command!("clear_core_logs", (), (), wire::logs::CoreLogError);
    // R/commands/traffic.rs
    command!(
        "get_traffic_summary",
        (),
        nyanpasu_traffic::TrafficSummary,
        receive::IpcError
    );
    // R/commands/traffic.rs
    command!(
        "query_traffic_report",
        (nyanpasu_traffic::ReportRequest),
        nyanpasu_traffic::TrafficReport,
        receive::IpcError
    );
    // R/commands/traffic.rs
    command!(
        "query_traffic_usage",
        (
            nyanpasu_traffic::TrafficQuery,
            nyanpasu_traffic::Dimension,
            nyanpasu_traffic::Metric,
            Option<nyanpasu_traffic::UsageCursor>,
            usize
        ),
        nyanpasu_traffic::UsagePage,
        receive::IpcError
    );
    // R/commands/traffic.rs
    command!(
        "query_traffic_usage_by_keys",
        (
            nyanpasu_traffic::TrafficQuery,
            nyanpasu_traffic::Dimension,
            Vec<String>
        ),
        Vec<nyanpasu_traffic::UsageGroup>,
        receive::IpcError
    );
    // R/commands/traffic.rs
    command!(
        "query_traffic_closed_connections",
        (
            nyanpasu_traffic::TrafficRange,
            Vec<nyanpasu_traffic::TrafficFilter>,
            Option<nyanpasu_traffic::ClosedCursor>,
            usize
        ),
        nyanpasu_traffic::ClosedPage,
        receive::IpcError
    );
    // R/commands/traffic.rs
    command!(
        "query_traffic_active_connection_ids",
        (Vec<nyanpasu_traffic::TrafficFilter>),
        Vec<String>,
        receive::IpcError
    );
    // R/commands/traffic.rs
    command!(
        "set_clash_ws_recording",
        (crate::wire::streams::ClashWsKind, bool),
        crate::wire::streams::ClashWsRecording,
        receive::IpcError
    );
    // R/commands/traffic.rs
    command!(
        "clear_clash_ws_history",
        (crate::wire::streams::ClashWsKind),
        (),
        receive::IpcError
    );
    // R/commands/updater.rs
    command!(
        "get_core_agent_skills",
        (nyanpasu_config::application::ClashCore),
        Vec<crate::wire::profiles::CoreAgentSkill>,
        receive::IpcError
    );
    // R/commands/updater.rs
    command!(
        "fetch_latest_core_versions",
        (),
        crate::wire::updater::ManifestVersionLatest,
        receive::IpcError
    );
    // R/commands/updater.rs
    command!(
        "inspect_updater",
        (usize),
        crate::wire::updater::UpdaterSummary,
        receive::IpcError
    );
    // T/ipc.rs
    command!("read_clipboard_text", (), String, receive::IpcError);
    // T/ipc.rs
    command!("write_clipboard_text", (String), (), receive::IpcError);
    // T/ipc.rs
    command!(
        "show_native_notification",
        (String, Option<String>),
        (),
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "show_native_message_dialog",
        (
            String,
            Option<String>,
            crate::wire::native::NativeDialogKind,
            crate::wire::native::NativeDialogButtons
        ),
        String,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "ask_native_dialog",
        (
            String,
            Option<String>,
            crate::wire::native::NativeDialogKind
        ),
        bool,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "open_native_file_dialog",
        (Option<String>, Vec<crate::wire::native::FileDialogFilter>),
        Option<String>,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "probe_profile_url",
        (
            String,
            Option<nyanpasu_config::profile::RemoteProfileOptionsPatch>
        ),
        crate::wire::profiles::ProfileUrlProbe,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "take_pending_deep_links",
        (),
        Vec<String>,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "view_profile",
        (nyanpasu_config::profile::ProfileId),
        (),
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "url_delay_test",
        (String, u16),
        Option<u64>,
        receive::IpcError
    );
    // T/ipc.rs
    command!("get_ipsb_asn", (), serde_json::Value, receive::IpcError);
    // T/ipc.rs
    command!(
        "test_stun_server",
        (String, u16),
        nyanpasu_contracts::net::StunServerTest,
        receive::IpcError
    );
    // T/ipc.rs
    command!("flush_system_dns_cache", (), (), receive::IpcError);
    // T/ipc.rs
    command!("open_app_config_dir", (), (), receive::IpcError);
    // T/ipc.rs
    command!("open_app_data_dir", (), (), receive::IpcError);
    // T/ipc.rs
    command!(
        "create_config_backup",
        (),
        crate::wire::native::ConfigBackupInfo,
        receive::IpcError
    );
    // T/ipc.rs
    command!("open_backups_dir", (), (), receive::IpcError);
    // T/ipc.rs
    command!("open_core_dir", (), (), receive::IpcError);
    // T/ipc.rs
    command!("open_logs_dir", (), (), receive::IpcError);
    // T/ipc.rs
    command!("open_web_url", (String), (), receive::IpcError);
    // T/ipc.rs
    command!(
        "get_core_version",
        (nyanpasu_config::application::ClashCore),
        String,
        receive::IpcError
    );
    // T/ipc.rs
    command!("collect_logs", (), (), receive::IpcError);
    // T/ipc.rs
    command!(
        "update_core",
        (nyanpasu_config::application::ClashCore),
        usize,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "report_issue",
        (nyanpasu_diagnose::report::PageCapabilities),
        (),
        receive::IpcError
    );
    // T/ipc.rs
    command!("open_that", (String), (), receive::IpcError);
    // T/ipc.rs
    command!(
        "get_custom_app_dir",
        [cfg(windows)],
        (),
        Option<String>,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "get_custom_app_dir",
        [cfg(not(windows))],
        (),
        Option<String>,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "set_custom_app_dir",
        [cfg(windows)],
        (String),
        (),
        receive::IpcError
    );
    // T/ipc.rs
    command!("restart_application", (), (), receive::IpcError);
    // T/ipc.rs
    command!(
        "set_custom_app_dir",
        [cfg(not(windows))],
        (String),
        (),
        receive::IpcError
    );
    // T/ipc.rs
    command!("invoke_uwp_tool", [cfg(windows)], (), (), receive::IpcError);
    // T/ipc.rs
    command!(
        "set_tray_icon",
        (crate::values::TrayIcon, Option<std::path::PathBuf>),
        (),
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "set_widget_icon",
        (crate::values::TrayIcon, Option<std::path::PathBuf>),
        (),
        receive::IpcError
    );
    // T/ipc.rs
    command!("install_service", (), (), receive::IpcError);
    // T/ipc.rs
    command!("uninstall_service", (), (), receive::IpcError);
    // T/ipc.rs
    command!("start_service", (), (), receive::IpcError);
    // T/ipc.rs
    command!("stop_service", (), (), receive::IpcError);
    // T/ipc.rs
    command!("restart_service", (), (), receive::IpcError);
    // T/ipc.rs
    command!(
        "invoke_uwp_tool",
        [cfg(not(windows))],
        (),
        (),
        receive::IpcError
    );
    // T/ipc.rs; native Channel frame: wire::streams::ClashConnectionDetails
    command!(
        "subscribe_clash_connection_details",
        (),
        crate::wire::native::SubscriptionId,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "unsubscribe_clash_connection_details",
        (crate::wire::native::SubscriptionId),
        (),
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "get_release_channel",
        (),
        crate::wire::native::ReleaseChannelInfo,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "set_release_channel",
        (nyanpasu_config::application::ReleaseChannel),
        receive::MutationOutcome<()>,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "get_app_update_state",
        (),
        crate::wire::app_update::AppUpdateSnapshot,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "get_app_update_changelog",
        (String, String),
        crate::wire::app_update::AppUpdateChangelog,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "check_app_update",
        (),
        crate::wire::app_update::AppUpdateSnapshot,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "download_app_update",
        (),
        crate::wire::app_update::AppUpdateSnapshot,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "cancel_app_update_download",
        (),
        crate::wire::app_update::AppUpdateSnapshot,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "install_app_update",
        (),
        crate::wire::app_update::AppUpdateSnapshot,
        receive::IpcError
    );
    // T/ipc.rs
    command!(
        "discard_app_update_package",
        (),
        crate::wire::app_update::AppUpdateSnapshot,
        receive::IpcError
    );
    // T/ipc.rs
    command!("save_window_size_state", (String), (), receive::IpcError);
    // T/ipc.rs
    command!("create_main_window", (), (), receive::IpcError);
    // T/ipc.rs
    command!("create_debug_tray_menu_window", (), (), receive::IpcError);
    // T/ipc.rs
    command!("simulate_crash", (bool), (), receive::IpcError);
    // T/ipc.rs
    command!(
        "copy_clash_env",
        (crate::wire::native::CopyEnvOption),
        (),
        receive::IpcError
    );
    // T/ipc.rs
    command!("quit_application", (), (), receive::IpcError);
    // T/ipc.rs
    command!(
        "create_editor_window",
        (crate::wire::native::EditorWindowType, Option<String>),
        (),
        receive::IpcError
    );
    // T/ipc.rs
    command!("report_window_ready", (), (), receive::IpcError);
    // T/ipc.rs
    command!(
        "set_debug_http_enabled",
        (bool),
        crate::wire::configuration::DebugHttpStatus,
        receive::IpcError
    );
    // Parse the generated source, so both halves of mutually exclusive platform cfgs
    // participate even on a host where only one declaration is compiled.
    let source = syn::parse_file(include_str!("../command_catalog.rs")).unwrap();
    let catalog = source
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Const(item) if item.ident == "COMMAND_CATALOG" => Some(item),
            _ => None,
        })
        .unwrap();
    let syn::Expr::Reference(reference) = &*catalog.expr else {
        panic!("catalog reference")
    };
    let syn::Expr::Array(array) = &*reference.expr else {
        panic!("catalog array")
    };
    let mut registered = Vec::new();
    for entry in &array.elems {
        let syn::Expr::Struct(entry) = entry else {
            panic!("catalog entry")
        };
        let name = entry
            .fields
            .iter()
            .find(|field| field.member == syn::Member::Named(syn::parse_quote!(name)))
            .unwrap();
        let syn::Expr::Lit(name) = &name.expr else {
            panic!("catalog name")
        };
        let syn::Lit::Str(name) = &name.lit else {
            panic!("catalog name string")
        };
        let guards: Vec<_> = entry
            .attrs
            .iter()
            .map(|attr| compact(&attr.meta.to_token_stream().to_string()))
            .collect();
        registered.push((name.value(), guards));
    }
    commands.sort();
    registered.sort();
    assert_eq!(
        commands, registered,
        "declaration names, guards and multiplicity"
    );

    // Wrappers in the desktop crate add Event/Specta identity, not another payload layer.
    let mut events = BTreeSet::new();
    macro_rules! event {
        ($name:ident, $payload:ty) => {{
            response::<$payload>();
            events.insert(EventName::$name.as_str());
        }};
    }
    event!(AppUpdateStateChanged, wire::app_update::AppUpdateSnapshot);
    event!(ClashWs, wire::streams::ClashWsPacket);
    event!(
        ConfigurationStatusChanged,
        wire::configuration::ConfigurationStatus
    );
    event!(CoreLogsChanged, wire::events::CoreLogsChanged);
    event!(CoreStatusChanged, wire::core::CoreStatusInfo);
    event!(SchemeRequestReceived, ());
    event!(ServiceStatusChanged, wire::core::ServiceHostStatus);
    event!(StateChanged, wire::events::StateChanged);
    event!(StorageResync, wire::events::StorageResync);
    event!(StorageValueChanged, wire::events::StorageValueChanged);
    event!(WindowMessage, wire::events::WindowMessage);
    response::<wire::streams::ClashConnectionDetails>();
    assert_eq!(
        events,
        EventName::ALL.map(EventName::as_str).into_iter().collect()
    );
}

fn native_values() {
    use serde_json::json;
    fn same<T: Serialize + DeserializeOwned>(value: serde_json::Value) {
        let received: T = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(received).unwrap(), value);
    }
    for kind in ["info", "warning", "error"] {
        same::<wire::native::NativeDialogKind>(json!(kind));
    }
    for buttons in [
        json!({"type":"ok"}),
        json!({"type":"ok_cancel"}),
        json!({"type":"yes_no"}),
        json!({"type":"yes_no_cancel"}),
        json!({"type":"ok_custom", "ok":"Continue"}),
        json!({"type":"ok_cancel_custom", "ok":"Save", "cancel":"Back"}),
        json!({"type":"yes_no_cancel_custom", "yes":"Accept", "no":"Refuse", "cancel":"Back"}),
    ] {
        same::<wire::native::NativeDialogButtons>(buttons);
    }
    for option in ["shell", "cmd", "pwsh"] {
        same::<wire::native::CopyEnvOption>(json!(option));
    }
    for option in ["profile", "css-editor"] {
        same::<wire::native::EditorWindowType>(json!(option));
    }
    same::<wire::native::FileDialogFilter>(json!({"name":"YAML", "extensions":["yaml", "yml"]}));
    same::<wire::native::ConfigBackupInfo>(json!({"name":"backup", "path":"C:/data/測試.zip"}));
    same::<wire::native::ReleaseChannelInfo>(json!({"current":"stable", "installed":"nightly"}));
    same::<wire::native::SubscriptionId>(json!(7));
    let page: nyanpasu_diagnose::report::PageCapabilities =
        serde_json::from_value(json!({})).unwrap();
    assert!(page.webgl.is_none());
    assert!(page.webgpu.is_none());
    same::<nyanpasu_diagnose::report::PageCapabilities>(json!({
        "userAgent":"test", "fullVersionList":[{"brand":"browser", "version":"1"}],
        "platformVersion":null, "webgl":{"vendor":null, "renderer":"test"},
        "webgpu":{"available":true, "adapter":{"vendor":"test", "architecture":null,"device":null,"description":null}}, "devicePixelRatio":1.25
    }));
    same::<wire::events::StorageResync>(json!({}));
    same::<wire::events::StorageValueChanged>(json!({"key":"removed", "value":null, "revision":9}));
    same::<wire::events::WindowMessage>(
        json!({"from":"main", "to":"*", "event":"ready", "payload":[null, {"nested":true}]}),
    );
}

fn compact(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}
