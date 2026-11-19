//! Single source of truth for the tauri-specta builder.
//! Shared by `lib.rs` (runtime registration + debug export) and the
//! `export_typescript_bindings` test (CI freshness).

use nyanpasu_rpc::commands;
use tauri_specta::{collect_commands, collect_events};
use tauri_specta_query::{CommandSet, TanstackQueryFramework};

use crate::{adapters::events, ipc, unified_rpc, window};

pub(crate) fn build_transport_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        // The only direct Tauri command is the UnifiedRpc transport entrypoint.
        // Application operations, including Channel subscription control, belong
        // in the schema builder below and dispatch through call_rpc.
        .commands(collect_commands![unified_rpc::native::call_rpc])
        .events(collect_events![
            events::AppUpdateStateChanged,
            events::AppUpdateSummaryChanged,
            events::SyncStatusChanged,
            events::CoreLogsChangedEvent,
            events::CoreLogsSummaryChangedEvent,
            events::ClashWsEvent,
            window::WindowMessageEvent,
            events::StorageValueChangedEvent,
            events::StorageResyncEvent,
            ipc::system::os::SchemeRequestReceivedEvent,
            events::CoreStatusChangedEvent,
            ipc::ConfigurationStatusChanged,
            events::ServiceStatusChangedEvent,
            events::StateChangedEvent,
            ipc::ConfigurationStatusSummaryChanged,
            events::CoreStatusSummaryChangedEvent,
            events::ServiceStatusSummaryChangedEvent
        ])
        .dangerously_cast_bigints_to_number()
}

/// Collect only function schemas; shared declarations have no native wrappers.
macro_rules! schema_commands {
    ($($command:tt)*) => {
        tauri_specta::Commands::<tauri::Wry>(
            std::sync::Arc::new(|_| false),
            std::sync::Arc::new(specta::function::collect_functions![$($command)*]),
        )
    };
}

