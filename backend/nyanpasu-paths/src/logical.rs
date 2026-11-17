//! Platform-neutral keys for items that leave the device.
//!
//! A [`LogicalPath`] is validated by string rules only and never goes through
//! `std::path`, so the same key is valid, and names the same item, on every
//! platform. This module contains no platform-conditional code.

use std::{fmt, str::FromStr};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use snafu::{Snafu, ensure};

const MAX_SEGMENTS: usize = 4;
const MAX_LENGTH: usize = 255;
const MAX_SEGMENT_LENGTH: usize = 128;

/// A slash-separated relative key, identical on every platform.
///
/// One to four segments. A segment starts with an ASCII letter or digit,
/// continues with letters, digits, `.`, `_` or `-`, does not end with `.`, and
/// its stem is not a Windows device name. Keys are compared by bytes; use
/// [`LogicalPath::case_folded`] to find names a case-insensitive filesystem
/// cannot hold together.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LogicalPath(String);

/// Why a string is not a [`LogicalPath`].
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
pub enum LogicalPathError {
    #[snafu(display("a logical path has one to {MAX_SEGMENTS} segments, found {count}"))]
    SegmentCount { count: usize },
    #[snafu(display("a logical path is at most {MAX_LENGTH} bytes, found {length}"))]
    TooLong { length: usize },
    #[snafu(display("segment {segment:?} is empty or longer than {MAX_SEGMENT_LENGTH} bytes"))]
    SegmentLength { segment: String },
    #[snafu(display("segment {segment:?} contains a character outside [A-Za-z0-9._-]"))]
    SegmentCharacter { segment: String },
    #[snafu(display("segment {segment:?} must start with a letter or digit"))]
    SegmentStart { segment: String },
    #[snafu(display("segment {segment:?} must not end with a dot"))]
    SegmentTrailingDot { segment: String },
    #[snafu(display("segment {segment:?} is a reserved device name on Windows"))]
    ReservedName { segment: String },
}

impl LogicalPath {
    /// Validates `value` as a logical path.
    ///
    /// # Errors
    ///
    /// Returns the first rule the value breaks.
    pub fn parse(value: &str) -> Result<Self, LogicalPathError> {
        ensure!(
            value.len() <= MAX_LENGTH,
            TooLongSnafu {
                length: value.len()
            }
        );
        let count = value.split('/').count();
        ensure!(count <= MAX_SEGMENTS, SegmentCountSnafu { count });
        for segment in value.split('/') {
            validate_segment(segment)?;
        }
        Ok(Self(value.to_owned()))
    }

    /// Appends one segment, as in `prefix.join("name.yaml")`.
    ///
    /// # Errors
    ///
    /// Fails when the segment or the resulting path breaks a rule.
    pub fn join(&self, segment: &str) -> Result<Self, LogicalPathError> {
        Self::parse(&format!("{}/{segment}", self.0))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }

    /// The key under ASCII case folding. Two paths with equal folded keys
    /// collide on Windows and on a default macOS volume.
    #[must_use]
    pub fn case_folded(&self) -> String {
        self.0.to_ascii_lowercase()
    }

    /// The physical path under `root`, joined segment by segment so that no
    /// separator is ever translated.
    #[must_use]
    pub fn to_physical(&self, root: &Utf8Path) -> Utf8PathBuf {
        let mut path = root.to_owned();
        for segment in self.segments() {
            path.push(segment);
        }
        path
    }
}

fn validate_segment(segment: &str) -> Result<(), LogicalPathError> {
    ensure!(
        !segment.is_empty() && segment.len() <= MAX_SEGMENT_LENGTH,
        SegmentLengthSnafu { segment }
    );
    ensure!(
        segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
        SegmentCharacterSnafu { segment }
    );
    ensure!(
        segment.as_bytes()[0].is_ascii_alphanumeric(),
        SegmentStartSnafu { segment }
    );
    ensure!(!segment.ends_with('.'), SegmentTrailingDotSnafu { segment });
    let stem = segment.split('.').next().unwrap_or(segment);
    ensure!(!is_windows_device(stem), ReservedNameSnafu { segment });
    Ok(())
}

fn is_windows_device(stem: &str) -> bool {
    let stem = stem.to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let numbered = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"));
    matches!(numbered, Some(digit) if matches!(digit.as_bytes(), [b'1'..=b'9']))
}

impl fmt::Display for LogicalPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for LogicalPath {
    type Err = LogicalPathError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl TryFrom<String> for LogicalPath {
    type Error = LogicalPathError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<LogicalPath> for String {
    fn from(path: LogicalPath) -> Self {
        path.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_names_the_application_creates() {
        for value in [
            "app/application",
            "profiles/pK3mA9xZq2Lw.yaml",
            "profiles/s_Old-0000.js",
            "icons/tun.png",
            "a/b/c/d",
        ] {
            assert_eq!(LogicalPath::parse(value).unwrap().as_str(), value);
        }
    }

    #[test]
    fn rejects_unportable_names() {
        let cases = [
            ("", "length"),
            ("a//b", "length"),
            ("a/b/c/d/e", "count"),
            ("profiles/.hidden", "start"),
            ("profiles/-dash", "start"),
            ("profiles/name.", "trailing"),
            ("profiles/a b.yaml", "character"),
            ("profiles/a\\b.yaml", "character"),
            ("profiles/é.yaml", "character"),
            ("profiles/..", "start"),
            ("profiles/NUL.txt", "reserved"),
            ("profiles/com1", "reserved"),
            ("profiles/lpt9.yaml", "reserved"),
        ];
        for (value, expected) in cases {
            let kind = match LogicalPath::parse(value).unwrap_err() {
                LogicalPathError::SegmentCount { .. } => "count",
                LogicalPathError::TooLong { .. } | LogicalPathError::SegmentLength { .. } => {
                    "length"
                }
                LogicalPathError::SegmentCharacter { .. } => "character",
                LogicalPathError::SegmentStart { .. } => "start",
                LogicalPathError::SegmentTrailingDot { .. } => "trailing",
                LogicalPathError::ReservedName { .. } => "reserved",
            };
            assert_eq!(kind, expected, "{value:?}");
        }
        assert!(LogicalPath::parse(&"a".repeat(129)).is_err());
        assert!(LogicalPath::parse("profiles/com10.yaml").is_ok());
        assert!(LogicalPath::parse("profiles/console.yaml").is_ok());
    }

    #[test]
    fn joins_physical_paths_by_segment() {
        let path = LogicalPath::parse("profiles/a.yaml").unwrap();
        let physical = path.to_physical(Utf8Path::new("root"));
        assert_eq!(
            physical
                .components()
                .map(|part| part.as_str())
                .collect::<Vec<_>>(),
            ["root", "profiles", "a.yaml"]
        );
    }

    #[test]
    fn folds_case_for_collision_checks() {
        let upper = LogicalPath::parse("profiles/A.yaml").unwrap();
        let lower = LogicalPath::parse("profiles/a.yaml").unwrap();
        assert_ne!(upper, lower);
        assert_eq!(upper.case_folded(), lower.case_folded());
    }

    #[test]
    fn deserialization_validates() {
        let parsed: LogicalPath = serde_json::from_str("\"app/clash\"").unwrap();
        assert_eq!(parsed.as_str(), "app/clash");
        assert!(serde_json::from_str::<LogicalPath>("\"../etc\"").is_err());
    }
}
