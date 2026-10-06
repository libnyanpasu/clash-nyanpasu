use crate::{
    client::{NyanpasuClient, application_workflow::startup::StartupOutcome},
    core::tray::proxies,
    log_err,
    window::WindowManager,
};
use tauri::{App, Manager};

/// handle something when start app
#[tracing_attributes::instrument(skip_all)]
pub fn resolve_setup(app: &mut App) {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    #[cfg(any(windows, target_os = "linux"))]
    log::trace!("init system tray");
    #[cfg(any(windows, target_os = "linux"))]
    crate::core::tray::icon::resize_images(crate::utils::help::get_max_scale_factor()); // generate latest cache icon by current scale factor

    {
        let client = app.state::<crate::client::NyanpasuClient>();
        // TODO(startup): resolve_setup needs restructuring; startup_reconcile
        // should not block setup. See
        // docs/plan/2026-09-28-workflow-lifecycle-simplification.md §9.
        let report = tracing::info_span!("startup_reconcile")
            .in_scope(|| tauri::async_runtime::block_on(client.startup_reconcile()));
        if let Some(observation) = &report.observation {
            log::info!(
                target: "app",
                "startup reconcile {} observed: desired {:?}, service {:?}, runtime {:?}",
                report.operation_id,
                observation.desired,
                observation.service,
                observation.runtime
            );
        }
        match &report.outcome {
            StartupOutcome::Ready => {
                log::info!(target: "app", "startup reconcile {}: ready", report.operation_id)
            }
            outcome => log::warn!(
                target: "app",
                "startup reconcile {}: {outcome:?}",
                report.operation_id
            ),
        }
        // Even an unsettled startup lets them run: what they change queues
        // behind the startup command.
        log_err!(client.start_background_sources());
    }

    log::trace!("init clash connection connector");
    log_err!(crate::core::clash::setup(app));

    log_err!(tauri::async_runtime::block_on(
        app.state::<crate::client::NyanpasuClient>()
            .start_clash_streams()
    ));

    let silent_start = app
        .state::<NyanpasuClient>()
        .app_config_snapshot()
        .enable_silent_start;
    if !silent_start {
        log_err!(
            app.state::<WindowManager>()
                .open(&crate::window::kinds::MainWindow, None)
        );
    }

    // test job
    proxies::setup_proxies(app.app_handle());
    crate::core::storage::register_web_storage_listener(app.app_handle());
}