pub(crate) fn build_specta_builder() -> (String, tauri_specta::Builder<tauri::Wry>) {
    let command_set = CommandSet::<tauri::Wry>::new(
        schema_commands![
            commands::meta::rpc_describe,
            commands::system::status::get_http_server_status,
            commands::remote::settings::get_local_socket_status,
            commands::remote::settings::list_local_networks,
            commands::remote::settings::get_remote_access_certificate,
            commands::system::status::get_configuration_status,
            commands::system::status::get_configuration_status_summary,
            commands::system::service::get_service_status_summary,
            commands::system::core::get_core_status_summary,
            // Read-only commands
            ipc::system::os::read_clipboard_text,
            commands::logs::core::query_core_logs,
            commands::logs::core::query_core_logs_summary,
            commands::logs::core::query_meow_logbook,
            commands::logs::core::get_core_log,
            commands::logs::core::get_core_log_status,
            commands::logs::core::get_core_log_status_summary,
            commands::logs::app::list_log_files,
            commands::system::proxy::get_sys_proxy,
            commands::settings::clash::get_clash_info,
            commands::profile::runtime::get_runtime_config,
            commands::profile::runtime::get_runtime_yaml,
            commands::profile::runtime::get_runtime_exists,
            commands::profile::runtime::inspect_runtime,
            commands::profile::runtime::inspect_applied_runtime,
            commands::profile::runtime::inspect_runtime_node,
            commands::profile::runtime::get_postprocessing_output,
            commands::proxy::delay::clash_api_get_proxy_delay,
            commands::settings::clash::clash_api_get_settings,
            commands::settings::clash::clash_api_get_configs,
            commands::system::core::clash_api_get_version,
            commands::proxy::rules::clash_api_get_rules,
            commands::proxy::rules::clash_api_get_providers_rules,
            commands::proxy::delay::clash_api_get_group_delay,
            commands::proxy::providers::clash_api_get_providers_proxies,
            commands::system::core::fetch_latest_core_versions,
            commands::system::core::inspect_updater,
            commands::system::core::inspect_updater_summary,
            ipc::system::core::get_core_version,
            commands::settings::app::get_app_settings,
            commands::settings::app::get_app_config,
            commands::settings::clash::get_clash_settings,
            commands::settings::clash::get_clash_config,
            commands::system::core::get_config_features,
            commands::system::network::get_ip_lookup_providers,
            commands::system::hotkeys::get_hotkey_functions,
            commands::profile::store::get_profiles,
            commands::profile::store::get_profiles_listing,
            ipc::profile::content::probe_profile_url,
            commands::profile::sync::get_profile_sync_status,
            commands::profile::sync::get_profile_sync_runs,
            commands::profile::sync::get_profile_sync_status_summary,
            commands::profile::sync::get_profile_sync_runs_summary,
            commands::profile::sync::get_profile_sync_logs,
            commands::profile::content::read_profile_file,
            ipc::system::os::get_custom_app_dir,
            commands::system::service::status_service,
            commands::system::os::is_portable,
            commands::proxy::groups::get_proxies,
            commands::proxy::groups::get_proxy_group_icons,
            commands::system::icons::is_tray_icon_set,
            commands::system::icons::is_widget_icon_set,
            commands::system::core::get_core_status,
            commands::system::network::get_network_automation_status,
            ipc::proxy::delay::url_delay_test,
            ipc::system::network::get_ipsb_asn,
            commands::system::os::is_appimage,
            commands::system::service::get_service_install_prompt,
            commands::settings::storage::get_storage_item,
            commands::settings::storage::get_all_storage_items,
            commands::system::hotkeys::get_hotkeys,
            commands::system::core::get_core_dir,
            commands::system::core::get_core_agent_skills,
            commands::system::core::get_core_agent_skills_summary,
            commands::traffic::connections::get_clash_ws_snapshot,
            commands::traffic::usage::get_traffic_summary,
            commands::traffic::usage::get_meow_statistics,
            commands::traffic::usage::query_meow_traffic_ledger,
            commands::traffic::usage::query_traffic_report,
            commands::traffic::usage::query_traffic_usage,
            commands::traffic::usage::query_traffic_usage_by_keys,
            commands::traffic::connections::query_traffic_closed_connections,
            commands::traffic::connections::query_traffic_active_connection_ids,
            ipc::system::update::get_app_update_state,
            ipc::system::update::get_app_update_summary,
            ipc::system::update::get_app_update_changelog,
            ipc::settings::app::get_release_channel,
            commands::system::os::get_system_accent_color,
            ipc::system::proxy::get_transparent_proxy_status,
            ipc::system::proxy::get_transparent_proxy_status_summary,
            commands::remote::tokens::list_remote_access_tokens,
            commands::remote::sessions::list_remote_browser_sessions,
            commands::settings::sync::get_sync_status,
            commands::settings::sync::get_sync_settings,
            commands::settings::sync::get_sync_provider_kinds,
            commands::settings::sync::list_sync_conflicts,
            commands::settings::sync::list_sync_snapshots,
        ],
        schema_commands![
            ipc::system::os::write_clipboard_text,
            ipc::system::os::show_native_notification,
            ipc::system::os::show_native_message_dialog,
            ipc::system::os::ask_native_dialog,
            ipc::system::os::open_native_file_dialog,
            commands::system::status::retry_configuration_runtime,
            commands::system::status::retry_configuration_effect,
            ipc::settings::app::set_release_channel,
            ipc::system::update::check_app_update,
            ipc::system::update::download_app_update,
            ipc::system::update::cancel_app_update_download,
            ipc::system::update::install_app_update,
            ipc::system::update::discard_app_update_package,
            // Creating/releasing a detail receiver changes stream demand.
            ipc::traffic::connections::subscribe_clash_connection_details,
            ipc::traffic::connections::unsubscribe_clash_connection_details,
            // Side-effecting commands
            commands::settings::network::probe_direct_egress,
            ipc::settings::network::test_stun_server,
            commands::logs::core::clear_core_logs,
            commands::logs::app::open_log_session,
            commands::logs::app::query_logs,
            commands::logs::app::query_logs_summary,
            commands::logs::app::close_log_session,
            commands::logs::app::report_frontend_events,
            ipc::system::os::flush_system_dns_cache,
            ipc::system::os::open_app_config_dir,
            ipc::system::os::open_app_data_dir,
            ipc::system::os::open_backups_dir,
            ipc::settings::backup::create_config_backup,
            ipc::system::os::open_logs_dir,
            ipc::system::os::open_web_url,
            ipc::remote::open_remote_access_certificate,
            ipc::remote::export_remote_access_authority,
            ipc::system::core::open_core_dir,
            ipc::system::os::install_cli,
            commands::system::core::restart_sidecar,
            commands::settings::app::patch_app_config,
            commands::system::proxy::patch_system_settings,
            commands::settings::clash::patch_clash_config,
            commands::system::proxy::patch_clash_system_config,
            commands::system::network::apply_network_automation,
            commands::system::network::request_wifi_name_access,
            commands::settings::clash::patch_runtime_overrides,
            commands::system::proxy::patch_runtime_access,
            commands::proxy::mode::set_proxy_mode,
            commands::system::core::change_clash_core,
            commands::proxy::connections::clash_api_delete_connections,
            commands::profile::providers::clash_api_update_providers_rules,
            ipc::system::os::invoke_uwp_tool,
            ipc::system::core::update_core,
            ipc::system::os::collect_logs,
            ipc::system::os::report_issue,
            commands::system::icons::set_tray_icon_from_bytes,
            commands::system::icons::set_widget_icon_from_bytes,
            commands::profile::store::enhance_profiles,
            commands::profile::store::import_profile,
            ipc::system::os::take_pending_deep_links,
            commands::profile::store::create_profile,
            commands::profile::store::reorder_profile,
            commands::profile::store::reorder_profiles_by_list,
            commands::profile::layout::set_profile_layout,
            commands::profile::layout::create_profile_folder,
            commands::profile::layout::set_proxy_group_layout,
            commands::profile::layout::create_proxy_group_folder,
            commands::profile::store::update_profile,
            commands::profile::store::delete_profile,
            commands::profile::store::activate_profile,
            commands::profile::store::set_global_transforms,
            commands::profile::store::set_profile_valid_fields,
            commands::profile::store::patch_profile_metadata,
            commands::profile::store::patch_remote_profile_options,
            commands::profile::store::replace_profile_definition,
            ipc::system::os::view_profile,
            commands::profile::content::save_profile_file,
            ipc::system::os::set_custom_app_dir,
            ipc::system::service::install_service,
            ipc::system::service::uninstall_service,
            ipc::system::service::start_service,
            ipc::system::service::stop_service,
            ipc::system::service::restart_service,
            commands::system::service::set_service_mode,
            commands::remote::settings::set_remote_access,
            commands::remote::settings::reset_remote_access_tls,
            commands::remote::sessions::create_http_launch_url,
            commands::remote::tokens::create_remote_access_token,
            commands::remote::tokens::revoke_remote_access_token,
            commands::remote::sessions::revoke_remote_browser_session,
            commands::proxy::groups::select_proxy,
            commands::proxy::groups::clear_proxy_fixed,
            commands::profile::providers::update_proxy_provider,
            commands::proxy::providers::clash_api_healthcheck_proxy_provider,
            ipc::system::window::restart_application,
            ipc::system::icons::set_tray_icon,
            ipc::system::icons::set_widget_icon,
            ipc::system::os::open_that,
            commands::settings::storage::set_storage_item,
            commands::settings::storage::remove_storage_item,
            commands::settings::storage::clear_storage,
            commands::system::hotkeys::set_hotkeys,
            commands::proxy::groups::mutate_proxies,
            commands::traffic::connections::set_clash_ws_recording,
            commands::traffic::connections::clear_clash_ws_history,
            ipc::system::window::save_window_size_state,
            ipc::system::window::create_main_window,
            ipc::system::window::create_debug_tray_menu_window,
            ipc::system::window::simulate_crash,
            ipc::system::window::create_editor_window,
            ipc::system::window::report_window_ready,
            ipc::system::os::copy_clash_env,
            ipc::system::window::quit_application,
            commands::settings::sync::configure_sync,
            commands::settings::sync::test_sync_connection,
            commands::settings::sync::enable_sync,
            commands::settings::sync::disable_sync,
            commands::settings::sync::sync_now,
            commands::settings::sync::resolve_sync_conflict,
            commands::settings::sync::restore_sync_snapshot,
        ],
    )
    .events(collect_events![
        events::AppUpdateStateChanged,
        events::AppUpdateSummaryChanged,
        events::SyncStatusChanged,
        events::CoreLogsChangedEvent,
        events::ClashWsEvent,
        window::WindowMessageEvent,
        events::StorageValueChangedEvent,
        events::StorageResyncEvent,
        ipc::system::os::SchemeRequestReceivedEvent,
        events::CoreStatusChangedEvent,
        ipc::ConfigurationStatusChanged,
        events::ServiceStatusChangedEvent,
        events::StateChangedEvent,
        ipc::ConfigurationStatusSummaryChanged,
        events::CoreStatusSummaryChangedEvent,
        events::ServiceStatusSummaryChangedEvent,
        events::CoreLogsSummaryChangedEvent
    ])
    // PR-3 T01: profile domain types, add-only. Commands referencing them
    // arrive with T08; explicit registration keeps them exported (and the
    // specta nested-tagged-enum risk probed) before any command exists.
    .typ::<nyanpasu_config::profile::Profiles>()
    .typ::<nyanpasu_rpc::icons::IconRequest>()
    .typ::<nyanpasu_config::profile::FileConfig>()
    .typ::<nyanpasu_config::profile::CompositionConfig>()
    .typ::<nyanpasu_config::profile::OverlayTransform>()
    .typ::<nyanpasu_config::profile::ScriptTransform>()
    .typ::<nyanpasu_config::profile::MaterializedFile>()
    .typ::<nyanpasu_config::profile::ProfileMetadataPatch>()
    .typ::<nyanpasu_config::profile::RemoteProfileOptionsPatch>()
    .typ::<nyanpasu_config::profile::ProfileValidationError>()
    .build(TanstackQueryFramework::React);

    let (query_bindings, builder) = command_set;

    (
        query_bindings,
        permission_constants(builder).dangerously_cast_bigints_to_number(),
    )
}

