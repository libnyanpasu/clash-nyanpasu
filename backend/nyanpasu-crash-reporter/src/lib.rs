//! A scrollable crash report window, run in a process separate from the panic.

mod platform;
#[cfg(test)]
mod render_tests;
mod report;
mod startup_failure;
#[cfg(test)]
mod startup_failure_render_tests;
pub use startup_failure::{StartupFailureInput, StartupFailureText, show_startup_failure};
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

use nyanpasu_diagnose::{Event, Step};
pub use report::{CrashReport, PanicDetails, ReporterInput};
use slint::ComponentHandle as _;
use snafu::{ResultExt as _, Snafu};
use std::{path::PathBuf, sync::mpsc::Receiver};

/// The window's localized strings and the issue template for the locale.
pub struct ReporterText {
    pub heading: String,
    pub description: String,
    pub version: String,
    pub system: String,
    pub error: String,
    pub location: String,
    pub backtrace: String,
    pub environment: String,
    pub copy: String,
    pub copied: String,
    pub copy_section: String,
    pub section_copied: String,
    pub copy_failed: String,
    pub close: String,
    pub report_issue: String,
    pub issue_opened: String,
    pub open_failed: String,
    pub issue_template: String,
    pub manual_heading: String,
    pub manual_description: String,
    pub collecting: String,
    pub steps: [String; 5],
    pub ready: String,
    pub failed: String,
    pub interrupted: String,
    pub show_in_folder: String,
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
pub fn show_report(
    input: ReporterInput,
    text: &ReporterText,
    events: Receiver<Event>,
) -> Result<(), ReporterError> {
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
    platform::configure_fonts();
    let window = ui::ReporterWindow::new().context(CreateWindowSnafu)?;
    opening.set(Some(window.as_weak()));
    fill_window(&window, &input, text);

    // One clipboard for the window's lifetime: on X11 this process serves the
    // copied text, which disappears with the `Clipboard`.
    let clipboard = RefCell::new(arboard::Clipboard::new().ok());
    let copy = Rc::new(move |text: &str| {
        clipboard
            .borrow_mut()
            .as_mut()
            .is_some_and(|clipboard| clipboard.set_text(text).is_ok())
    });

    // Shows a copy's outcome for 3 seconds.
    let timer = Rc::new(slint::Timer::default());
    let show_copied = {
        let timer = timer.clone();
        let copy_failed = text.copy_failed.clone();
        move |window: &ui::ReporterWindow, copied: bool, message: &str| {
            if !copied {
                timer.stop();
                window.set_status(copy_failed.as_str().into());
                return;
            }
            window.set_status(message.into());
            let weak = window.as_weak();
            timer.start(
                slint::TimerMode::SingleShot,
                Duration::from_secs(3),
                move || {
                    if let Some(window) = weak.upgrade() {
                        window.set_status("".into());
                    }
                },
            );
        }
    };
    let show_copied = Rc::new(show_copied);

    // Callbacks read the report, the environment and the archive from the
    // window, which holds them as the diagnosis delivers them.
    let weak = window.as_weak();
    let (copy_report, show) = (copy.clone(), show_copied.clone());
    let copied = text.copied.clone();
    window.on_copy_report(move || {
        if let Some(window) = weak.upgrade() {
            show(&window, copy_report(&window.get_report_text()), &copied);
        }
    });
    let weak = window.as_weak();
    let (copy_section, show) = (copy.clone(), show_copied);
    let section_copied = text.section_copied.clone();
    window.on_copy_section(move |section| {
        if let Some(window) = weak.upgrade() {
            show(&window, copy_section(&section), &section_copied);
        }
    });

    let weak = window.as_weak();
    let issue_template = text.issue_template.clone();
    let opened = text.issue_opened.clone();
    let open_failed = text.open_failed.clone();
    let copy_failed = text.copy_failed.clone();
    window.on_report_issue(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        timer.stop();
        let copied = copy(&window.get_report_text());
        let url = input.issue_url(&issue_template, &window.get_environment_text());
        let status = if !url.as_ref().is_some_and(|url| open::that(url).is_ok()) {
            &open_failed
        } else if !copied {
            &copy_failed
        } else {
            &opened
        };
        if let Some(path) = archive_path(&window) {
            let _ = opener::reveal(&path);
        }
        window.set_status(status.as_str().into());
    });
    window.on_close_report(|| {
        let _ = slint::quit_event_loop();
    });
    let weak = window.as_weak();
    let reveal_failed = text.open_failed.clone();
    window.on_show_archive(move || {
        if let Some(window) = weak.upgrade()
            && let Some(path) = archive_path(&window)
            && opener::reveal(&path).is_err()
        {
            window.set_status(reveal_failed.as_str().into());
        }
    });
    let weak = window.as_weak();
    let labels = EventText::from(text);
    std::thread::spawn(move || {
        let mut steps = Vec::new();
        let mut finished = false;
        while let Ok(event) = events.recv() {
            finished = matches!(event, Event::Finished { .. });
            let weak = weak.clone();
            let labels = labels.clone();
            if let Event::Started { steps: order } = &event {
                steps = order.clone();
            }
            let order = steps.clone();
            if slint::invoke_from_event_loop(move || {
                if let Some(window) = weak.upgrade() {
                    apply_event(&window, event, &order, &labels);
                }
            })
            .is_err()
            {
                return;
            }
            if finished {
                break;
            }
        }
        if !finished {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(window) = weak.upgrade() {
                    window.set_progress_label(labels.interrupted.into());
                }
            });
        }
    });
    window.run().context(RunEventLoopSnafu)
}

