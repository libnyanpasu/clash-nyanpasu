//! Single source of truth for the tauri-specta builder.
//! Shared by `lib.rs` (runtime registration + debug export) and the
//! `export_typescript_bindings` test (CI freshness).

use tauri_specta::{collect_commands, collect_events};

use crate::{core, ipc, window};

pub(crate) fn build_specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            // common
            ipc::get_sys_proxy,
            ipc::flush_system_dns_cache,
            ipc::open_app_config_dir,
            ipc::open_app_data_dir,
            ipc::open_logs_dir,
            ipc::open_web_url,
            ipc::open_core_dir,
            // cmds::kill_sidecar,
            ipc::restart_sidecar,
            // clash
            ipc::get_clash_info,
            ipc::get_clash_logs,
            ipc::patch_clash_config,
            ipc::change_clash_core,
            ipc::get_runtime_config,
            ipc::get_runtime_yaml,
            ipc::get_runtime_exists,
            ipc::get_postprocessing_output,
            ipc::clash_api_get_proxy_delay,
            ipc::clash_api_get_configs,
            ipc::clash_api_delete_connections,
            ipc::clash_api_get_version,
            ipc::clash_api_get_rules,
            ipc::clash_api_get_providers_rules,
            ipc::clash_api_update_providers_rules,
            ipc::clash_api_get_group_delay,
            ipc::clash_api_get_providers_proxies,
            ipc::uwp::invoke_uwp_tool,
            // updater
            ipc::fetch_latest_core_versions,
            ipc::update_core,
            ipc::inspect_updater,
            ipc::get_core_version,
            // utils
            ipc::collect_logs,
            // verge
            ipc::get_verge_config,
            ipc::patch_verge_config,
            ipc::get_hotkey_functions,
            // cmds::update_hotkeys,
            // profile
            ipc::get_profiles,
            ipc::enhance_profiles,
            ipc::import_profile,
            ipc::get_pending_deep_link,
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
            ipc::read_profile_file,
            ipc::save_profile_file,
            ipc::get_custom_app_dir,
            ipc::set_custom_app_dir,
            // service mode
            ipc::service::status_service,
            ipc::service::install_service,
            ipc::service::uninstall_service,
            ipc::service::start_service,
            ipc::service::stop_service,
            ipc::service::restart_service,
            ipc::is_portable,
            // ipc::get_device_info,
            ipc::get_proxies,
            ipc::select_proxy,
            ipc::update_proxy_provider,
            ipc::restart_application,
            ipc::collect_envs,
            ipc::get_server_port,
            ipc::set_tray_icon,
            ipc::is_tray_icon_set,
            ipc::get_core_status,
            ipc::url_delay_test,
            ipc::get_ipsb_asn,
            ipc::open_that,
            ipc::is_appimage,
            ipc::get_service_install_prompt,
            ipc::cleanup_processes,
            ipc::get_storage_item,
            ipc::set_storage_item,
            ipc::remove_storage_item,
            ipc::get_all_storage_items,
            ipc::clear_storage,
            ipc::get_hotkeys,
            ipc::set_hotkeys,
            ipc::mutate_proxies,
            ipc::get_core_dir,
            // clash layer
            ipc::get_clash_ws_connections_state,
            ipc::get_clash_ws_snapshot,
            ipc::set_clash_ws_recording,
            ipc::clear_clash_ws_history,
            // updater layer
            ipc::check_update,
            // window management
            ipc::save_window_size_state,
            ipc::create_main_window,
            ipc::create_debug_tray_menu_window,
            ipc::create_editor_window,
            // tray actions
            ipc::copy_clash_env,
            ipc::quit_application,
            // color
            ipc::get_system_accent_color,
        ])
        .events(collect_events![
            core::clash::ClashConnectionsEvent,
            core::clash::ws::ClashWsEvent,
            window::WindowMessageEvent,
            window::WindowReadyEvent,
            core::storage::StorageValueChangedEvent,
            ipc::SchemeRequestReceivedEvent
        ])
        .dangerously_cast_bigints_to_number()
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
}

#[cfg(test)]
mod tests {
    use specta_typescript::Typescript;

    use super::build_specta_builder;

    const BINDINGS_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../frontend/interface/src/ipc/bindings.ts"
    );

    fn exported_type<'a>(generated: &'a str, name: &str) -> &'a str {
        let marker = format!("export type {name} =");
        let start = generated
            .find(&marker)
            .unwrap_or_else(|| panic!("expected generated declaration for {name}"));
        let rest = &generated[start..];
        let end = rest[marker.len()..]
            .find("\nexport ")
            .map(|offset| marker.len() + offset)
            .unwrap_or(rest.len());
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
        build_specta_builder()
            .export(
                Typescript::default().header("/* oxlint-disable */\n// @ts-nocheck"),
                BINDINGS_PATH,
            )
            .expect("failed to export typescript bindings");

        let npx = if cfg!(target_os = "windows") {
            "npx.cmd"
        } else {
            "npx"
        };
        let status = std::process::Command::new(npx)
            .args(["prettier", "--write", BINDINGS_PATH])
            .status()
            .expect("failed to spawn prettier");
        assert!(status.success(), "prettier --write failed on bindings.ts");

        let generated =
            std::fs::read_to_string(BINDINGS_PATH).expect("bindings.ts must exist after export");
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
            "RebuildOutcome",
            "CommitOutcome",
        ] {
            assert!(
                generated.contains(&format!("export type {name}"))
                    || generated.contains(&format!("export interface {name}")),
                "expected named TS export for {name}"
            );
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

        // T8 freeze: the named-export loop above only proves these types exist;
        // pin the actual generated shapes so a wire-format drift breaks CI.
        // Substrings are copied verbatim from the generated product (prettier
        // emits single-quoted tags and one union variant per line).
        assert!(
            generated.contains(
                "export type RebuildOutcome =\n  | { status: 'ok' }\n  | { status: 'degraded'; error: string }"
            ),
            "RebuildOutcome tagged-union shape drifted from the generated bindings"
        );
        // importProfile must return the instantiated generic, not a bare uid.
        assert!(
            generated.contains("CommitOutcome<ProfileId>"),
            "importProfile must return CommitOutcome<ProfileId>"
        );
    }
}