/// A command an HTTP caller may reach, as the token dialog lists it.
#[derive(serde::Serialize, specta::Type)]
struct HttpCommand {
    name: &'static str,
    /// The module path below the command root, such as `proxy.delay`.
    namespace: &'static str,
    permission: nyanpasu_config::application::Permission,
    effect: nyanpasu_rpc_protocol::CommandEffect,
    sensitive: bool,
}

/// The role expansions, the anonymous limit, what a token can be granted and the commands HTTP
/// offers, which the frontend shows and explains to the user; only the backend validates and
/// admits.
fn permission_constants(
    builder: tauri_specta::Builder<tauri::Wry>,
) -> tauri_specta::Builder<tauri::Wry> {
    use nyanpasu_config::application::{ANONYMOUS_PERMISSIONS_LIMIT, Role};
    use nyanpasu_contracts::auth::Grant;
    use nyanpasu_rpc_protocol::{RpcTarget, permission::RESOURCES};
    use strum::IntoEnumIterator as _;

    let roles = Role::iter()
        .map(|role| (role.to_string(), role.permissions()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let http_commands: Vec<HttpCommand> = RESOURCES
        .iter()
        .filter(|resource| resource.is_command() && resource.targets.contains(&RpcTarget::Http))
        .map(|command| HttpCommand {
            name: command.name,
            namespace: command.namespace,
            permission: command.permission,
            effect: command.effect,
            sensitive: command.sensitive,
        })
        .collect();
    builder
        .constant("ROLE_PERMISSIONS", roles)
        .constant("ANONYMOUS_PERMISSIONS_LIMIT", ANONYMOUS_PERMISSIONS_LIMIT)
        .constant("ASSIGNABLE_ROLES", Grant::ASSIGNABLE_ROLES)
        .constant(
            "GRANTABLE_RESOURCES",
            Grant::grantable_resources().collect::<Vec<_>>(),
        )
        .constant("HTTP_COMMANDS", http_commands)
}

pub(crate) fn adapt_rpc_bindings(path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
    adapt_command_transport(path.as_ref())?;
    adapt_event_transport(path)
}

pub(crate) fn adapt_query_bindings(
    path: impl AsRef<std::path::Path>,
    query_bindings: &str,
) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};

    let queries_start = "export const queries = {";
    let mutations_start = "export const mutations = {";
    if query_bindings.matches(queries_start).count() != 1
        || query_bindings.matches(mutations_start).count() != 1
    {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "TanStack Query bindings changed; query package factory was not generated",
        ));
    }

    let source = query_bindings
        .replacen(queries_start, "const queries = {", 1)
        .replacen(mutations_start, "const mutations = {", 1);
    let queries_start = source.find("const queries = {").unwrap();
    let imports = &source[..queries_start];
    let source = format!(
        "{}\nimport type {{ RpcClient }} from '@nyanpasu/rpc'\n\nexport function createQueryBindings(rpc: RpcClient) {{\n  const commands = rpc\n\n{}\n  return {{ queries, mutations }}\n}}\n",
        imports,
        &source[queries_start..]
    );
    std::fs::write(path, source)
}

fn adapt_command_transport(path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};

    const ORIGINAL: &str = "import { invoke as __TAURI_INVOKE } from \"@tauri-apps/api/core\";";
    const REPLACEMENT: &str = "";

    const CHANNEL_ORIGINAL: &str =
        "import { invoke as __TAURI_INVOKE, Channel } from \"@tauri-apps/api/core\";";
    const CHANNEL_TYPE_IMPORT: &str = "import type { Channel } from \"@tauri-apps/api/core\";";

    let path = path.as_ref();
    let source = std::fs::read_to_string(path)?;
    let original = if source.contains(CHANNEL_ORIGINAL) {
        CHANNEL_ORIGINAL
    } else {
        ORIGINAL
    };
    if source.matches(original).count() != 1 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "tauri-specta invoke import changed; command transport was not installed",
        ));
    }
    // Channel is only a desktop command argument type. Keeping its import
    // erased makes the shared schema safe to load in a browser.
    let replacement = if original == CHANNEL_ORIGINAL {
        CHANNEL_TYPE_IMPORT.to_owned()
    } else {
        REPLACEMENT.to_owned()
    };
    let source = source
        .replacen(original, &replacement, 1)
        .replace("__TAURI_INVOKE", "__RPC_INVOKE");
    let commands_start = "export const commands = {";
    let events_start = "/** Events */";
    let types_start = "/* Types */";
    if source.matches(commands_start).count() != 1
        || source.matches(events_start).count() != 1
        || source.matches(types_start).count() != 1
    {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "tauri-specta output changed; RPC client factory was not generated",
        ));
    }
    let command_at = source.find(commands_start).unwrap();
    let events_at = source.find(events_start).unwrap();
    let types_at = source.find(types_start).unwrap();
    let prefix = format!(
        "import type {{ RpcTransport }} from './transport'\nimport {{ createDefaultRpcTransport }} from './transport'\n\nexport function createRpcClient(transport: RpcTransport = createDefaultRpcTransport()) {{\n  const __RPC_INVOKE = transport.commands.invoke.bind(transport.commands)\n"
    );
    let commands = source[command_at..events_at].replacen(commands_start, "const commands = {", 1);
    // Constants follow the events; they stay module exports, outside the client factory.
    let (events, constants) = source[events_at..types_at].split_at(
        source[events_at..types_at]
            .find("/* Constants */")
            .unwrap_or(types_at - events_at),
    );
    let events_and_separator = events.replacen(events_start, "", 1);
    let event_object_at = events_and_separator
        .find("export const events = {")
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing generated event object"))?;
    let mut events_and_separator = events_and_separator;
    events_and_separator.replace_range(
        event_object_at..event_object_at + "export const events = {".len(),
        "const events = {",
    );
    let commands_and_events = format!(
        "{prefix}{commands}{events_and_separator}\n  return {{ ...commands, events, icons: transport.icons, listenResync: transport.events.listenResync.bind(transport.events), dispose: () => transport.events.dispose() }}\n}}\n\nexport type RpcClient = ReturnType<typeof createRpcClient>\n\n{constants}\n"
    );
    let source = format!(
        "{}{}{}",
        &source[..command_at],
        commands_and_events,
        &source[types_at..]
    );
    std::fs::write(path, source)
}

fn adapt_event_transport(path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};

    const EVENT_IMPORT: &str = "import * as __TAURI_EVENT from \"@tauri-apps/api/event\";";
    const EVENT_IMPL_START: &str = "type EventEmit<T> = [T] extends [null]";
    const EVENT_IMPL_END: &str = "    return Object.assign(fn, base);\n}";
    const RPC_EVENT_IMPL: &str = r#"type EventEmit<T> = [T] extends [null] ? () => Promise<void> : (payload: T) => Promise<void>;
type RpcEvent<T> = { event: string; id: number; payload: T };
type RpcEventCallback<T> = (event: RpcEvent<T>) => void;

import type { RpcEventTransport } from './event-transport';