fn archive_path(window: &ui::ReporterWindow) -> Option<PathBuf> {
    let path = window.get_archive_path();
    (!path.is_empty()).then(|| PathBuf::from(path.as_str()))
}

fn fill_window(window: &ui::ReporterWindow, input: &ReporterInput, text: &ReporterText) {
    let (app, os) = match input {
        ReporterInput::Crash(report) => (&report.app, &report.os),
        ReporterInput::Manual { app, os } => (app, os),
    };
    window.set_app(app.as_str().into());
    window.set_os(os.as_str().into());
    window.set_is_crash(matches!(input, ReporterInput::Crash(_)));
    if let ReporterInput::Crash(report) = input {
        window.set_payload(report.payload().into());
        window.set_location(report.location().into());
    }
    window.set_report_text(input.text().into());
    window.set_backtrace_text(input.text().into());
    window.set_environment_text(format!("{}…", text.collecting).into());
    window.set_finished(false);
    window.set_has_problems(false);
    window.set_archive_path("".into());
    window.set_progress(0.);
    window.set_progress_label(text.collecting.as_str().into());
    window.set_monospace_font(platform::monospace_font().into());

    let strings = window.global::<ui::ReporterStrings>();
    strings.set_heading(
        if matches!(input, ReporterInput::Crash(_)) {
            &text.heading
        } else {
            &text.manual_heading
        }
        .as_str()
        .into(),
    );
    strings.set_description(
        if matches!(input, ReporterInput::Crash(_)) {
            &text.description
        } else {
            &text.manual_description
        }
        .as_str()
        .into(),
    );
    strings.set_version(text.version.as_str().into());
    strings.set_system(text.system.as_str().into());
    strings.set_error(text.error.as_str().into());
    strings.set_location(text.location.as_str().into());
    strings.set_backtrace(text.backtrace.as_str().into());
    strings.set_environment(text.environment.as_str().into());
    strings.set_copy(text.copy.as_str().into());
    strings.set_copy_section(text.copy_section.as_str().into());
    strings.set_close(text.close.as_str().into());
    strings.set_report_issue(text.report_issue.as_str().into());
    strings.set_show_in_folder(text.show_in_folder.as_str().into());
}

#[derive(Clone)]
struct EventText {
    collecting: String,
    steps: [String; 5],
    ready: String,
    failed: String,
    interrupted: String,
}
impl From<&ReporterText> for EventText {
    fn from(text: &ReporterText) -> Self {
        Self {
            collecting: text.collecting.clone(),
            steps: text.steps.clone(),
            ready: text.ready.clone(),
            failed: text.failed.clone(),
            interrupted: text.interrupted.clone(),
        }
    }
}
fn apply_event(window: &ui::ReporterWindow, event: Event, steps: &[Step], text: &EventText) {
    match event {
        Event::Started { .. } => {
            window.set_finished(false);
            window.set_progress(0.);
        }
        Event::Progress { step, fraction } => {
            let index = steps.iter().position(|value| *value == step).unwrap_or(0);
            window
                .set_progress((index as f32 + fraction.clamp(0., 1.)) / steps.len().max(1) as f32);
            let label = &text.steps[match step {
                Step::Host => 0,
                Step::Webview => 1,
                Step::Core => 2,
                Step::AppLogs => 3,
                Step::CoreLogs => 4,
            }];
            window.set_progress_label(format!("{}: {label}", text.collecting).into());
        }
        Event::Finished {
            report,
            environment,
            archive,
            problems,
        } => {
            window.set_report_text(report.into());
            window.set_environment_text(environment.into());
            window.set_finished(true);
            window.set_progress(1.);
            window.set_has_problems(!problems.is_empty());
            let size = archive
                .as_ref()
                .and_then(|path| std::fs::metadata(path).ok())
                .map(|m| nyanpasu_diagnose::report::format_bytes(m.len()));
            let label = if problems.is_empty() {
                format!(
                    "{}{}",
                    text.ready,
                    size.map(|size| format!(" · {size}")).unwrap_or_default()
                )
            } else {
                format!("⚠ {} ({})", text.failed, problems.len())
            };
            window.set_progress_label(label.into());
            window.set_archive_path(
                archive
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .into(),
            );
        }
    }
}
