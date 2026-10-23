use std::io::Read as _;

use nyanpasu_crash_reporter::{CrashReport, PanicDetails, ReporterText, show_crash_report};
use rust_i18n::t;
use sysinfo::System;

use crate::{consts::BUILD_INFO, utils};

pub(super) fn show() -> anyhow::Result<()> {
    let locale = utils::help::detect_system_i18n_key();
    rust_i18n::set_locale(locale);
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    let details = serde_json::from_slice(&input).unwrap_or_else(|_| PanicDetails {
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
        heading: t!("dialog.crash_reporter.heading").into_owned(),
        description: t!("dialog.crash_reporter.description").into_owned(),
        version: t!("dialog.crash_reporter.version").into_owned(),
        system: t!("dialog.crash_reporter.system").into_owned(),
        error: t!("dialog.crash_reporter.error").into_owned(),
        location: t!("dialog.crash_reporter.location").into_owned(),
        report: t!("dialog.crash_reporter.report").into_owned(),
        copy: t!("dialog.crash_reporter.copy").into_owned(),
        copied: t!("dialog.crash_reporter.copied").into_owned(),
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
    if let Err(error) = show_crash_report(&report, &text) {
        tracing::error!(?error, "Failed to show the crash reporter");
        let report = report.text();
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
