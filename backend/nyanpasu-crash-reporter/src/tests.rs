use std::collections::HashMap;

use crate::{CrashReport, PanicDetails};

fn report(backtrace: &str) -> CrashReport {
    CrashReport {
        app: "2.0.0-alpha (e8fe65e31)".into(),
        os: "Windows 11 Pro (x86_64)".into(),
        panic: PanicDetails {
            parent_pid: 123,
            payload: Some("index out of bounds\nsecond line & more".into()),
            location: Some("src/catalog.rs:505:9".into()),
            backtrace: backtrace.into(),
        },
    }
}

#[test]
fn report_text_preserves_the_backtrace() {
    assert_eq!(
        report("  0: app.exe+0x123\r\n  1: app.exe+0x456\n").text(),
        "Clash Nyanpasu 2.0.0-alpha (e8fe65e31)\nOS: Windows 11 Pro (x86_64)\nPayload: index out of bounds\nsecond line & more\nLocation: src/catalog.rs:505:9\nBacktrace:\n  0: app.exe+0x123\r\n  1: app.exe+0x456\n",
    );
}

#[test]
fn missing_panic_fields_have_explicit_placeholders() {
    let mut report = report("");
    report.panic.payload = None;
    report.panic.location = None;
    assert_eq!(
        report.text(),
        "Clash Nyanpasu 2.0.0-alpha (e8fe65e31)\nOS: Windows 11 Pro (x86_64)\nPayload: <non-string payload>\nLocation: <unknown>\nBacktrace:\n",
    );
}

fn fields(url: &str) -> HashMap<String, String> {
    let url = url::Url::parse(url).unwrap();
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.host_str(), Some("github.com"));
    assert_eq!(url.path(), "/libnyanpasu/clash-nyanpasu/issues/new");
    url.query_pairs().into_owned().collect()
}

#[test]
fn short_issue_round_trips_all_fields() {
    let fields = fields(
        &report("frame 0\nframe 1")
            .issue_url("bug_report.zh.yaml")
            .unwrap(),
    );
    assert_eq!(fields.len(), 5);
    assert_eq!(fields["template"], "bug_report.zh.yaml");
    assert_eq!(fields["title"], "[Crash] index out of bounds");
    assert_eq!(
        fields["actual"],
        "The application crashed.\n\n```\nPayload: index out of bounds\nsecond line & more\nLocation: src/catalog.rs:505:9\n```",
    );
    assert_eq!(
        fields["env_infos"],
        "> App: 2.0.0-alpha (e8fe65e31)\n> OS: Windows 11 Pro (x86_64)",
    );
    assert_eq!(fields["log"], "```text\nframe 0\nframe 1\n```");
}

#[test]
fn long_issue_keeps_only_a_prefix_of_whole_frames() {
    let backtrace = (0..500)
        .map(|frame| format!("{frame:4}: clash-nyanpasu.exe+0x{:x}\n", frame * 1000))
        .collect::<String>();
    let url = report(&backtrace).issue_url("bug_report.yaml").unwrap();
    assert!(url.len() <= 6000, "URL has {} bytes", url.len());
    let fields = fields(&url);
    let lines: Vec<_> = fields["log"].lines().collect();
    let kept = &lines[1..lines.len() - 2];
    assert!(!kept.is_empty());
    assert!(kept.len() < 500);
    assert_eq!(
        kept,
        &backtrace.lines().take(kept.len()).collect::<Vec<_>>()
    );
    assert_eq!(
        lines[lines.len() - 2],
        format!(
            "... {} more lines: the full report is on the clipboard",
            500 - kept.len(),
        ),
    );
    assert_eq!(lines.last(), Some(&"```"));
}

#[test]
fn issue_title_is_cut_on_a_multibyte_character_boundary() {
    let mut report = report("");
    report.panic.payload = Some("崩溃🦀".repeat(100));
    let fields = fields(&report.issue_url("bug_report.yaml").unwrap());
    assert_eq!(fields["title"].chars().count(), 80);
    assert_eq!(
        fields["title"],
        format!("[Crash] {}", report.payload())
            .chars()
            .take(80)
            .collect::<String>(),
    );
}

#[test]
fn panic_details_json_round_trip() {
    for sample_payload in [None, Some("panic\n中文 & 🦀".into())] {
        let details = PanicDetails {
            parent_pid: 123,
            payload: sample_payload,
            location: None,
            backtrace: "frame 0\r\nframe 1".into(),
        };
        let json = serde_json::to_vec(&details).unwrap();
        let decoded: PanicDetails = serde_json::from_slice(&json).unwrap();
        assert_eq!(decoded.payload, details.payload);
        assert_eq!(decoded.location, details.location);
        assert_eq!(decoded.backtrace, details.backtrace);
    }
}

#[test]
fn oversized_metadata_makes_the_issue_url_unavailable() {
    let mut report = report("frame 0");
    report.panic.payload = Some("崩溃".repeat(1000));
    assert!(report.issue_url("bug_report.yaml").is_none());
    assert!(report.text().contains(report.payload()));
}

#[test]
fn manual_issue_has_environment_without_crash_fields() {
    let input = crate::ReporterInput::Manual {
        app: "test".into(),
        os: "test".into(),
    };
    let fields = fields(
        &input
            .issue_url(
                "bug_report.yaml",
                "----------- System -----------\nOS: Test",
            )
            .unwrap(),
    );
    assert_eq!(fields["title"], "");
    assert!(!fields.contains_key("actual"));
    assert!(!fields.contains_key("log"));
    assert_eq!(
        fields["env_infos"],
        "> ----------- System -----------\n> OS: Test"
    );
}

#[test]
fn crash_issue_puts_the_environment_in_its_own_field() {
    let input = crate::ReporterInput::Crash(report("  0: app.exe+0x123\n"));
    let fields = fields(
        &input
            .issue_url(
                "bug_report.yaml",
                "----------- System -----------\nOS: Test\n",
            )
            .unwrap(),
    );
    assert_eq!(
        fields["env_infos"],
        "> ----------- System -----------\n> OS: Test"
    );
    assert!(fields["log"].contains("0: app.exe+0x123"));
}
