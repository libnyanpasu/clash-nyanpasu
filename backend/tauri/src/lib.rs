#![feature(auto_traits, negative_impls, let_chains)]
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
// This lint was needed by ambassador
#![allow(clippy::duplicated_attributes)]
mod cmds;
mod config;
mod consts;
mod core;
mod enhance;
mod event_handler;
mod feat;
mod ipc;
mod server;
mod setup;
mod utils;
mod widget;
mod window;

use std::io;

use crate::{
    config::Config,
    core::handle::Handle,
    utils::{init, resolve},
};
use specta_typescript::{BigIntExportBehavior, Typescript};
use tauri::{Emitter, Manager};
use tauri_specta::{collect_commands, collect_events};
use utils::resolve::{is_window_opened, reset_window_open_counter};

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> std::io::Result<()> {
    // share the tauri async runtime to nyanpasu-utils
    #[cfg(feature = "deadlock-detection")]
    deadlock_detection();

    // Should be in first place in order prevent single instance check block everything
    // Custom scheme check
    #[cfg(not(target_os = "macos"))]
    // on macos the plugin handles this (macos doesn't use cli args for the url)
    let custom_scheme = match std::env::args().nth(1) {
        Some(url) => url::Url::parse(&url).ok(),
        None => None,
    };
    #[cfg(target_os = "macos")]
    let custom_scheme: Option<url::Url> = None;

    if custom_scheme.is_none() {
        // Parse commands
        cmds::parse().unwrap();
    };
    #[cfg(feature = "verge-dev")]
    tauri_plugin_deep_link::prepare("moe.elaina.clash.nyanpasu.dev");

    #[cfg(not(feature = "verge-dev"))]
    tauri_plugin_deep_link::prepare("moe.elaina.clash.nyanpasu");

    // 单例检测
    let single_instance_result = utils::init::check_singleton();
    if single_instance_result
        .as_ref()
        .is_ok_and(|instance| instance.is_none())
    {
        std::process::exit(0);
    }
    // Use system locale as default
    let locale = {
        let locale = utils::help::get_system_locale();
        utils::help::mapping_to_i18n_key(&locale)
    };
    rust_i18n::set_locale(locale);

    if single_instance_result
        .as_ref()
        .is_ok_and(|instance| instance.is_some())
    {
        if let Err(e) = init::run_pending_migrations() {
            utils::dialog::panic_dialog(
                &format!(
                    "Failed to finish migration event: {}\nYou can see the detailed information at migration.log in your local data dir.\nYou're supposed to submit it as the attachment of new issue.", 
                    e,
                )
            );
            std::process::exit(1);
        }
    }

    crate::log_err!(init::init_config());

    // Panic Hook to show a panic dialog and save logs
    std::panic::set_hook(Box::new(move |panic_info| {
        use std::backtrace::{Backtrace, BacktraceStatus};
        let payload = panic_info.payload();

        #[allow(clippy::manual_map)]
        let payload = if let Some(s) = payload.downcast_ref::<&str>() {
            Some(&**s)
        } else if let Some(s) = payload.downcast_ref::<String>() {
            Some(s.as_str())
        } else {
            None
        };

        let location = panic_info.location().map(|l| l.to_string());
        let (backtrace, note) = {
            let backtrace = Backtrace::force_capture();
            let note = (backtrace.status() == BacktraceStatus::Disabled)
                .then_some("run with RUST_BACKTRACE=1 environment variable to display a backtrace");
            (Some(backtrace), note)
        };

        tracing::error!(
            panic.payload = payload,
            panic.location = location,
            panic.backtrace = backtrace.as_ref().map(tracing::field::display),
            panic.note = note,
            "A panic occurred",
        );

        // This is a workaround for the upstream issue: https://github.com/tauri-apps/tauri/issues/10546
        if let Some(s) = payload.as_ref()
            && s.contains("PostMessage failed ; is the messages queue full?")
        {
            return;
        }

        // FIXME: maybe move this logic to a util function?
        let msg = format!(
            "Oops, we encountered some issues and program will exit immediately.\n\npayload: {:#?}\nlocation: {:?}\nbacktrace: {:#?}\n\n",
            payload, location, backtrace,
        );
        let child = std::process::Command::new(tauri::utils::platform::current_exe().unwrap())
            .arg("panic-dialog")
            .arg(msg.as_str())
            .spawn();
        // fallback to show a dialog directly
        if child.is_err() {
            utils::dialog::panic_dialog(msg.as_str());
        }

        match Handle::global().app_handle.lock().as_ref() {
            Some(app_handle) => {
                app_handle.exit(1);
            }
            None => {
                log::error!("app handle is not initialized");
                std::process::exit(1);
            }
        }
    }));

    // setup specta
    let specta_builder = tauri_specta::Builder::<tauri::Wry>::new()
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
            ipc::reorder_profiles_by_list,
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
            ipc::mutate_proxies,
            ipc::get_core_dir,
            // clash layer
            ipc::get_clash_ws_connections_state,
        ])
        .events(collect_events![core::clash::ClashConnectionsEvent]);

    #[cfg(debug_assertions)]
    {
        const SPECTA_BINDINGS_PATH: &str = "../../frontend/interface/src/ipc/bindings.ts";

        match specta_builder.export(
            Typescript::default()
                .formatter(specta_typescript::formatter::prettier)
                .formatter(|file| {
                    let npx_command = if cfg!(target_os = "windows") {
                        "npx.cmd"
                    } else {
                        "npx"
                    };

                    std::process::Command::new(npx_command)
                        .arg("prettier")
                        .arg("--write")
                        .arg(file)
                        .output()
                        .map(|_| ())
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                })
                .bigint(BigIntExportBehavior::Number)
                .header("/* eslint-disable */\n// @ts-nocheck"),
            SPECTA_BINDINGS_PATH,
        ) {
            Ok(_) => {
                log::debug!(
                    "Exported typescript bindings, path: {}",
                    SPECTA_BINDINGS_PATH
                );
            }
            Err(e) => {
                panic!("Failed to export typescript bindings: {}", e);
            }
        };
    }

    let verge = { Config::verge().latest().language.clone().unwrap() };
    rust_i18n::set_locale(verge.as_str());

    // show a dialog to print the single instance error
    let _singleton = single_instance_result.unwrap().unwrap(); // hold the guard until the end of the program

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .invoke_handler(specta_builder.invoke_handler())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .setup(move |app| {
            specta_builder.mount_events(app);

            #[cfg(target_os = "macos")]
            {
                use tauri::menu::{MenuBuilder, SubmenuBuilder};
                let submenu = SubmenuBuilder::new(app, "Edit")
                    .undo()
                    .redo()
                    .copy()
                    .paste()
                    .cut()
                    .select_all()
                    .close_window()
                    .quit()
                    .build()
                    .unwrap();
                let menu = MenuBuilder::new(app).item(&submenu).build().unwrap();
                app.set_menu(menu).unwrap();
            }

            resolve::resolve_setup(app);

            // setup custom scheme
            let handle = app.handle().clone();
            // For start new app from schema
            #[cfg(not(target_os = "macos"))]
            if let Some(url) = custom_scheme {
                log::info!(target: "app", "started with schema");
                resolve::create_window(&handle.clone());
                while !is_window_opened() {
                    log::info!(target: "app", "waiting for window open");
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Handle::global()
                    .app_handle
                    .lock()
                    .as_ref()
                    .unwrap()
                    .emit("scheme-request-received", url.clone())
                    .unwrap();
            }
            // This operation should terminate the app if app is called by custom scheme and this instance is not the primary instance
            log_err!(tauri_plugin_deep_link::register(
                &["clash-nyanpasu", "clash"],
                move |request| {
                    log::info!(target: "app", "scheme request received: {:?}", &request);
                    resolve::create_window(&handle.clone()); // create window if not exists
                    while !is_window_opened() {
                        log::info!(target: "app", "waiting for window open");
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    handle.emit("scheme-request-received", request).unwrap();
                }
            ));
            std::thread::spawn(move || {
                nyanpasu_utils::runtime::block_on(async move {
                    server::run(*server::SERVER_PORT)
                        .await
                        .expect("failed to start server");
                });
            });
            Ok(())
        });

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while running tauri application");
    app.run(|app_handle, e| match e {
        tauri::RunEvent::ExitRequested { api, code, .. } if code.is_none() => {
            api.prevent_exit();
        }
        tauri::RunEvent::ExitRequested { .. } => {
            utils::help::cleanup_processes(app_handle);
        }
        tauri::RunEvent::WindowEvent { label, event, .. } if label == "main" => match event {
            tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                core::tray::on_scale_factor_changed(scale_factor);
            }
            tauri::WindowEvent::CloseRequested { .. } => {
                log::debug!(target: "app", "window close requested");
                let _ = resolve::save_window_state(app_handle, true);
                #[cfg(target_os = "macos")]
                crate::utils::dock::macos::hide_dock_icon();
            }
            tauri::WindowEvent::Destroyed => {
                log::debug!(target: "app", "window destroyed");
                reset_window_open_counter();
            }
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                log::debug!(target: "app", "window moved or resized");
                std::thread::sleep(std::time::Duration::from_nanos(1));
                let _ = resolve::save_window_state(app_handle, false);
            }
            _ => {}
        },
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            resolve::create_window(app_handle);
        }
        _ => {}
    });

    Ok(())
}
