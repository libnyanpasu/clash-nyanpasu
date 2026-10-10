//! Single source of truth for the tauri-specta builder.
//! Shared by `lib.rs` (runtime registration + debug export) and the
//! `export_typescript_bindings` test (CI freshness).

use tauri_specta::{collect_commands, collect_events};
use tauri_specta_query::{CommandSet, TanstackQueryFramework};

use crate::{core, ipc, unified_rpc, window};

pub(crate) fn build_transport_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        // The only direct Tauri command is the UnifiedRpc transport entrypoint.
        // Application operations, including Channel subscription control, belong
        // in the schema builder below and dispatch through call_rpc.
        .commands(collect_commands![unified_rpc::call_rpc])
        .events(collect_events![
            crate::client::app_update::AppUpdateStateChanged,
            core::logs::CoreLogsChanged,
            core::clash::ws::ClashWsEvent,
            window::WindowMessageEvent,
            core::storage::StorageValueChangedEvent,
            ipc::SchemeRequestReceivedEvent,
            core::actor_v2::CoreStatusChangedEvent,
            ipc::ConfigurationStatusChanged,
            core::actor_v2::ServiceStatusChangedEvent
        ])
        .dangerously_cast_bigints_to_number()
}

pub(crate) fn build_specta_builder() -> (String, tauri_specta::Builder<tauri::Wry>) {
    let command_set = CommandSet::<tauri::Wry>::new(
        collect_commands![
            ipc::get_debug_http_status,
            // Read-only commands
            ipc::read_clipboard_text,
            ipc::query_core_logs,
            ipc::get_core_log,
            ipc::get_core_log_status,
            ipc::list_log_files,
            ipc::get_sys_proxy,
            ipc::get_clash_info,
            ipc::get_runtime_config,
            ipc::get_runtime_yaml,
            ipc::get_runtime_exists,
            ipc::inspect_runtime,
            ipc::inspect_applied_runtime,
            ipc::inspect_runtime_node,
            ipc::get_postprocessing_output,
            ipc::clash_api_get_proxy_delay,
            ipc::clash_api_get_configs,
            ipc::clash_api_get_version,
            ipc::clash_api_get_rules,
            ipc::clash_api_get_providers_rules,
            ipc::clash_api_get_group_delay,
            ipc::clash_api_get_providers_proxies,
            ipc::fetch_latest_core_versions,
            ipc::inspect_updater,
            ipc::get_core_version,
            ipc::get_app_config,
            ipc::get_clash_config,
            ipc::get_hotkey_functions,
            ipc::get_profiles,
            ipc::get_profile_sync_status,
            ipc::get_profile_sync_runs,
            ipc::get_profile_sync_logs,
            ipc::read_profile_file,
            ipc::get_custom_app_dir,
            ipc::service::status_service,
            ipc::is_portable,
            ipc::get_proxies,
            ipc::collect_envs,
            ipc::get_cached_icon,
            ipc::get_tray_icon,
            ipc::is_tray_icon_set,
            ipc::get_core_status,
            ipc::url_delay_test,
            ipc::get_ipsb_asn,
            ipc::is_appimage,
            ipc::get_service_install_prompt,
            ipc::get_storage_item,
            ipc::get_all_storage_items,
            ipc::get_hotkeys,
            ipc::get_core_dir,
            ipc::get_clash_ws_snapshot,
            ipc::get_traffic_summary,
            ipc::query_traffic_report,
            ipc::query_traffic_usage,
            ipc::query_traffic_usage_by_keys,
            ipc::query_traffic_closed_connections,
            ipc::query_traffic_active_connection_ids,
            ipc::get_app_update_state,
            ipc::get_release_channel,
            ipc::get_system_accent_color,
            ipc::get_transparent_proxy_status,
        ],
        collect_commands![
            ipc::write_clipboard_text,
            ipc::show_native_notification,
            ipc::show_native_message_dialog,
            ipc::ask_native_dialog,
            ipc::open_native_file_dialog,
            ipc::set_debug_http_enabled,
            ipc::get_configuration_status,
            ipc::retry_configuration_runtime,
            ipc::retry_configuration_effect,
            ipc::set_release_channel,
            ipc::check_app_update,
            ipc::download_app_update,
            ipc::cancel_app_update_download,
            ipc::install_app_update,
            ipc::discard_app_update_package,
            // Creating/releasing a detail receiver changes stream demand.
            ipc::subscribe_clash_connection_details,
            ipc::unsubscribe_clash_connection_details,
            // Side-effecting commands
            ipc::probe_direct_egress,
            ipc::clear_core_logs,
            ipc::open_log_session,
            ipc::query_logs,
            ipc::close_log_session,
            ipc::report_frontend_events,
            ipc::flush_system_dns_cache,
            ipc::open_app_config_dir,
            ipc::open_app_data_dir,
            ipc::open_backups_dir,
            ipc::create_config_backup,
            ipc::open_logs_dir,
            ipc::open_web_url,
            ipc::open_core_dir,
            ipc::restart_sidecar,
            ipc::patch_app_config,
            ipc::patch_clash_config,
            ipc::patch_runtime_overrides,
            ipc::change_clash_core,
            ipc::clash_api_delete_connections,
            ipc::clash_api_update_providers_rules,
            ipc::uwp::invoke_uwp_tool,
            ipc::update_core,
            ipc::collect_logs,
            ipc::set_tray_icon_from_bytes,
            ipc::enhance_profiles,
            ipc::import_profile,
            ipc::take_pending_deep_links,
            ipc::create_profile,
            ipc::reorder_profile,
            ipc::reorder_profiles_by_list,
            ipc::update_profile,
            ipc::delete_profile,
            ipc::activate_profile,
            ipc::set_global_transforms,
            ipc::set_profile_valid_fields,
            ipc::patch_profile_metadata,
            ipc::patch_remote_profile_options,
            ipc::replace_profile_definition,
            ipc::view_profile,
            ipc::save_profile_file,
            ipc::set_custom_app_dir,
            ipc::service::install_service,
            ipc::service::uninstall_service,
            ipc::service::start_service,
            ipc::service::stop_service,
            ipc::service::restart_service,
            ipc::select_proxy,
            ipc::clear_proxy_fixed,
            ipc::update_proxy_provider,
            ipc::clash_api_healthcheck_proxy_provider,
            ipc::restart_application,
            ipc::set_tray_icon,
            ipc::open_that,
            ipc::set_storage_item,
            ipc::remove_storage_item,
            ipc::clear_storage,
            ipc::set_hotkeys,
            ipc::mutate_proxies,
            ipc::set_clash_ws_recording,
            ipc::clear_clash_ws_history,
            ipc::save_window_size_state,
            ipc::create_main_window,
            ipc::create_debug_tray_menu_window,
            ipc::create_editor_window,
            ipc::report_window_ready,
            ipc::copy_clash_env,
            ipc::quit_application,
        ],
    )
    .events(collect_events![
        crate::client::app_update::AppUpdateStateChanged,
        core::logs::CoreLogsChanged,
        core::clash::ws::ClashWsEvent,
        window::WindowMessageEvent,
        core::storage::StorageValueChangedEvent,
        ipc::SchemeRequestReceivedEvent,
        core::actor_v2::CoreStatusChangedEvent,
        ipc::ConfigurationStatusChanged,
        core::actor_v2::ServiceStatusChangedEvent
    ])
    // PR-3 T01: profile domain types, add-only. Commands referencing them
    // arrive with T08; explicit registration keeps them exported (and the
    // specta nested-tagged-enum risk probed) before any command exists.
    .typ::<nyanpasu_config::profile::Profiles>()
    .typ::<nyanpasu_config::profile::FileConfig>()
    .typ::<nyanpasu_config::profile::CompositionConfig>()
    .typ::<nyanpasu_config::profile::OverlayTransform>()
    .typ::<nyanpasu_config::profile::ScriptTransform>()
    .typ::<nyanpasu_config::profile::MaterializedFile>()
    .typ::<nyanpasu_config::profile::ProfileMetadataPatch>()
    .typ::<nyanpasu_config::profile::RemoteProfileOptionsPatch>()
    .typ::<nyanpasu_config::profile::ProfileValidationError>()
    .typ::<crate::client::StateChanged>()
    .build(TanstackQueryFramework::React);

    let (query_bindings, builder) = command_set;

    (query_bindings, builder.dangerously_cast_bigints_to_number())
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
    let events_and_separator = source[events_at..types_at].replacen(events_start, "", 1);
    let event_object_at = events_and_separator
        .find("export const events = {")
        .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing generated event object"))?;
    let mut events_and_separator = events_and_separator;
    events_and_separator.replace_range(
        event_object_at..event_object_at + "export const events = {".len(),
        "const events = {",
    );
    let commands_and_events = format!(
        "{prefix}{commands}{events_and_separator}\n  return {{ ...commands, events, listenMutation: (callback: (payload: StateChanged) => void) => transport.events.listenMutation((payload) => callback(payload as StateChanged)), listenResync: transport.events.listenResync.bind(transport.events), dispose: () => transport.events.dispose() }}\n}}\n\nexport type RpcClient = ReturnType<typeof createRpcClient>\n\n"
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
        assert!(!queries.contains("setDebugHttpEnabled:"));
        assert!(mutations.contains("setDebugHttpEnabled:"));
        assert!(queries.contains("getAppUpdateState:"));
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
            "Degradation",
            "DegradationPhase",
            "DegradationReason",
            "NyanpasuAppConfig",
            "NyanpasuAppConfigPatch",
            "ClashConfig",
            "ClashConfigPatch",
            "ClashGuardOverridesPatch",
            "ClashApiConfig",
        ] {
            assert!(
                generated.contains(&format!("export type {name}"))
                    || generated.contains(&format!("export interface {name}")),
                "expected named TS export for {name}"
            );
        }

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
            "the whitelisted overrides DTO is replaced by ClashGuardOverridesPatch"
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
        let clash_patch = exported_type(&generated, "ClashConfigPatch_Deserialize");
        assert_contains_all(
            clash_patch,
            "ClashConfigPatch_Deserialize",
            &[
                "mixed_port?: PortStrategyPatch_Deserialize",
                "external_controller?: ExternalControllerStrategyPatch_Deserialize",
                "break_connection?: BreakConnectionStrategyPatch_Deserialize",
            ],
        );

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
                "'runtime_check'",
                "'runtime_promote'",
                "'runtime_publish'",
                "'runtime_apply'",
                "'core_rollback'",
                "'system_effect'",
                "'ui_effect'",
            ],
        );
        assert!(
            !generated.contains("legacy_mirror"),
            "nothing constructs the legacy_mirror phase, so it may not return to the wire"
        );

        // create/import must return the instantiated generic carrying ProfileId.
        assert!(
            generated.contains("MutationOutcome<ProfileId>"),
            "createProfile/importProfile must return MutationOutcome<ProfileId>"
        );
        // Unit mutations must keep a stable MutationOutcome<()> shape (TS null).
        assert!(
            generated.contains("MutationOutcome<null>"),
            "unit profile mutations must return MutationOutcome<null>"
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
    }
}
