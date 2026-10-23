use serde::{Deserialize, Serialize};

/// What the panicking process hands to the reporter process, as JSON on its stdin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanicDetails {
    pub parent_pid: u32,
    pub payload: Option<String>,
    pub location: Option<String>,
    pub backtrace: String,
}

/// Everything the window shows and the issue carries.
#[derive(Clone)]
pub struct CrashReport {
    /// "<version> (<commit>)".
    pub app: String,
    /// "<long OS version> (<arch>)".
    pub os: String,
    pub panic: PanicDetails,
}

impl CrashReport {
    pub(crate) fn payload(&self) -> &str {
        self.panic
            .payload
            .as_deref()
            .unwrap_or("<non-string payload>")
    }

    pub(crate) fn location(&self) -> &str {
        self.panic.location.as_deref().unwrap_or("<unknown>")
    }

    /// The plain-text report: shown in the window, copied, and put in the issue.
    /// The diagnosis places the environment between the head and the tail.
    pub fn text(&self) -> String {
        self.head() + &self.tail()
    }

    /// What failed and where, read first.
    pub fn head(&self) -> String {
        format!(
            "Clash Nyanpasu {}\nOS: {}\nPayload: {}\nLocation: {}\n",
            self.app,
            self.os,
            self.payload(),
            self.location(),
        )
    }

    /// The backtrace is long, so it comes last.
    pub fn tail(&self) -> String {
        format!("Backtrace:\n{}", self.panic.backtrace)
    }

    /// The prefilled bug report URL, dropping whole backtrace lines to fit 6000 bytes.
    /// None if the other fields alone exceed the budget.
    pub fn issue_url(&self, template: &str) -> Option<String> {
        self.issue_url_with_environment(
            template,
            &format!("> App: {}\n> OS: {}", self.app, self.os),
        )
    }

    fn issue_url_with_environment(&self, template: &str, environment: &str) -> Option<String> {
        let title: String = format!(
            "[Crash] {}",
            self.payload().lines().next().unwrap_or_default()
        )
        .chars()
        .take(80)
        .collect();
        let actual = format!(
            "The application crashed.\n\n```\nPayload: {}\nLocation: {}\n```",
            self.payload(),
            self.location(),
        );
        let lines: Vec<&str> = self.panic.backtrace.split_inclusive('\n').collect();
        for kept in (0..=lines.len()).rev() {
            let mut log = format!("```text\n{}", lines[..kept].concat());
            if !log.ends_with('\n') {
                log.push('\n');
            }
            if kept < lines.len() {
                log.push_str(&format!(
                    "... {} more lines: the full report is on the clipboard\n",
                    lines.len() - kept,
                ));
            }
            log.push_str("```");
            let mut url =
                url::Url::parse("https://github.com/libnyanpasu/clash-nyanpasu/issues/new")
                    .expect("the issue URL is valid");
            url.query_pairs_mut().extend_pairs([
                ("template", template),
                ("title", title.as_str()),
                ("actual", actual.as_str()),
                ("env_infos", environment),
                ("log", log.as_str()),
            ]);
            if url.as_str().len() <= 6000 {
                return Some(url.into());
            }
        }
        None
    }
}

/// The same frontend serves a panic or a manually initiated issue report.
pub enum ReporterInput {
    Crash(CrashReport),
    Manual { app: String, os: String },
}
impl ReporterInput {
    pub fn text(&self) -> String {
        self.head() + &self.tail()
    }
    pub fn head(&self) -> String {
        match self {
            Self::Crash(report) => report.head(),
            Self::Manual { .. } => String::new(),
        }
    }
    pub fn tail(&self) -> String {
        match self {
            Self::Crash(report) => report.tail(),
            Self::Manual { .. } => String::new(),
        }
    }
    pub(crate) fn issue_url(&self, template: &str, environment: &str) -> Option<String> {
        let environment = environment
            .trim()
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        match self {
            Self::Crash(crash) => crash.issue_url_with_environment(template, &environment),
            Self::Manual { .. } => {
                let mut url =
                    url::Url::parse("https://github.com/libnyanpasu/clash-nyanpasu/issues/new")
                        .expect("the issue URL is valid");
                url.query_pairs_mut().extend_pairs([
                    ("template", template),
                    ("title", ""),
                    ("env_infos", environment.as_str()),
                ]);
                (url.as_str().len() <= 6000).then(|| url.into())
            }
        }
    }
}