function makeEvent<T>(eventTransport: RpcEventTransport, name: string, serialize?: (payload: T) => unknown, deserialize?: (payload: any) => T) {
    const mapEvent = (cb: RpcEventCallback<T>) => (event: RpcEvent<any>) => cb({ ...event, payload: deserialize ? deserialize(event.payload) : event.payload });
    const mapPayload = (payload: T) => serialize ? serialize(payload) : payload;
    return {
        listen: (cb: RpcEventCallback<T>) => eventTransport.listen(name, mapEvent(cb)),
        once: (cb: RpcEventCallback<T>) => eventTransport.once(name, mapEvent(cb)),
        emit: ((payload: T) => eventTransport.emit(name, mapPayload(payload)) as unknown) as EventEmit<T>
    };
}"#;

    let path = path.as_ref();
    let source = std::fs::read_to_string(path)?;
    if source.matches(EVENT_IMPORT).count() != 1
        || source.matches(EVENT_IMPL_START).count() != 1
        || source.matches(EVENT_IMPL_END).count() != 1
    {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "tauri-specta event bindings changed; event transport was not installed",
        ));
    }
    let mut source = source.replacen(EVENT_IMPORT, "", 1);
    let start = source.find(EVENT_IMPL_START).unwrap();
    let end = source.find(EVENT_IMPL_END).unwrap() + EVENT_IMPL_END.len();
    source.replace_range(start..end, RPC_EVENT_IMPL);
    let runtime_source =
        source.replace("import type { Channel } from \"@tauri-apps/api/core\";", "");
    if runtime_source.contains("__TAURI_EVENT") || runtime_source.contains("new Channel") {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "RPC event binding still references a Tauri runtime adapter",
        ));
    }
    let source = inject_event_transport(&source)?;
    std::fs::write(path, source)
}

