//! Presentation of a startup failure; the host supplies paths and recovery policy.
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use slint::ComponentHandle as _;
use snafu::ResultExt as _;

use crate::{
    CreateWindowSnafu, ReporterError, RunEventLoopSnafu, SelectBackendSnafu, platform, ui,
};

/// Display data and optional folder actions, resolved by the host.
pub struct StartupFailureInput {
    pub version: String,
    pub path: Option<PathBuf>,
    pub details: String,
    pub containing_dir: Option<PathBuf>,
    pub backups_dir: Option<PathBuf>,
    pub logs_dir: Option<PathBuf>,
}

/// Localized startup text supplied by the host, independently of the OS language.
pub struct StartupFailureText {
    pub heading: String,
    pub description: String,
    pub recovery: String,
    pub path: String,
    pub details: String,
    pub open_containing: String,
    pub open_backups: String,
    pub open_logs: String,
    pub copy: String,
    pub copied: String,
    pub copy_failed: String,
    pub open_failed: String,
    pub exit: String,
}

/// Blocks until the window closes. Call once per process on its main thread.
///
/// # Errors
/// Returns backend, window construction or event loop failures to the host fallback.
pub fn show_startup_failure(
    input: StartupFailureInput,
    text: &StartupFailureText,
) -> Result<(), ReporterError> {
    let opening = Rc::new(Cell::new(None::<slint::Weak<ui::StartupFailureWindow>>));
    platform::select_backend({
        let opening = opening.clone();
        move |event_loop| {
            if let Some(window) = opening.take().and_then(|weak| weak.upgrade()) {
                platform::centre(
                    window.window(),
                    slint::LogicalSize::new(
                        window.get_opening_width(),
                        window.get_opening_height(),
                    ),
                    event_loop,
                );
            }
        }
    })
    .context(SelectBackendSnafu)?;
    platform::configure_fonts();
    let window = ui::StartupFailureWindow::new().context(CreateWindowSnafu)?;
    opening.set(Some(window.as_weak()));
    fill_window(&window, &input, text);
    // Retain the X11 clipboard owner until the window closes.
    let clipboard = RefCell::new(arboard::Clipboard::new().ok());
    bind_actions(
        &window,
        input,
        text,
        move |text| {
            clipboard
                .borrow_mut()
                .as_mut()
                .is_some_and(|clipboard| clipboard.set_text(text).is_ok())
        },
        |path| open::that(path).is_ok(),
        || {
            let _ = slint::quit_event_loop();
        },
    );
    window.run().context(RunEventLoopSnafu)
}

pub(crate) fn fill_window(
    window: &ui::StartupFailureWindow,
    input: &StartupFailureInput,
    text: &StartupFailureText,
) {
    window.set_heading(text.heading.as_str().into());
    window.set_description(text.description.as_str().into());
    window.set_recovery(text.recovery.as_str().into());
    window.set_version(input.version.as_str().into());
    window.set_path(
        input
            .path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
            .into(),
    );
    window.set_path_label(text.path.as_str().into());
    window.set_details_label(text.details.as_str().into());
    window.set_details(input.details.as_str().into());
    window.set_monospace_font(platform::monospace_font().into());
    window.set_containing_label(text.open_containing.as_str().into());
    window.set_backups_label(text.open_backups.as_str().into());
    window.set_logs_label(text.open_logs.as_str().into());
    window.set_copy_label(text.copy.as_str().into());
    window.set_exit_label(text.exit.as_str().into());
    window.set_has_containing(input.containing_dir.is_some());
    window.set_has_backups(input.backups_dir.is_some());
    window.set_has_logs(input.logs_dir.is_some());
}

pub(crate) fn bind_actions(
    window: &ui::StartupFailureWindow,
    input: StartupFailureInput,
    text: &StartupFailureText,
    copy: impl Fn(&str) -> bool + 'static,
    open: impl Fn(&std::path::Path) -> bool + 'static,
    exit: impl Fn() + 'static,
) {
    let weak = window.as_weak();
    let copied = text.copied.clone();
    let failed = text.copy_failed.clone();
    window.on_copy_details(move || {
        if let Some(window) = weak.upgrade() {
            let success = copy(&input.details);
            window.set_status_is_error(!success);
            window.set_status(if success { &copied } else { &failed }.as_str().into());
        }
    });
    let open = Rc::new(open);
    let folder_action = |path: Option<PathBuf>| {
        let weak = window.as_weak();
        let open = open.clone();
        let failed = text.open_failed.clone();
        move || {
            if let Some(path) = &path
                && let Some(window) = weak.upgrade()
            {
                let success = open(path);
                window.set_status_is_error(!success);
                window.set_status(if success { "" } else { failed.as_str() }.into());
            }
        }
    };
    window.on_open_containing(folder_action(input.containing_dir));
    window.on_open_backups(folder_action(input.backups_dir));
    window.on_open_logs(folder_action(input.logs_dir));
    window.on_exit(exit);
}
