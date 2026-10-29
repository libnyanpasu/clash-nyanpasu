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
mod startup_failure;
mod state;
mod unified_rpc;

mod host_paths;
#[cfg(windows)]
mod shutdown_hook;
mod utils;
mod widget;
mod window;

use crate::utils::{init, resolve};
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

        // The version and commit name the build whose archived symbols resolve
        // the backtrace's module offsets.
        // FIXME: maybe move this logic to a util function?
        let msg = format!(
            "Oops, we encountered some issues and program will exit immediately.

version: {} ({})
payload: {payload:#?}
location: {location:?}
backtrace:
{backtrace}
",
            consts::BUILD_INFO.pkg_version,
            consts::BUILD_INFO.commit_hash,
        );
        let details = nyanpasu_crash_reporter::PanicDetails {
            parent_pid: std::process::id(),
            payload: payload.map(str::to_owned),
            location: location.clone(),
            backtrace: backtrace.to_string(),
        };
        let child = (|| -> anyhow::Result<()> {
            use std::io::Write as _;

            // The reporter outlives the panicking parent. Closing stdin after
            // the synchronous write lets it read the entire JSON before UI setup.
            #[allow(clippy::zombie_processes)]
            let mut child = std::process::Command::new(tauri::utils::platform::current_exe()?)
                .args(["panic-dialog", "--language", rust_i18n::locale().as_ref()])
                .stdin(std::process::Stdio::piped())
                .spawn()?;
            let mut stdin = child.stdin.take().expect("the child's stdin is piped");
            stdin.write_all(&serde_json::to_vec(&details)?)?;
            Ok(())
        })();
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
pub fn run() -> std::process::ExitCode {
    // Nothing before the logger reaches a trace, so its share is logged.
    let started = std::time::Instant::now();
    rust_i18n::set_locale(utils::help::detect_system_i18n_key());
    // The one resolver: every later stage is given it, none looks the directories up again.
    // Every stage needs both roots, so a host that has none ends the process here.
    let paths = match host_paths::discover() {
        Ok(paths) => paths,
        Err(error) => {
            let path = match &error {
                nyanpasu_paths::DiscoverError::NotUtf8 { path, .. } => Some(path.clone()),
                nyanpasu_paths::DiscoverError::PortableInstallDir {
                    source: nyanpasu_paths::InstallDirError::NotUtf8 { path },
                } => Some(path.clone()),
                _ => None,
            };
            startup_failure::show_one_shot(
                startup_failure::FailureKind::Directories,
                path,
                &error,
                None,
            );
            return std::process::ExitCode::FAILURE;
        }
    };
    let mut profilers = utils::profiling::Profilers::default();
    #[cfg(feature = "dhat-heap")]
    profilers.start_heap(&paths);

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
        match cmds::parse(&paths) {
            Ok(true) => return std::process::ExitCode::SUCCESS,
            Ok(false) => {}
            Err(error) => {
                eprintln!("{}", snafu::Report::from_error(&error));
                return std::process::ExitCode::FAILURE;
            }
        }
    };
    // Reuse nyanpasu-utils' process-lived runtime before Tauri initializes its own.
    // Only the GUI needs it: the subcommands above exit without it, and building
    // it starts a worker thread per CPU, at most four.
    tauri::async_runtime::set(nyanpasu_utils::runtime::get_runtime_handle());
    #[cfg(feature = "verge-dev")]
    tauri_plugin_deep_link::prepare("moe.elaina.clash.nyanpasu.dev");

    #[cfg(not(feature = "verge-dev"))]
    tauri_plugin_deep_link::prepare("moe.elaina.clash.nyanpasu");

    // 单例检测 with robust logging
    let single_instance_result = utils::init::check_singleton(&paths);
    match &single_instance_result {
        Ok(Some(_)) => {
            tracing::info!(target: "app", "Acquired single-instance lock");
        }
        Ok(None) => {
            tracing::warn!(target: "app", "Another instance is running; exiting");
            return std::process::ExitCode::SUCCESS;
        }
        Err(e) => {
            tracing::error!(target: "app", "Failed to check single-instance lock: {e:?}");
            // Policy: continue startup in best-effort mode
        }
    }

    // Past the single-instance lock, which a losing process never reaches, so it creates nothing.
    if let Err(error) = paths.create_base_dirs() {
        startup_failure::show_one_shot(
            startup_failure::FailureKind::Directories,
            Some(error.path.clone().into_std_path_buf()),
            &error,
            None,
        );
        return std::process::ExitCode::FAILURE;
    }

    if single_instance_result
        .as_ref()
        .is_ok_and(|instance| instance.is_some())
        && let Err(e) = init::run_pending_migrations(&paths)
    {
        if let init::Error::OpenMigrationLog { path, .. } = &e {
            startup_failure::show_one_shot(
                startup_failure::FailureKind::Logging,
                Some(path.clone()),
                &e,
                None,
            );
        } else {
            startup_failure::show(&startup_failure::Report::migration(&e, &paths));
        }
        return std::process::ExitCode::FAILURE;
    }

    let (logger_reload, jobs_capture) = match init::logging::init(&mut profilers, &paths) {
        Ok(logging) => logging,
        Err(error) => {
            let path = match &error {
                init::logging::Error::CreateWriter { path, .. }
                | init::logging::Error::CreateDirectory { path, .. } => path.clone(),
                _ => paths.app_logs_dir().into_std_path_buf(),
            };
            startup_failure::show_one_shot(
                startup_failure::FailureKind::Logging,
                Some(path),
                &error,
                None,
            );
            return std::process::ExitCode::FAILURE;
        }
    };
    let startup_logging = init::logging::LoggingShutdown(logger_reload.clone());
    tracing::info!(
        pre_logging_ms = started.elapsed().as_millis(),
        "logging initialized"
    );
    let prepare_span = tracing::info_span!("prepare_app").entered();
    crate::log_err!(init::init_config(&paths));

    // Until setup hands over an app handle, a panic can only end the process.
    install_panic_hook(None);

    // Keep the Tauri transport surface separate from the RPC schema.
    let transport_builder = specta_export::build_transport_builder();
    #[cfg(debug_assertions)]
    let (query_bindings, rpc_schema_builder) = specta_export::build_specta_builder();

    #[cfg(debug_assertions)]
    {
        const SPECTA_BINDINGS_PATH: &str = "../../frontend/rpc/src/tauri-bindings.ts";
        const RPC_BINDINGS_PATH: &str = "../../frontend/rpc/src/rpc-bindings.ts";
        const QUERY_BINDINGS_PATH: &str = "../../frontend/query/src/query-bindings.ts";

        if let Err(error) = transport_builder.export(
            Typescript::default().header("/* oxlint-disable */\n// @ts-nocheck"),
            SPECTA_BINDINGS_PATH,
        ) {
            startup_failure::show_internal(&error, &paths, &startup_logging);
            return std::process::ExitCode::FAILURE;
        }
        match rpc_schema_builder.export(
            Typescript::default().header("/* oxlint-disable */\n// @ts-nocheck"),
            RPC_BINDINGS_PATH,
        ) {
            Ok(_) => {
                if let Err(e) = specta_export::adapt_rpc_bindings(RPC_BINDINGS_PATH) {
                    startup_failure::show_internal(&e, &paths, &startup_logging);
                    return std::process::ExitCode::FAILURE;
                }
                if let Err(error) =
                    specta_export::adapt_query_bindings(QUERY_BINDINGS_PATH, &query_bindings)
                {
                    startup_failure::show_internal(&error, &paths, &startup_logging);
                    return std::process::ExitCode::FAILURE;
                }
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
                startup_failure::show_internal(&e, &paths, &startup_logging);
                return std::process::ExitCode::FAILURE;
            }
        };
    }

    // show a dialog to print the single instance error
    // Hold the guard until the end of the program if acquired
    let _singleton = match single_instance_result {
        Ok(Some(guard)) => Some(guard),
        _ => None,
    };

    let mut context = tauri::generate_context!();
    let executable_dir = match paths.app_install_dir() {
        Ok(directory) => directory.as_std_path(),
        Err(error) => {
            let path = match &error {
                nyanpasu_paths::InstallDirError::NotUtf8 { path } => Some(path.clone()),
                _ => None,
            };
            startup_failure::show_one_shot(
                startup_failure::FailureKind::Directories,
                path,
                &error,
                None,
            );
            startup_logging.finish_blocking();
            return std::process::ExitCode::FAILURE;
        }
    };
    let metadata = match bundle::BundleMetadata::resolve(
        cfg!(windows),
        context.config_mut(),
        executable_dir,
        || tauri::webview_version().is_ok(),
    ) {
        Ok(metadata) => metadata,
        Err(error) => {
            let path = match &error {
                bundle::Error::InspectWebview { path, .. }
                | bundle::Error::ValidateWebview { path } => Some(path.clone()),
                _ => None,
            };
            let recovery = if bundle::is_portable(executable_dir) {
                "web_view2.portable_recovery"
            } else {
                "web_view2.installed_recovery"
            };
            startup_failure::show_one_shot(
                startup_failure::FailureKind::WebView2,
                path,
                &error,
                Some(recovery),
            );
            startup_logging.finish_blocking();
            return std::process::ExitCode::FAILURE;
        }
    };
    let updater = metadata.setup(context.config_mut()).expect(
        "the embedded bundle configuration has valid updater endpoints and a supported target",
    );
    // The setup closure owns its copy; build failures still need paths for the reporter.
    let failure_paths = paths.clone();

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .manage(utils::exit::ExitBoundary::default())
        .manage(init::logging::LoggingShutdown(logger_reload.clone()))
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
            // A reloaded page keeps the same webview, so `Channel::send`
            // cannot tell its old connection-detail subscriptions are gone.
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                core::clash::connection_details::cancel_for_webview(webview, webview.label());
            }
        })
        .setup(move |app| {
            let failure_host = setup::TauriStartupFailureHost {
                handle: app.handle().clone(),
                paths: paths.clone(),
            };
            setup::run_setup(
                setup::StartupOwners::default(),
                |owners| {
                    transport_builder.mount_events(app);
                    setup::setup(
                        app,
                        metadata,
                        logger_reload,
                        jobs_capture,
                        paths.clone(),
                        owners,
                    )
                    .map_err(|error| startup_failure::StartupFailure::from_setup(&error, &paths))?;

                    #[cfg(target_os = "macos")]
                    {
                        use tauri::menu::{MenuBuilder, SubmenuBuilder};
                        let menu = (|| -> tauri::Result<()> {
                            let submenu = SubmenuBuilder::new(app, "Edit")
                                .undo()
                                .redo()
                                .copy()
                                .paste()
                                .cut()
                                .select_all()
                                .close_window()
                                .quit()
                                .build()?;
                            let menu = MenuBuilder::new(app).item(&submenu).build()?;
                            app.set_menu(menu)?;
                            Ok(())
                        })();
                        if let Err(error) = menu {
                            tracing::warn!(%error, "Edit menu unavailable; continuing without it");
                        }
                    }

                    install_panic_hook(Some(app.handle().clone()));
                    resolve::resolve_setup(app);
                    setup::setup_unified_rpc(app)
                        .map_err(|error| startup_failure::StartupFailure::from_rpc(&error))?;

                    // setup custom scheme
                    let handle = app.handle().clone();
                    // Deep links wait here until a frontend takes them.
                    app.manage(crate::ipc::PendingDeepLinks::default());
                    // For start new app from schema
                    #[cfg(not(target_os = "macos"))]
                    if let Some(url) = custom_scheme {
                        log::info!(target: "app", "started with schema");
                        queue_deep_link(&handle, url.to_string());
                        log_err!(
                            handle
                                .state::<window::WindowManager>()
                                .open(&window::kinds::MainWindow, None)
                        );
                    }
                    // This operation should terminate the app if app is called by custom scheme and this instance is not the primary instance
                    log_err!(tauri_plugin_deep_link::register(
                        &["clash-nyanpasu", "clash"],
                        move |request| {
                            log::info!(target: "app", "scheme request received: {:?}", request);
                            // create window if not exists
                            log_err!(
                                handle
                                    .state::<window::WindowManager>()
                                    .open(&window::kinds::MainWindow, None)
                            );
                            queue_deep_link(&handle, request);
                        }
                    ));
                    Ok(())
                },
                &failure_host,
            )
        });

    let app = match builder.build(context) {
        Ok(app) => app,
        Err(error) => {
            startup_failure::show_internal(&error, &failure_paths, &startup_logging);
            return std::process::ExitCode::FAILURE;
        }
    };
    drop(prepare_span);
    app.run(move |app_handle, e| match e {
        tauri::RunEvent::ExitRequested { api, code, .. } => {
            if code.is_some() {
                profilers.finish_heap();
            }
            utils::exit::on_exit_requested(app_handle, code, &api);
        }
        tauri::RunEvent::Exit => profilers.finish(),
        tauri::RunEvent::WindowEvent { label, event, .. } => {
            if label == "main" {
                match &event {
                    tauri::WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                        core::tray::on_scale_factor_changed(
                            &app_handle.state::<nyanpasu_paths::PathResolver>(),
                            *scale_factor,
                        );
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
            log_err!(
                app_handle
                    .state::<window::WindowManager>()
                    .open(&window::kinds::MainWindow, None)
            );
        }
        _ => {}
    });

    std::process::ExitCode::SUCCESS
}