fn inject_event_transport(source: &str) -> std::io::Result<String> {
    use std::io::{Error, ErrorKind};

    let mut events_end = source
        .find("/* Types */")
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing generated types section"))?;
    let mut output = source.to_owned();
    let mut search_from = 0;
    while search_from < events_end {
        let Some(relative_start) = output[search_from..events_end].find("makeEvent<") else {
            break;
        };
        let start = search_from + relative_start;
        let mut depth = 0usize;
        let mut open = None;
        for (offset, ch) in output[start..].char_indices() {
            match ch {
                '<' => depth += 1,
                '>' => {
                    depth = depth.checked_sub(1).ok_or_else(|| {
                        Error::new(ErrorKind::InvalidData, "invalid generated event generic")
                    })?;
                    if depth == 0 {
                        open = Some(start + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let open = open.ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidData,
                "unterminated generated event generic",
            )
        })?;
        let paren = output[open..]
            .find('(')
            .map(|offset| open + offset)
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "invalid generated event call"))?;
        output.insert_str(paren + 1, "transport.events, ");
        events_end += "transport.events, ".len();
        search_from = paren + 1 + "transport.events, ".len();
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use specta_typescript::Typescript;

    use super::{
        adapt_query_bindings, adapt_rpc_bindings, build_specta_builder, build_transport_builder,
    };

    const BINDINGS_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../frontend/rpc/src/tauri-bindings.ts"
    );
    const RPC_BINDINGS_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../frontend/rpc/src/rpc-bindings.ts"
    );
    const QUERY_BINDINGS_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../frontend/query/src/query-bindings.ts"
    );

    fn exported_type<'a>(generated: &'a str, name: &str) -> &'a str {
        // Support both `export type Foo =` and generic `export type Foo<T> =`.
        let plain = format!("export type {name} =");
        let generic = format!("export type {name}<");
        let start = generated
            .find(&plain)
            .or_else(|| generated.find(&generic))
            .unwrap_or_else(|| panic!("expected generated declaration for {name}"));
        let rest = &generated[start..];
        let end = rest.find("\nexport ").unwrap_or(rest.len());
        &rest[..end]
    }

    fn assert_contains_all(declaration: &str, name: &str, expected: &[&str]) {
        for needle in expected {
            assert!(
                declaration.contains(needle),
                "expected {name} to contain {needle:?}, got:\n{declaration}"
            );
        }
    }

    #[test]
    fn protocol_event_names_match_desktop_names() {
        use nyanpasu_rpc_protocol::EventName;
        use tauri_specta::Event as _;

        use crate::{adapters::events, ipc, window};

        let desktop_names = [
            (
                EventName::AppUpdateStateChanged,
                events::AppUpdateStateChanged::NAME,
            ),
            (
                EventName::AppUpdateSummaryChanged,
                events::AppUpdateSummaryChanged::NAME,
            ),
            (EventName::ClashWs, events::ClashWsEvent::NAME),
            (
                EventName::CoreLogsChanged,
                events::CoreLogsChangedEvent::NAME,
            ),
            (
                EventName::ConfigurationStatusChanged,
                ipc::ConfigurationStatusChanged::NAME,
            ),
            (
                EventName::CoreStatusChanged,
                events::CoreStatusChangedEvent::NAME,
            ),
            (
                EventName::SchemeRequestReceived,
                ipc::system::os::SchemeRequestReceivedEvent::NAME,
            ),
            (
                EventName::ServiceStatusChanged,
                events::ServiceStatusChangedEvent::NAME,
            ),
            (
                EventName::StorageValueChanged,
                events::StorageValueChangedEvent::NAME,
            ),
            (EventName::StorageResync, events::StorageResyncEvent::NAME),
            (EventName::WindowMessage, window::WindowMessageEvent::NAME),
            (EventName::StateChanged, events::StateChangedEvent::NAME),
            (
                EventName::ConfigurationStatusSummaryChanged,
                ipc::ConfigurationStatusSummaryChanged::NAME,
            ),
            (
                EventName::CoreStatusSummaryChanged,
                events::CoreStatusSummaryChangedEvent::NAME,
            ),
            (
                EventName::ServiceStatusSummaryChanged,
                events::ServiceStatusSummaryChangedEvent::NAME,
            ),
            (
                EventName::CoreLogsSummaryChanged,
                events::CoreLogsSummaryChangedEvent::NAME,
            ),
            (
                EventName::SyncStatusChanged,
                events::SyncStatusChanged::NAME,
            ),
        ];
        assert_eq!(EventName::ALL.len(), desktop_names.len());
        for (event, (expected_event, desktop_name)) in EventName::ALL.into_iter().zip(desktop_names)
        {
            assert_eq!(event, expected_event);
            assert_eq!(event.as_str(), desktop_name);
        }
    }

    #[test]
    fn tray_icon_wire_mode_matches_existing_schema() {
        use nyanpasu_rpc_protocol::values::TrayIcon;
        let expected = ["normal", "tun", "system_proxy"];
        assert_eq!(TrayIcon::default().as_str(), expected[0]);
        assert_eq!(TrayIcon::all_supported().len(), expected.len());
        for (mode, expected) in TrayIcon::all_supported().iter().zip(expected) {
            assert_eq!(mode.as_str(), expected);
            assert_eq!(mode.to_string(), expected);
            assert_eq!(<&str>::from(*mode), expected);
            assert_eq!(<&str>::from(mode), expected);
            let value = serde_json::to_value(mode).unwrap();
            assert_eq!(value, expected);
            assert_eq!(
                serde_json::from_value::<TrayIcon>(value).unwrap().as_str(),
                expected
            );
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("icons.ts");
        let (_, builder) = build_specta_builder();
        builder.export(Typescript::default(), &path).unwrap();
        let generated = std::fs::read_to_string(path).unwrap();
        let declaration = exported_type(&generated, "TrayIcon");
        assert_eq!(
            declaration.lines().next().unwrap(),
            "export type TrayIcon = \"normal\" | \"tun\" | \"system_proxy\";"
        );
    }

    #[test]
    fn schema_collector_accepts_neutral_external_commands() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("neutral.ts");
        tauri_specta::Builder::<tauri::Wry>::new()
            .commands(schema_commands![
                nyanpasu_rpc::commands::profile::store::get_profiles,
                nyanpasu_rpc::commands::logs::app::open_log_session,
                nyanpasu_rpc::commands::system::icons::is_tray_icon_set,
            ])
            .dangerously_cast_bigints_to_number()
            .export(Typescript::default(), &path)
            .unwrap();
        let generated = std::fs::read_to_string(path).unwrap();
        for name in ["getProfiles", "openLogSession", "isTrayIconSet"] {
            assert!(generated.contains(name), "missing external schema: {name}");
        }
        for dependency in [
            "RpcState",
            "RpcCaller",
            "RpcDescribeContext",
            "HostIconFiles",
            "PathResolver",
        ] {
            assert!(
                !generated.contains(dependency),
                "injected dependency leaked: {dependency}"
            );
        }
    }

    /// The macro declaration is the source of truth for query/mutation; the hand-listed Specta
    /// groups that become TanStack queries and mutations must agree with it for every command.
    #[test]
    fn query_mutation_split_agrees_with_the_declared_effect() {
        use nyanpasu_rpc_protocol::{CommandEffect, permission::RESOURCES};

        let (query_bindings, _) = build_specta_builder();
        let (_, rest) = query_bindings.split_once("export const queries =").unwrap();
        let (queries, mutations) = rest.split_once("export const mutations =").unwrap();
        for command in RESOURCES.iter().filter(|resource| resource.is_command()) {
            let mut camel = String::new();
            let mut upper = false;
            for character in command.name.chars() {
                if character == '_' {
                    upper = true;
                } else if upper {
                    camel.extend(character.to_uppercase());
                    upper = false;
                } else {
                    camel.push(character);
                }
            }
            let key = format!("{camel}:");
            let (declared, other) = match command.effect {
                CommandEffect::Query => (queries, mutations),
                CommandEffect::Mutation => (mutations, queries),
            };
            assert!(
                declared.contains(&key),
                "{} is declared {:?} but is not in the matching Specta group",
                command.name,
                command.effect
            );
            assert!(
                !other.contains(&key),
                "{} is declared {:?} but is also in the other Specta group",
                command.name,
                command.effect
            );
        }
    }

    #[test]
    fn configuration_status_is_exported_as_a_query() {
        let (query_bindings, _) = build_specta_builder();
        let (queries, mutations) = query_bindings.split_once("export const mutations").unwrap();
        assert!(queries.contains("getConfigurationStatus"));
        assert!(!mutations.contains("getConfigurationStatus"));
    }

    /// Regenerates the committed TS bindings in place (same path and header as
    /// the debug-run export in lib.rs), then asserts every profile domain type
    /// exports as a named TS type. CI enforces freshness via
    /// `git diff --exit-code` after `pnpm test` (ci.yml test_unit job).
    #[test]
    fn export_typescript_bindings() {
        build_transport_builder()
            .export(
                Typescript::default().header("/* oxlint-disable */\n// @ts-nocheck"),
                BINDINGS_PATH,
            )
            .expect("failed to export Tauri transport bindings");
        let (query_bindings, builder) = build_specta_builder();
        builder
            .export(
                Typescript::default().header("/* oxlint-disable */\n// @ts-nocheck"),
                RPC_BINDINGS_PATH,
            )
            .expect("failed to export RPC schema bindings");
        adapt_rpc_bindings(RPC_BINDINGS_PATH).expect("failed to generate RPC bindings");
        adapt_query_bindings(QUERY_BINDINGS_PATH, &query_bindings)
            .expect("failed to generate query bindings");

        let prettier_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../node_modules/.bin")
            .join(if cfg!(target_os = "windows") {
                "prettier.cmd"
            } else {
                "prettier"
            });
        let mut prettier = if cfg!(target_os = "windows") {
            let mut command = std::process::Command::new("cmd");
            command.arg("/C").arg(prettier_path);
            command
        } else {
            std::process::Command::new(prettier_path)
        };
        let status = prettier
            .args([
                "--write",
                BINDINGS_PATH,
                RPC_BINDINGS_PATH,
                QUERY_BINDINGS_PATH,
            ])
            .status()
            .expect("failed to spawn workspace Prettier binary");
        assert!(status.success(), "prettier --write failed on bindings.ts");

        let transport_generated =
            std::fs::read_to_string(BINDINGS_PATH).expect("bindings.ts must exist after export");
        let generated = std::fs::read_to_string(RPC_BINDINGS_PATH)
            .expect("rpc-bindings.ts must exist after export");
        let query_generated = std::fs::read_to_string(QUERY_BINDINGS_PATH)
            .expect("query-bindings.ts must exist after export");
        assert!(transport_generated.contains("invoke as __TAURI_INVOKE"));
        assert!(transport_generated.contains("callRpc:"));
        assert!(!transport_generated.contains("getProfiles:"));
        assert!(generated.contains("createRpcClient"));
        assert!(generated.contains("transport.commands.invoke"));
        assert!(!generated.contains("__TAURI_INVOKE"));
        assert!(!generated.contains("export const queries ="));
        assert!(!generated.contains("export const mutations ="));
        let (_, queries) = query_generated.split_once("const queries =").unwrap();
        let (queries, mutations) = queries.split_once("const mutations =").unwrap();
        assert!(!transport_generated.contains("subscribeClashConnectionDetails:"));
        assert!(!transport_generated.contains("unsubscribeClashConnectionDetails:"));
        assert!(!queries.contains("subscribeClashConnectionDetails:"));
        assert!(!queries.contains("unsubscribeClashConnectionDetails:"));
        assert!(mutations.contains("subscribeClashConnectionDetails:"));
        assert!(mutations.contains("unsubscribeClashConnectionDetails:"));
        // The launch URL is a credential: a query would keep it in the client cache.
        assert!(!queries.contains("createHttpLaunchUrl:"));
        assert!(mutations.contains("createHttpLaunchUrl:"));
        assert!(!queries.contains("setRemoteAccess:"));
        assert!(mutations.contains("setRemoteAccess:"));
        assert!(queries.contains("getHttpServerStatus:"));
        assert!(queries.contains("getAppUpdateState:"));
        // A probe downloads but writes nothing.
        assert!(queries.contains("probeProfileUrl:"));
        for name in [
            "checkAppUpdate:",
            "downloadAppUpdate:",
            "cancelAppUpdateDownload:",
            "installAppUpdate:",
            "discardAppUpdatePackage:",
        ] {
            assert!(!queries.contains(name), "{name} changes updater state");
            assert!(mutations.contains(name));
        }
        assert!(generated.contains("appUpdateStateChanged:"));
        assert!(generated.contains("stateChangedEvent:"));
        assert!(!generated.contains("checkUpdate:"));
        assert!(!generated.contains("installUpdate:"));
        assert!(!generated.contains("__TAURI_EVENT"));
        assert!(generated.contains("import type { Channel }"));
        assert!(!generated.contains("import { Channel }"));
        assert!(!generated.contains("new Channel"));
        // PR-3 T08: the profile IPC surface now speaks the domain types, so the
        // legacy `Profiles` / `RemoteProfileOptions` exports are retired. The
        // domain document/options types are asserted via their specta remote
        // shadow names (`ProfileDocument` / `ProfileRemoteOptions`).
        for name in [
            "ProfileDocument",
            "ProfileItem",
            "ProfileDefinition",
            "ConfigDefinition",
            "FileConfig",
            "CompositionConfig",
            "TransformDefinition",
            "OverlayTransform",
            "ScriptTransform",
            "ScriptRuntime",
            "ProfileSource",
            "LocalBinding",
            "ExternalMode",
            "MaterializedFile",
            "ProfileRemoteOptions",
            "SubscriptionInfo",
            "ProfileSubscriptionInfo",
            "ProfileMetadataPatch",
            "RemoteProfileOptionsPatch",
            "ProfileValidationError",
            "MutationOutcome",
            "RpcMutationOutcome",
            "RpcUnitMutationOutcome",
            "RpcProfileMutationOutcome",
            "RpcFolderMutationOutcome",
            "PublicMutationOutcome",
            "PublicDegradation",
            "PublicDegradationReason",
            "RpcAppUpdateOutcome",
            "RpcNetworkAutomationStatus",
            "NetworkAutomationSummary",
            "NetworkAutomationApplicationSummary",
            "MechanismOutcomeKinds",
            "MechanismOutcomeKind",
            "AppUpdateSummary",
            "AppUpdateReleaseSummary",
            "TransparentProxyStatusSummary",
            "UpdaterProgressSummary",
            "UpdaterProgressState",
            "DownloadProgressSummary",
            "DownloadProgressState",
            "CoreAgentSkillSummary",
            "Degradation",
            "DegradationPhase",
            "DegradationReason",
            "NyanpasuAppConfig",
            "ApplicationPreferencesPatch",
            "SystemSettingsPatch",
            "ClashConfig",
            "ClashSettingsPatch",
            "ClashSystemPatch",
            "ClashOverridesSettingsPatch",
            "ClashAccessPatch",
            "ClashApiConfig",
        ] {
            assert!(
                generated.contains(&format!("export type {name}"))
                    || generated.contains(&format!("export interface {name}")),
                "expected named TS export for {name}"
            );
        }

        // Each request contract advertises only the fields of its privilege class. The unrestricted
        // patches stay internal and are not exported at all.
        for internal in [
            "NyanpasuAppConfigPatch",
            "ClashConfigPatch",
            "ClashGuardOverridesPatch",
        ] {
            assert!(!generated.contains(internal), "{internal} is exported");
        }
        let advertised = |name: &str, required: &[&str], forbidden: &[&str]| {
            for suffix in ["Serialize", "Deserialize"] {
                let declaration = exported_type(&generated, &format!("{name}_{suffix}"));
                // A property key, not a word in a doc comment: `field?:`, `field:` or `'field'`.
                for field in forbidden {
                    for key in [
                        format!("{field}?:"),
                        format!("{field}:"),
                        format!("'{field}'"),
                    ] {
                        assert!(
                            !declaration.contains(&key),
                            "{name} advertises {field}:\n{declaration}"
                        );
                    }
                }
                assert_contains_all(declaration, name, required);
            }
        };
        advertised(
            "ApplicationPreferencesPatch",
            &[
                "proxy_layout_column",
                // Ordinary null is a no-op and the field is optional, so both are in the contract.
                "theme_color?: string | null",
                // The clearable preference accepts an explicit null.
                "release_channel?: ReleaseChannel | null",
            ],
            &[
                "hotkeys",
                "enable_service_mode",
                "remote_access",
                "enable_auto_launch",
                "enable_system_proxy",
                "enable_proxy_guard",
                "system_proxy_bypass",
                "proxy_guard_interval",
                "pac_url",
            ],
        );
        advertised(
            "SystemSettingsPatch",
            &[
                "enable_auto_launch",
                "enable_system_proxy",
                "enable_proxy_guard",
                "system_proxy_bypass",
                "proxy_guard_interval",
                "pac_url?: string | null",
            ],
            &[
                "hotkeys",
                "remote_access",
                "proxy_layout_column",
                "release_channel",
            ],
        );
        advertised(
            "ClashSettingsPatch",
            &["expand_include_all", "break_connection"],
            &[
                "enable_tun_mode",
                "external_controller",
                "mixed_port",
                "overrides",
            ],
        );
        advertised(
            "ClashSystemPatch",
            &[
                "enable_tun_mode",
                "external_controller",
                "mixed_port",
                "web_ui_list",
            ],
            &["expand_include_all", "break_connection", "overrides"],
        );
        advertised(
            "ClashOverridesSettingsPatch",
            &["'log-level'", "ipv6"],
            &["mode", "allow-lan", "secret"],
        );
        advertised(
            "ClashAccessPatch",
            &["'allow-lan'", "secret"],
            &["mode", "log-level", "ipv6"],
        );

        assert!(
            !generated.contains("export type RebuildOutcome")
                && !generated.contains("export interface RebuildOutcome"),
            "old RebuildOutcome wire must be removed from bindings"
        );
        assert!(
            !generated.contains("export type CommitOutcome")
                && !generated.contains("export interface CommitOutcome"),
            "old CommitOutcome wire must be removed from bindings"
        );
        assert!(
            !generated.contains("export type PatchRuntimeConfig"),
            "the whitelisted overrides DTO is replaced by the per-class override patches"
        );
        assert!(
            !generated.contains("export type IVerge") && !generated.contains("export type Legacy"),
            "no legacy verge DTO may stay on the wire"
        );

        // The plain `ClashConfig` name belongs to the typed persistent config,
        // not to the clash-API `/configs` DTO.
        let clash_config = exported_type(&generated, "ClashConfig");
        assert_contains_all(
            clash_config,
            "ClashConfig",
            &["overrides: ClashGuardOverrides", "mixed_port: PortStrategy"],
        );
        // The composite clash fields take nested patches, so edits of sibling
        // sub-fields merge in the actor instead of replacing each other.
        for (name, nested) in [
            (
                "ClashSystemPatch_Deserialize",
                &[
                    "mixed_port?: PortStrategyPatch_Deserialize",
                    "external_controller?: ExternalControllerStrategyPatch_Deserialize",
                ][..],
            ),
            (
                "ClashSettingsPatch_Deserialize",
                &["break_connection?: BreakConnectionStrategyPatch_Deserialize"][..],
            ),
        ] {
            assert_contains_all(exported_type(&generated, name), name, nested);
        }

        for phase in ["Deserialize", "Serialize"] {
            let name = format!("ConfigDefinition_{phase}");
            let declaration = exported_type(&generated, &name);
            assert_contains_all(
                declaration,
                &name,
                &[
                    "type: 'file'",
                    "type: 'composition'",
                    "source: ProfileSource_",
                    "extend_proxies_from?",
                ],
            );
            assert!(
                !declaration.contains("file: {") && !declaration.contains("composition: {"),
                "{name} must not contain newtype wrapper keys:\n{declaration}"
            );

            let name = format!("TransformDefinition_{phase}");
            let declaration = exported_type(&generated, &name);
            assert_contains_all(
                declaration,
                &name,
                &[
                    "type: 'overlay'",
                    "type: 'script'",
                    "source: ProfileSource_",
                    "runtime: ScriptRuntime",
                ],
            );
            assert!(
                !declaration.contains("overlay: {") && !declaration.contains("script: {"),
                "{name} must not contain newtype wrapper keys:\n{declaration}"
            );

            let name = format!("ProfileSource_{phase}");
            let declaration = exported_type(&generated, &name);
            assert_contains_all(
                declaration,
                &name,
                &[
                    "type: 'local'",
                    "type: 'remote'",
                    "file: ManagedProfilePath",
                    "url: string",
                ],
            );
            assert!(
                !declaration.contains("materialized:"),
                "{name} must expose flattened materialized fields:\n{declaration}"
            );

            let name = format!("LocalBinding_{phase}");
            let declaration = exported_type(&generated, &name);
            assert_contains_all(
                declaration,
                &name,
                &[
                    "type: 'managed'",
                    "type: 'external'",
                    "file: ManagedProfilePath",
                    "target: ExternalProfilePath",
                    "mode: ExternalMode",
                ],
            );
            assert!(
                !declaration.contains("materialized:"),
                "{name} must expose flattened materialized fields:\n{declaration}"
            );
        }

        // S08 freeze: the named-export loop above only proves these types exist;
        // pin the actual generated shapes so a wire-format drift breaks CI.
        // Substrings are copied verbatim from the generated product (prettier
        // emits single-quoted tags and one union variant per line).
        let mutation_outcome = exported_type(&generated, "MutationOutcome");
        assert_contains_all(
            mutation_outcome,
            "MutationOutcome",
            &[
                "status: 'committed'",
                "status: 'committed_degraded'",
                "value: T",
                "degradations:",
            ],
        );
        assert!(
            !mutation_outcome.contains("status: 'ok'")
                && !mutation_outcome.contains("status: 'degraded'"),
            "MutationOutcome must not retain legacy RebuildOutcome status tags:\n{mutation_outcome}"
        );

        let degradation = exported_type(&generated, "Degradation");
        assert_contains_all(
            degradation,
            "Degradation",
            &[
                "phase: DegradationPhase",
                "reason: DegradationReason",
                "message:",
                "retryable:",
            ],
        );

        let reason = exported_type(&generated, "DegradationReason");
        assert_contains_all(
            reason,
            "DegradationReason",
            &["code: 'runtime_deferred'", "code: 'cleanup_deferred'"],
        );

        let phase = exported_type(&generated, "DegradationPhase");
        assert_contains_all(
            phase,
            "DegradationPhase",
            &[
                "'profile_materialization'",
                "'runtime_build'",
                "'runtime_apply'",
                "'system_effect'",
                "'ui_effect'",
            ],
        );
        assert!(
            !generated.contains("legacy_mirror"),
            "nothing constructs the legacy_mirror phase, so it may not return to the wire"
        );

        let (commands_section, _) = generated
            .split_once("/* Types */")
            .expect("expected generated Types section");
        // create/import must return the concrete wrapper carrying ProfileId.
        assert!(
            commands_section.contains("RpcProfileMutationOutcome_Serialize"),
            "createProfile/importProfile must return RpcProfileMutationOutcome_Serialize"
        );
        // Folder creation must return the concrete wrapper carrying FolderId.
        assert!(
            commands_section.contains("RpcFolderMutationOutcome_Serialize"),
            "folder mutations must return RpcFolderMutationOutcome_Serialize"
        );
        // Unit mutations must return the concrete wrapper carrying null.
        assert!(
            commands_section.contains("RpcUnitMutationOutcome_Serialize"),
            "unit profile mutations must return RpcUnitMutationOutcome_Serialize"
        );
        let profile_outcome = exported_type(&generated, "RpcProfileMutationOutcome_Serialize");
        assert!(
            profile_outcome.contains("RpcMutationOutcome_Serialize<ProfileId>"),
            "expected RpcProfileMutationOutcome_Serialize to instantiate ProfileId, got: {profile_outcome}"
        );
        let folder_outcome = exported_type(&generated, "RpcFolderMutationOutcome_Serialize");
        assert!(
            folder_outcome.contains("RpcMutationOutcome_Serialize<FolderId>"),
            "expected RpcFolderMutationOutcome_Serialize to instantiate FolderId, got: {folder_outcome}"
        );
        let unit_outcome = exported_type(&generated, "RpcUnitMutationOutcome_Serialize");
        assert!(
            unit_outcome.contains("RpcMutationOutcome_Serialize<null>"),
            "expected RpcUnitMutationOutcome_Serialize to instantiate null, got: {unit_outcome}"
        );
        // Generic instantiation must never leave unbound `<T>` in command signatures.
        assert!(
            !commands_section.contains("RpcMutationOutcome_Serialize<T>")
                && !commands_section.contains("RpcMutationOutcome<T>"),
            "command return types must not leak unbound generic <T>"
        );
        // App update mutations must expose safe/full outcomes rather than full snapshot only.
        assert!(
            commands_section.contains("RpcAppUpdateOutcome"),
            "app update mutations must return RpcAppUpdateOutcome"
        );
        // Bounded ClashCore wire enum must remain preserved in public error variants.
        assert_contains_all(
            exported_type(&generated, "PublicInstallCoreBinaryKind_Serialize"),
            "PublicInstallCoreBinaryKind_Serialize",
            &["core: ClashCore_Serialize"],
        );

        // A2: the connection-detail Channel payload wraps clash-api's
        // strongly typed Connection with the two rate fields, and unknown
        // Mihomo fields are named (`_extra`) rather than flattened, so the
        // detail dialog can tell them apart from known fields (A0/A2).
        let clash_connection = exported_type(&generated, "ClashConnection_Serialize");
        assert_contains_all(
            clash_connection,
            "ClashConnection_Serialize",
            &["downloadSpeed", "uploadSpeed", "Connection_Serialize"],
        );
        let connection = exported_type(&generated, "Connection_Serialize");
        assert_contains_all(
            connection,
            "Connection_Serialize",
            &[
                "chains: string[]",
                "metadata: ConnectionMetadata_Serialize",
                "_extra:",
            ],
        );

        // Safe profile listing and sync summaries keep flat discriminated shapes
        // and public scheduler ordering fields without leaking raw URLs or text.
        for suffix in ["Serialize", "Deserialize"] {
            let listing_config_name = format!("ProfileListingConfig_{suffix}");
            let listing_config = exported_type(&generated, &listing_config_name);
            let expected_source = format!("source: ProfileListingSource_{suffix}");
            assert_contains_all(
                listing_config,
                &listing_config_name,
                &[
                    "type: 'file'",
                    &expected_source,
                    "type: 'composition'",
                    "transforms",
                ],
            );
            for wrapper in ["file:", "file?:", "composition:", "composition?:"] {
                assert!(
                    !listing_config.contains(wrapper),
                    "{listing_config_name} must serialize flat, not wrapped under {wrapper}:\n{listing_config}"
                );
            }
            let profiles_listing_name = format!("ProfilesListing_{suffix}");
            let profiles_listing = exported_type(&generated, &profiles_listing_name);
            let expected_items = format!("items: ProfileListingItem_{suffix}[]");
            assert_contains_all(
                profiles_listing,
                &profiles_listing_name,
                &[
                    "current: ProfileId | null",
                    "proxy_group_layouts:",
                    &expected_items,
                ],
            );
        }
        assert_contains_all(
            exported_type(&generated, "ProfileSyncSummary"),
            "ProfileSyncSummary",
            &[
                "scheduled: boolean",
                "active: SyncRunSummary[]",
                "has_registration_error: boolean",
            ],
        );
        assert_contains_all(
            exported_type(&generated, "SyncRunSummary"),
            "SyncRunSummary",
            &[
                "id: string",
                "admission_sequence: string",
                "state: SyncRunStateSummary",
            ],
        );
        assert_contains_all(
            exported_type(&generated, "SyncRunPageSummary"),
            "SyncRunPageSummary",
            &["items: SyncRunSummary[]", "next: RunCursorDto | null"],
        );
        assert_contains_all(
            exported_type(&generated, "ProfileSyncReadError"),
            "ProfileSyncReadError",
            &[
                "kind: 'unknown_profile'",
                "kind: 'not_remote'",
                "kind: 'invalid_cursor'",
                "kind: 'unavailable'",
            ],
        );
        assert_contains_all(
            exported_type(&generated, "CoreLogStatusSummary"),
            "CoreLogStatusSummary",
            &[
                "generation: string",
                "version: number",
                "has_error: boolean",
            ],
        );
        assert_contains_all(
            exported_type(&generated, "CoreLogRowSummary"),
            "CoreLogRowSummary",
            &[
                "id: CoreLogCursor",
                "received_at: number",
                "level: CoreLogLevel",
                "has_payload: boolean",
                "truncated: boolean",
            ],
        );
        assert_contains_all(
            exported_type(&generated, "CoreLogPageSummary"),
            "CoreLogPageSummary",
            &["rows: CoreLogRowSummary[]", "status: CoreLogStatusSummary"],
        );
        assert_contains_all(
            exported_type(&generated, "LogRowSummary"),
            "LogRowSummary",
            &[
                "id: string",
                "level: Level",
                "has_target: boolean",
                "unparsed: boolean",
                "truncated: boolean",
            ],
        );
        assert_contains_all(
            exported_type(&generated, "LogPageSummary"),
            "LogPageSummary",
            &["rows: LogRowSummary[]", "file: string", "building: boolean"],
        );
        assert_contains_all(
            exported_type(&generated, "CoreLogReadError"),
            "CoreLogReadError",
            &[
                "kind: 'invalid_request'",
                "kind: 'cursor_expired'",
                "kind: 'record_gone'",
                "kind: 'too_large'",
                "kind: 'unavailable'",
            ],
        );
        assert_contains_all(
            exported_type(&generated, "ClashReadError"),
            "ClashReadError",
            &[
                "kind: 'settings_unavailable'",
                "kind: 'version_unavailable'",
                "kind: 'rules_unavailable'",
                "kind: 'rule_providers_unavailable'",
                "kind: 'proxy_providers_unavailable'",
                "kind: 'streams_unavailable'",
            ],
        );
    }

    #[test]
    fn specta_transparent_and_inline_asymmetric_generic_regression() {
        #[derive(serde::Serialize, serde::Deserialize, specta::Type)]
        #[serde(rename_all = "snake_case")]
        #[allow(dead_code)]
        enum ProbeCore {
            #[serde(alias = "alias-only")]
            Mihomo,
            ClashRs,
        }

        #[derive(serde::Serialize, serde::Deserialize, specta::Type)]
        #[serde(rename_all = "snake_case")]
        struct ProbePayload<T> {
            value: T,
            core: ProbeCore,
        }

        #[derive(serde::Serialize, specta::Type)]
        #[serde(untagged)]
        #[allow(dead_code)]
        enum ProbeOutcome<T> {
            Full(ProbePayload<T>),
            Public { code: &'static str, core: ProbeCore },
        }

        #[derive(serde::Serialize, specta::Type)]
        #[serde(transparent)]
        struct ProbeConcreteU32(ProbeOutcome<u32>);

        #[derive(serde::Serialize, specta::Type)]
        #[serde(transparent)]
        struct ProbeConcreteString(ProbeOutcome<String>);

        #[derive(serde::Serialize, specta::Type)]
        #[serde(untagged)]
        #[specta(inline)]
        #[allow(dead_code)]
        enum ProbeInlineOutcome<T> {
            Full(#[specta(inline)] ProbePayload<T>),
            Public { code: &'static str, core: ProbeCore },
        }

        #[tauri::command]
        #[specta::specta]
        fn probe_u32_cmd() -> ProbeConcreteU32 {
            unreachable!("type-export regression only")
        }

        #[tauri::command]
        #[specta::specta]
        fn probe_string_cmd() -> ProbeConcreteString {
            unreachable!("type-export regression only")
        }

        #[tauri::command]
        #[specta::specta]
        fn probe_inline_u32_cmd() -> ProbeInlineOutcome<u32> {
            unreachable!("type-export regression only")
        }

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("probe.ts");
        tauri_specta::Builder::<tauri::Wry>::new()
            .commands(tauri_specta::collect_commands![
                probe_u32_cmd,
                probe_string_cmd,
                probe_inline_u32_cmd,
            ])
            .export(Typescript::default(), &path)
            .expect("original Typescript exporter handles transparent + inline phased generics");

        let generated = std::fs::read_to_string(&path).expect("read exported probe.ts");
        let (commands_section, _) = generated
            .split_once("/* Types */")
            .expect("expected Types section");
        assert!(
            !commands_section.contains("<T>") && !commands_section.contains("any"),
            "command signatures must stay concrete without <T> or any:\n{commands_section}"
        );
        let concrete_u32 = exported_type(&generated, "ProbeConcreteU32_Serialize");
        assert!(
            concrete_u32.contains("ProbeOutcome_Serialize<number>"),
            "ProbeConcreteU32_Serialize must bind number:\n{concrete_u32}"
        );
        let concrete_str = exported_type(&generated, "ProbeConcreteString_Serialize");
        assert!(
            concrete_str.contains("ProbeOutcome_Serialize<string>"),
            "ProbeConcreteString_Serialize must bind string:\n{concrete_str}"
        );
        assert!(
            commands_section.contains("value: number")
                && commands_section.contains("ProbeCore_Serialize"),
            "ProbeInlineOutcome<u32> must inline concrete value and bounded ProbeCore_Serialize:\n{commands_section}"
        );
        let core_serialize = exported_type(&generated, "ProbeCore_Serialize");
        assert!(
            !core_serialize.contains("alias-only")
                && core_serialize.contains("mihomo")
                && core_serialize.contains("clash_rs"),
            "serialize-only ProbeCore_Serialize must keep canonical variants without deserialize alias:\n{core_serialize}"
        );
    }
}
