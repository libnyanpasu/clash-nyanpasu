use crate::{
    client::{NyanpasuClient, application_workflow::startup::StartupOutcome},
    core::tray::proxies,
    log_err,
    window::WindowManager,
};
use tauri::{App, Manager};

#[cfg(target_os = "macos")]
fn set_window_controls_pos(
    window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
    x: f64,
    y: f64,
) -> anyhow::Result<()> {
    use objc2_app_kit::NSWindowButton;
    use objc2_foundation::NSRect;
    let close = window
        .standardWindowButton(NSWindowButton::CloseButton)
        .ok_or(anyhow::anyhow!("failed to get close button"))?;
    let miniaturize = window
        .standardWindowButton(NSWindowButton::MiniaturizeButton)
        .ok_or(anyhow::anyhow!("failed to get miniaturize button"))?;
    let zoom = window
        .standardWindowButton(NSWindowButton::ZoomButton)
        .ok_or(anyhow::anyhow!("failed to get zoom button"))?;

    let title_bar_container_view = unsafe {
        close
            .superview()
            .and_then(|view| view.superview())
            .ok_or(anyhow::anyhow!("failed to get title bar container view"))?
    };

    let close_rect = close.frame();
    let button_height = close_rect.size.height;

    let title_bar_frame_height = button_height + y;
    let mut title_bar_rect = title_bar_container_view.frame();
    title_bar_rect.size.height = title_bar_frame_height;
    title_bar_rect.origin.y = window.frame().size.height - title_bar_frame_height;
    unsafe {
        title_bar_container_view.setFrame(title_bar_rect);
    }

    let space_between = miniaturize.frame().origin.x - close.frame().origin.x;
    let window_buttons = vec![close, miniaturize, zoom];

    for (i, button) in window_buttons.into_iter().enumerate() {
        let mut rect: NSRect = button.frame();
        rect.origin.x = x + (i as f64 * space_between);
        unsafe {
            button.setFrameOrigin(rect.origin);
        }
    }
    Ok(())
}

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
