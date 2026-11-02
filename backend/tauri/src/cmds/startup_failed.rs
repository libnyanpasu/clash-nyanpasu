//! Stdin-to-EOF startup reporter adapter. No application owners are started here.
use nyanpasu_config::application::I18nLanguage;
use nyanpasu_crash_reporter::StartupFailureText;
use rust_i18n::t;
use snafu::ResultExt as _;

use crate::startup_failure::{FailureKind, Report};

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to read startup failure from stdin"))]
    ReadInput { source: std::io::Error },
    #[snafu(display("failed to decode startup failure"))]
    DecodeInput { source: serde_json::Error },
    #[snafu(display("failed to show startup failure"))]
    ShowWindow {
        source: nyanpasu_crash_reporter::ReporterError,
    },
}

pub(super) fn show(language: Option<I18nLanguage>) -> Result<(), Error> {
    read_and_show(std::io::stdin().lock(), language, |report, text| {
        nyanpasu_crash_reporter::show_startup_failure(report.input(), text)
    })
}

fn read_and_show(
    mut input: impl std::io::Read,
    language: Option<I18nLanguage>,
    show: impl FnOnce(Report, &StartupFailureText) -> Result<(), nyanpasu_crash_reporter::ReporterError>,
) -> Result<(), Error> {
    let mut json = Vec::new();
    input.read_to_end(&mut json).context(ReadInputSnafu)?;
    let report: Report = serde_json::from_slice(&json).context(DecodeInputSnafu)?;
    let locale = language
        .map(|language| language.as_str())
        .unwrap_or_else(crate::utils::help::detect_system_i18n_key);
    let text = text(report.failure.kind, locale);
    show(report, &text).context(ShowWindowSnafu)
}

pub(crate) fn text(kind: FailureKind, locale: &str) -> StartupFailureText {
    let key = match kind {
        FailureKind::Directories => "directories",
        FailureKind::MigrationFailed => "migration_failed",
        FailureKind::NewerVersion => "newer_version",
        FailureKind::UserConfig => "user_config",
        FailureKind::CoreRuntimeDirectory => "core_runtime_directory",
        FailureKind::JobsDatabase => "jobs_database",
        FailureKind::WebView2 => "web_view2",
        FailureKind::Logging => "logging",
        FailureKind::Permission => "permission",
        FailureKind::Busy => "busy",
        FailureKind::Internal => "internal",
    };
    let get = |key: &str| {
        let key = format!("dialog.startup_failure.{key}");
        t!(&key, locale = locale).into_owned()
    };
    StartupFailureText {
        heading: get("heading"),
        description: get(&format!("{key}.description")),
        recovery: get(&format!("{key}.recovery")),
        path: get("path"),
        details: get("details"),
        open_containing: get("open_containing"),
        open_backups: get("open_backups"),
        open_logs: get("open_logs"),
        copy: get("copy"),
        copied: get("copied"),
        copy_failed: get("copy_failed"),
        open_failed: get("open_failed"),
        exit: get("exit"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_to_eof_and_passes_host_language_and_report_to_window() {
        let input = br#"{"failure":{"kind":"jobs_database","path":"jobs.redb","detail":"journal failed"},"version":"test version","logs_dir":"logs","backups_dir":null}"#;
        let mut called = false;
        read_and_show(
            input.as_slice(),
            Some(I18nLanguage::SimplifiedChinese),
            |report, labels| {
                called = true;
                assert_eq!(report.failure.kind, FailureKind::JobsDatabase);
                assert_eq!(
                    report.failure.path.unwrap(),
                    std::path::PathBuf::from("jobs.redb")
                );
                assert_eq!(labels.heading, "启动失败");
                assert!(labels.recovery.contains("jobs.redb"));
                Ok(())
            },
        )
        .unwrap();
        assert!(called);
    }
}
