#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
// This lint was needed by ambassador
#![allow(clippy::duplicated_attributes)]
mod bundle;
mod client;
mod cmds;
mod consts;
mod core;
mod enhance;
mod event_handler;
mod ipc;
mod server;
mod service;
mod setup;
mod specta_export;
mod state;
mod unified_rpc;

#[cfg(windows)]
mod shutdown_hook;
mod utils;
mod widget;
mod window;

use crate::{
    core::backup::BACKUP_FAILED_EXIT_CODE,
    utils::{init, resolve},
};
use anyhow::Context;
use specta_typescript::Typescript;
use tauri::Manager;
use tauri_specta::Event;

rust_i18n::i18n!("./locales");

#[cfg(feature = "deadlock-detection")]
fn deadlock_detection() {
    use parking_lot::deadlock;
    use std::{thread, time::Duration};
    use tracing::error;
    thread::spawn(move || {
        loop {
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
        }
    });
}

/// Shows a panic dialog and saves logs, then exits once the app has shut down
/// when a handle exists, or at once otherwise.
fn install_panic_hook(app_handle: Option<tauri::AppHandle>) {
    nyanpasu_panics::setup_panic_hook(move |report| {
        let nyanpasu_panics::PanicReport {
            payload,
            location,
            backtrace,
        } = report;

        // This is a workaround for the upstream issue: https://github.com/tauri-apps/tauri/issues/10546
        if let Some(s) = payload
            && s.contains("PostMessage failed ; is the messages queue full?")
        {
            return;
        }

        // FIXME: maybe move this logic to a util function?
        let msg = format!(
            "Oops, we encountered some issues and program will exit immediately.

payload: {payload:#?}
location: {location:?}
backtrace: {backtrace:#?}

",
        );
        let child = std::process::Command::new(tauri::utils::platform::current_exe().unwrap())
            .arg("panic-dialog")
            .arg(msg.as_str())
            .spawn();
        // fallback to show a dialog directly
        if child.is_err() {
            utils::dialog::panic_dialog(msg.as_str());
        }

        match &app_handle {
            // The event loop that would run the exit is the one panicking, so
            // the shutdown runs here and the process ends after it.
            Some(app_handle) if utils::main_thread::is_main_thread() => {
                utils::exit::shutdown_on_main_thread(app_handle);
                std::process::exit(1);
            }
            // The hook must return: the panicking task can finish, and release
            // what it holds, only once it unwinds. The exit boundary then
            // waits for every owner before the app exits.
            Some(app_handle) => app_handle.exit(1),
            None => std::process::exit(1),
        }
    });
}

/// Queues a deep link for the frontend, then pokes any listening frontend to
/// take it. With no frontend listening yet the poke is lost but the link is
/// not: a frontend drains the queue once it has registered its listener.
fn queue_deep_link(app_handle: &tauri::AppHandle, url: String) {
    app_handle.state::<crate::ipc::PendingDeepLinks>().push(url);
    log_err!(
        crate::ipc::SchemeRequestReceivedEvent.emit(app_handle),
        "failed to emit scheme-request-received event"
    );
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> std::io::Result<()> {
    let startup = std::sync::Arc::new(utils::startup::StartupTimings::new());
    let entry = startup.stage("entry.commands_and_deep_link");
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
    drop(entry);

    // 单例检测 with robust logging
    let singleton = startup.stage("entry.single_instance");
    let single_instance_result = utils::init::check_singleton();
    drop(singleton);
    match &single_instance_result {
        Ok(Some(_)) => {
            tracing::info!(target: "app", "Acquired single-instance lock");
        }
        Ok(None) => {
            tracing::warn!(target: "app", "Another instance is running; exiting");
            std::process::exit(0);
        }
        Err(e) => {
            tracing::error!(target: "app", "Failed to check single-instance lock: {e:?}");
            // Policy: continue startup in best-effort mode
        }
    }
    // Use system locale as default
    let locale = startup.stage("entry.system_locale");
    rust_i18n::set_locale(utils::help::detect_system_i18n_key());
    drop(locale);

    let migrations = startup.stage("entry.migration_subprocess");
    if single_instance_result
        .as_ref()
        .is_ok_and(|instance| instance.is_some())
        && let Err(e) = init::run_pending_migrations()
    {
        let backup_failed = e
            .downcast_ref::<init::MigrationChildFailed>()
            .is_some_and(|failed| failed.status.code() == Some(BACKUP_FAILED_EXIT_CODE));
        let message = format!("Failed to finish migration event: {e}");
        match utils::path::PathResolver::from_env(None) {
            Ok(paths) => utils::dialog::migration_failed_dialog(&message, &paths, backup_failed),
            Err(_) => utils::dialog::panic_dialog(&message),
        }
        std::process::exit(1);
    }
    drop(migrations);

    let logging = startup.stage("entry.logging");
    let (logger_reload, jobs_capture) =
        init::logging::init().expect("failed to initialize logging");
    drop(logging);
    startup.enable_logging();
    tracing::info!(
        target: "clash_nyanpasu::startup",
        pid = std::process::id(),
        debug_assertions = cfg!(debug_assertions),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        "startup measurement context"
    );
    let config = startup.stage("entry.config_files");
    crate::log_err!(init::init_config());
    drop(config);

    // Until setup hands over an app handle, a panic can only end the process.
    install_panic_hook(None);

    // Keep the Tauri transport surface separate from the RPC schema.
    let transport = startup.stage("entry.transport_schema");
    let transport_builder = specta_export::build_transport_builder();
    drop(transport);
    #[cfg(debug_assertions)]
    {
        let _bindings = startup.stage("entry.debug_bindings_export");
        let (query_bindings, rpc_schema_builder) = specta_export::build_specta_builder();
        const SPECTA_BINDINGS_PATH: &str = "../../frontend/rpc/src/tauri-bindings.ts";
        const RPC_BINDINGS_PATH: &str = "../../frontend/rpc/src/rpc-bindings.ts";
        const QUERY_BINDINGS_PATH: &str = "../../frontend/query/src/query-bindings.ts";

        transport_builder
            .export(
                Typescript::default().header("/* oxlint-disable */\n// @ts-nocheck"),
                SPECTA_BINDINGS_PATH,
            )
            .expect("Failed to export Tauri transport bindings");
        match rpc_schema_builder.export(
            Typescript::default().header("/* oxlint-disable */\n// @ts-nocheck"),
            RPC_BINDINGS_PATH,
        ) {
            Ok(_) => {
                if let Err(e) = specta_export::adapt_rpc_bindings(RPC_BINDINGS_PATH) {
                    panic!("Failed to generate RPC bindings: {e}");
                }
                specta_export::adapt_query_bindings(QUERY_BINDINGS_PATH, &query_bindings)
                    .expect("Failed to generate query bindings");
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
                let _ = prettier
                    .args([
                        "--write",
                        SPECTA_BINDINGS_PATH,
                        RPC_BINDINGS_PATH,
                        QUERY_BINDINGS_PATH,
                    ])
                    .output();
                log::debug!("Exported typescript bindings, path: {SPECTA_BINDINGS_PATH}");
            }
            Err(e) => {
                panic!("Failed to export typescript bindings: {e}");
            }
        };
    }

    // show a dialog to print the single instance error
    // Hold the guard until the end of the program if acquired
    let _singleton = match single_instance_result {
        Ok(Some(guard)) => Some(guard),
        _ => None,
    };

    let bundle = startup.stage("entry.bundle_context");
    let mut context = tauri::generate_context!();
    let executable_dir =
        utils::dirs::app_install_dir().expect("failed to locate the application directory");
    let metadata =
        bundle::BundleMetadata::resolve(cfg!(windows), context.config(), &executable_dir)
            .expect("failed to resolve bundle metadata");
    let updater = metadata
        .setup(context.config_mut(), &executable_dir)
        .expect("failed to configure bundle startup");
    drop(bundle);

    let plugins = startup.stage("tauri.plugin_construction");
    let setup_startup = startup.clone();
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .manage(startup.clone())
        .manage(utils::exit::ExitBoundary::default())
        .manage(utils::app_update::UpdateInstallation::default())
        .invoke_handler(transport_builder.invoke_handler())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(updater.build())
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .on_page_load(|webview, payload| {
            if webview.label() == crate::consts::MAIN_WINDOW_LABEL
                && let Some(startup) =
                    webview.try_state::<std::sync::Arc<utils::startup::StartupTimings>>()
            {
                startup.milestone(match payload.event() {
                    tauri::webview::PageLoadEvent::Started => "main_page_load_started",
                    tauri::webview::PageLoadEvent::Finished => "main_page_load_finished",
                });
            }
            // A reloaded page keeps the same webview, so `Channel::send`
            // cannot tell its old connection-detail subscriptions are gone.
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                core::clash::connection_details::cancel_for_webview(webview, webview.label());
            }
        })
        .setup(move |app| {
            let _setup = setup_startup.stage("tauri.setup");
            let services = setup_startup.stage("setup.services");
            transport_builder.mount_events(app);
            setup::setup(app, metadata, logger_reload, jobs_capture)
                .context("Failed to setup the app")
                .inspect_err(|e| {
                    tracing::error!("Failed to setup the app: {:#?}", e);
                })?;
            drop(services);

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

            install_panic_hook(Some(app.handle().clone()));
            resolve::resolve_setup(app, &setup_startup);
            let rpc = setup_startup.stage("setup.unified_rpc");
            setup::setup_unified_rpc(app).context("Failed to initialize unified RPC")?;
            drop(rpc);

            // setup custom scheme
            let _deep_links = setup_startup.stage("setup.deep_links");
            let handle = app.handle().clone();
            // Deep links wait here until a frontend takes them.
            app.manage(crate::ipc::PendingDeepLinks::default());
            // For start new app from schema
            #[cfg(not(target_os = "macos"))]
            if let Some(url) = custom_scheme {
                log::info!(target: "app", "started with schema");
                queue_deep_link(&handle, url.to_string());
                resolve::create_window(&handle.clone());
            }
            // This operation should terminate the app if app is called by custom scheme and this instance is not the primary instance
            log_err!(tauri_plugin_deep_link::register(
                &["clash-nyanpasu", "clash"],
                move |request| {
                    log::info!(target: "app", "scheme request received: {:?}", request);
                    resolve::create_window(&handle.clone()); // create window if not exists
                    queue_deep_link(&handle, request);
                }
            ));
            Ok(())
        });
    drop(plugins);

    let build = startup.stage("tauri.build");
    let app = builder
        .build(context)
        .expect("error while running tauri application");
    drop(build);
    startup.milestone("tauri_build_finished");
    app.run(move |app_handle, e| match e {
        tauri::RunEvent::Ready => startup.milestone("event_loop_ready"),
        tauri::RunEvent::ExitRequested { api, code, .. } => {
            utils::exit::on_exit_requested(app_handle, code, &api);
        }
        tauri::RunEvent::WindowEvent { label, event, .. } => {
            if label == "main" {
                match &event {
                    tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                        core::tray::on_scale_factor_changed(*scale_factor);
                    }
                    tauri::WindowEvent::CloseRequested { .. } => {
                        log::debug!(target: "app", "window close requested");
                        let _ = resolve::save_window_state(app_handle);
                        #[cfg(target_os = "macos")]
                        crate::utils::dock::macos::hide_dock_icon();
                    }
                    tauri::WindowEvent::Destroyed => {
                        log::debug!(target: "app", "window destroyed");
                    }
                    _ => {}
                }
            }
            // Every webview's connection-detail subscriptions must end on
            // destroy, not just "main"'s: `Channel::send` cannot detect it.
            if matches!(event, tauri::WindowEvent::Destroyed) {
                core::clash::connection_details::cancel_for_webview(app_handle, &label);
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            resolve::create_window(app_handle);
        }
        _ => {}
    });

    Ok(())
}
