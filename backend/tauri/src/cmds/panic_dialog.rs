use std::io::{BufRead as _, Read as _};

use nyanpasu_config::application::I18nLanguage;
use nyanpasu_crash_reporter::{
    CrashReport, PanicDetails, ReporterInput, ReporterText, show_report,
};
use rust_i18n::t;
use sysinfo::System;

use crate::{consts::BUILD_INFO, utils};

pub(super) fn show(
    paths: &nyanpasu_paths::PathResolver,
    manual: bool,
    language: Option<I18nLanguage>,
) -> anyhow::Result<()> {
    let locale = language
        .map(|language| language.as_str())
        .unwrap_or_else(utils::help::detect_system_i18n_key);
    rust_i18n::set_locale(locale);
    let (sender, events) = std::sync::mpsc::channel();
    let mut input = Vec::new();
    if !manual {
        std::io::stdin().read_to_end(&mut input)?;
    }
    let details = serde_json::from_slice(&input).unwrap_or_else(|_| PanicDetails {
        parent_pid: 0,
        payload: None,
        location: None,
        backtrace: String::from_utf8_lossy(&input).into_owned(),
    });
    let report = CrashReport {
        app: format!("{} ({})", BUILD_INFO.pkg_version, BUILD_INFO.commit_hash),
        os: format!(
            "{} {} ({})",
            System::long_os_version().unwrap_or_else(|| "<unknown>".into()),
            System::kernel_version().unwrap_or_default(),
            System::cpu_arch(),
        ),
        panic: details,
    };
    let text = ReporterText {
        manual_heading: t!("dialog.crash_reporter.manual_heading").into_owned(),
        manual_description: t!("dialog.crash_reporter.manual_description").into_owned(),
        collecting: t!("dialog.crash_reporter.collecting").into_owned(),
        steps: [
            t!("dialog.crash_reporter.step_host").into_owned(),
            t!("dialog.crash_reporter.step_webview").into_owned(),
            t!("dialog.crash_reporter.step_core").into_owned(),
            t!("dialog.crash_reporter.step_app_logs").into_owned(),
            t!("dialog.crash_reporter.step_core_logs").into_owned(),
        ],
        ready: t!("dialog.crash_reporter.ready").into_owned(),
        failed: t!("dialog.crash_reporter.failed").into_owned(),
        interrupted: t!("dialog.crash_reporter.interrupted").into_owned(),
        show_in_folder: t!("dialog.crash_reporter.show_in_folder").into_owned(),

        heading: t!("dialog.crash_reporter.heading").into_owned(),
        description: t!("dialog.crash_reporter.description").into_owned(),
        version: t!("dialog.crash_reporter.version").into_owned(),
        system: t!("dialog.crash_reporter.system").into_owned(),
        error: t!("dialog.crash_reporter.error").into_owned(),
        location: t!("dialog.crash_reporter.location").into_owned(),
        backtrace: t!("dialog.crash_reporter.backtrace").into_owned(),
        environment: t!("dialog.crash_reporter.environment").into_owned(),
        copy: t!("dialog.crash_reporter.copy").into_owned(),
        copied: t!("dialog.crash_reporter.copied").into_owned(),
        copy_section: t!("dialog.crash_reporter.copy_section").into_owned(),
        section_copied: t!("dialog.crash_reporter.section_copied").into_owned(),
        copy_failed: t!("dialog.crash_reporter.copy_failed").into_owned(),
        close: t!("dialog.crash_reporter.close").into_owned(),
        report_issue: t!("dialog.crash_reporter.report_issue").into_owned(),
        issue_opened: t!("dialog.crash_reporter.issue_opened").into_owned(),
        open_failed: t!("dialog.crash_reporter.open_failed").into_owned(),
        issue_template: if locale.starts_with("zh") {
            "bug_report.zh.yaml"
        } else {
            "bug_report.yaml"
        }
        .into(),
    };
    let input = if manual {
        ReporterInput::Manual {
            app: report.app,
            os: report.os,
        }
    } else {
        ReporterInput::Crash(report)
    };
    if manual {
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else {
                    break;
                };
                let Ok(event) = serde_json::from_str(&line) else {
                    break;
                };
                if sender.send(event).is_err() {
                    break;
                }
            }
        });
    } else {
        let paths = paths.clone();
        let around = (input.head(), input.tail());
        let parent_pid = match &input {
            ReporterInput::Crash(report) => report.panic.parent_pid,
            _ => 0,
        };
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("diagnosis runtime");
            let _guard = runtime.enter();
            utils::collect::diagnose(
                &paths,
                None,
                Default::default(),
                around,
                parent_pid,
                true,
                |event| {
                    let _ = sender.send(event);
                },
            );
        });
    }
    let fallback = input.text();
    if let Err(error) = show_report(input, &text, events) {
        tracing::error!(?error, "Failed to show the crash reporter");
        let report = fallback;
        let lines: Vec<_> = report.lines().collect();
        let mut message = lines
            .iter()
            .take(40)
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        if lines.len() > 40 {
            message.push_str(&format!("\n… ({} more lines)", lines.len() - 40));
        }
        utils::dialog::panic_dialog(&message);
    }
    Ok(())
}
