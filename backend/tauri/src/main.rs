#![feature(auto_traits, negative_impls)]
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[cfg(target_os = "macos")]
#[macro_use]
extern crate cocoa;

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

mod cmds;
mod config;
mod consts;
mod core;
mod enhance;
mod feat;
mod ipc;
mod utils;
use crate::{
    config::Config,
    core::handle::Handle,
    utils::{init, resolve},
};
use tauri::{api, Manager, SystemTray};

rust_i18n::i18n!("../../locales");

#[cfg(feature = "deadlock-detection")]
fn deadlock_detection() {
    use parking_lot::deadlock;
    use std::{thread, time::Duration};
    use tracing::error;
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(10));
        let deadlocks = deadlock::check_deadlock();
        if deadlocks.is_empty() {
            continue;
        }

        error!("{} deadlocks detected", deadlocks.len());
        for (i, threads) in deadlocks.iter().enumerate() {
            error!("Deadlock #{}", i);
            for t in threads {
                error!("Thread Id {:#?}", t.thread_id());
                error!("{:#?}", t.backtrace());
            }
        }
    });
}

fn main() -> std::io::Result<()> {
    // share the tauri async runtime to nyanpasu-utils
    #[cfg(feature = "deadlock-detection")]
    deadlock_detection();

    // Parse commands
    cmds::parse().unwrap();

    // Should be in first place in order prevent single instance check block everything
    #[cfg(feature = "verge-dev")]
    tauri_plugin_deep_link::prepare("moe.elaina.clash.nyanpasu.dev");

    #[cfg(not(feature = "verge-dev"))]
    tauri_plugin_deep_link::prepare("moe.elaina.clash.nyanpasu");

    // 单例检测
    let single_instance_result = utils::init::check_singleton();

    // Use system locale as default
    let locale = {
        let locale = utils::help::get_system_locale();
        utils::help::mapping_to_i18n_key(&locale)
    };
    rust_i18n::set_locale(locale);

    if let Err(e) = init::run_pending_migrations() {
        utils::dialog::panic_dialog(
            &format!(
                "Failed to finish migration event: {}\nYou can see the detailed information at migration.log in your local data dir.\nYou're supposed to submit it as the attachment of new issue.", 
                e,
            )
        );
        std::process::exit(1);
    }

    crate::log_err!(init::init_config());

    // Panic Hook to show a panic dialog and save logs
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error!(format!("panic hook: {:?}", info));
        utils::dialog::panic_dialog(&format!("{:?}", info));
        default_panic(info);
    }));

    let verge = { Config::verge().latest().language.clone().unwrap() };
    rust_i18n::set_locale(verge.as_str());

    // show a dialog to print the single instance error
    let _singleton = single_instance_result.unwrap(); // hold the guard until the end of the program

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .system_tray(SystemTray::new())
        .setup(|app| {
            resolve::resolve_setup(app);
            // setup custom scheme
            let handle = app.handle().clone();
            // For start new app from schema
            #[cfg(not(target_os = "macos"))]
            if let Some(url) = std::env::args().nth(1) {
                log::info!(target: "app", "started with schema");
                if Config::verge().data().enable_silent_start.unwrap_or(true) {
                    resolve::create_window(&handle.clone());
                }
                app.listen_global("init-complete", move |_| {
                    log::info!(target: "app", "frontend init-complete event received");
                    Handle::global()
                        .app_handle
                        .lock()
                        .as_ref()
                        .unwrap()
                        .emit_all("scheme-request-received", url.clone())
                        .unwrap();
                });
            }

            log_err!(tauri_plugin_deep_link::register(
                &["clash-nyanpasu", "clash"],
                move |request| {
                    log::info!(target: "app", "scheme request received: {:?}", &request);
                    resolve::create_window(&handle.clone()); // create window if not exists
                    handle.emit_all("scheme-request-received", request).unwrap();
                }
            ));
            Ok(())
        })
        .on_system_tray_event(core::tray::Tray::on_system_tray_event)
        .invoke_handler(tauri::generate_handler![
            // common
            ipc::get_sys_proxy,
            ipc::open_app_config_dir,
            ipc::open_app_data_dir,
            ipc::open_logs_dir,
            ipc::open_web_url,
            ipc::open_core_dir,
            // cmds::kill_sidecar,
            ipc::restart_sidecar,
            ipc::grant_permission,
            // clash
            ipc::get_clash_info,
            ipc::get_clash_logs,
            ipc::patch_clash_config,
            ipc::change_clash_core,
            ipc::get_runtime_config,
            ipc::get_runtime_yaml,
            ipc::get_runtime_exists,
            ipc::get_runtime_logs,
            ipc::clash_api_get_proxy_delay,
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
            ipc::update_profile,
            ipc::delete_profile,
            ipc::read_profile_file,
            ipc::save_profile_file,
            ipc::save_window_size_state,
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
            ipc::get_proxies,
            ipc::select_proxy,
            ipc::update_proxy_provider,
            ipc::restart_application,
            ipc::collect_envs,
        ]);

    #[cfg(target_os = "macos")]
    {
        use tauri::{Menu, MenuItem, Submenu};

        builder = builder.menu(
            Menu::new().add_submenu(Submenu::new(
                "Edit",
                Menu::new()
                    .add_native_item(MenuItem::Undo)
                    .add_native_item(MenuItem::Redo)
                    .add_native_item(MenuItem::Copy)
                    .add_native_item(MenuItem::Paste)
                    .add_native_item(MenuItem::Cut)
                    .add_native_item(MenuItem::SelectAll)
                    .add_native_item(MenuItem::CloseWindow)
                    .add_native_item(MenuItem::Quit),
            )),
        );
    }

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(|app_handle, e| match e {
        tauri::RunEvent::ExitRequested { api, .. } => {
            api.prevent_exit();
        }
        tauri::RunEvent::Exit => {
            resolve::resolve_reset();
            api::process::kill_children();
            app_handle.exit(0);
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::WindowEvent { label, event, .. } => {
            use tauri::Manager;

            if label == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = resolve::save_window_state(app_handle, true);

                    if let Some(win) = app_handle.get_window("main") {
                        let _ = win.hide();
                    }
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        tauri::RunEvent::WindowEvent { label, event, .. } => {
            if label == "main" {
                match event {
                    tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                        core::tray::on_scale_factor_changed(scale_factor);
                    }
                    tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed => {
                        // log::info!(target: "app", "window close requested");
                        let _ = resolve::save_window_state(app_handle, true);
                    }
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                        // log::info!(target: "app", "window moved or resized");
                        std::thread::sleep(std::time::Duration::from_nanos(1));
                        let _ = resolve::save_window_state(app_handle, false);
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    });

    Ok(())
}
