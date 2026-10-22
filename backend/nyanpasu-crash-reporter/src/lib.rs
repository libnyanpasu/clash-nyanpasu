//! A scrollable crash report window, run in a process separate from the panic.

mod platform;
#[cfg(test)]
mod render_tests;
mod report;
#[cfg(test)]
mod tests;

mod ui {
    slint::include_modules!();
}

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

pub use report::{CrashReport, PanicDetails};
use slint::ComponentHandle as _;
use snafu::{ResultExt as _, Snafu};

/// The window's localized strings and the issue template for the locale.
pub struct ReporterText {
    pub heading: String,
    pub description: String,
    pub version: String,
    pub system: String,
    pub error: String,
    pub location: String,
    pub report: String,
    pub copy: String,
    pub copied: String,
    pub copy_failed: String,
    pub close: String,
    pub report_issue: String,
    pub issue_opened: String,
    pub open_failed: String,
    pub issue_template: String,
}

/// Why the reporter could not start or stopped abnormally.
#[derive(Debug, Snafu)]
pub enum ReporterError {
    #[snafu(display("could not select the Slint backend"))]
    SelectBackend { source: slint::PlatformError },
    #[snafu(display("could not create the crash report window"))]
    CreateWindow { source: slint::PlatformError },
    #[snafu(display("the event loop failed"))]
    RunEventLoop { source: slint::PlatformError },
}

/// Shows the window and returns once the user closes it. Call once per process, on the main thread.
pub fn show_crash_report(report: &CrashReport, text: &ReporterText) -> Result<(), ReporterError> {
    // The window to centre once the event loop starts.
    let opening = Rc::new(Cell::new(None::<slint::Weak<ui::ReporterWindow>>));
    platform::select_backend({
        let opening = opening.clone();
        move |event_loop| {
            if let Some(window) = opening.take().and_then(|weak| weak.upgrade()) {
                let size = slint::LogicalSize::new(
                    window.get_opening_width(),
                    window.get_opening_height(),
                );
                platform::centre(window.window(), size, event_loop);
            }
        }
    })
    .context(SelectBackendSnafu)?;
    let window = ui::ReporterWindow::new().context(CreateWindowSnafu)?;
    opening.set(Some(window.as_weak()));
    fill_window(&window, report, text);

    // One clipboard for the window's lifetime: on X11 this process serves the
    // copied text, which disappears with the `Clipboard`.
    let clipboard = RefCell::new(arboard::Clipboard::new().ok());
    let report_text = report.text();
    let copy = Rc::new(move || {
        clipboard
            .borrow_mut()
            .as_mut()
            .is_some_and(|clipboard| clipboard.set_text(report_text.as_str()).is_ok())
    });

    let timer = Rc::new(slint::Timer::default());
    let weak = window.as_weak();
    let copy_report = copy.clone();
    let copy_timer = timer.clone();
    let copied = text.copied.clone();
    let copy_failed = text.copy_failed.clone();
    window.on_copy_report(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !copy_report() {
            copy_timer.stop();
            window.set_status(copy_failed.as_str().into());
            return;
        }
        window.set_status(copied.as_str().into());
        let weak = window.as_weak();
        copy_timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_secs(3),
            move || {
                if let Some(window) = weak.upgrade() {
                    window.set_status("".into());
                }
            },
        );
    });

    let weak = window.as_weak();
    let url = report.issue_url(&text.issue_template);
    let opened = text.issue_opened.clone();
    let open_failed = text.open_failed.clone();
    let copy_failed = text.copy_failed.clone();
    window.on_report_issue(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        timer.stop();
        let copied = copy();
        let status = if !url.as_ref().is_some_and(|url| open::that(url).is_ok()) {
            &open_failed
        } else if !copied {
            &copy_failed
        } else {
            &opened
        };
        window.set_status(status.as_str().into());
    });
    window.on_close_report(|| {
        let _ = slint::quit_event_loop();
    });
    window.run().context(RunEventLoopSnafu)
}

fn fill_window(window: &ui::ReporterWindow, report: &CrashReport, text: &ReporterText) {
    window.set_app(report.app.as_str().into());
    window.set_os(report.os.as_str().into());
    window.set_payload(report.payload().into());
    window.set_location(report.location().into());
    window.set_report_text(report.text().into());
    window.set_monospace_font(platform::monospace_font().into());

    let strings = window.global::<ui::ReporterStrings>();
    strings.set_heading(text.heading.as_str().into());
    strings.set_description(text.description.as_str().into());
    strings.set_version(text.version.as_str().into());
    strings.set_system(text.system.as_str().into());
    strings.set_error(text.error.as_str().into());
    strings.set_location(text.location.as_str().into());
    strings.set_report(text.report.as_str().into());
    strings.set_copy(text.copy.as_str().into());
    strings.set_close(text.close.as_str().into());
    strings.set_report_issue(text.report_issue.as_str().into());
}
