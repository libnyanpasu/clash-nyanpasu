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
            ipc::patch_profiles_config,
            ipc::view_profile,
            ipc::patch_profile,
            ipc::create_profile,
            ipc::import_profile,
            ipc::reorder_profile,
            ipc::reorder_profiles_by_list,
            ipc::update_profile,
            ipc::delete_profile,
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
            core::storage::StorageValueChangedEvent
        ])
        .dangerously_cast_bigints_to_number()
}
