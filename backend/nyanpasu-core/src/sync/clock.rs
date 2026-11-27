//! Wall-clock strings and epochs for sync records.

/// The current UTC instant as RFC 3339, as commit manifests record it.
#[must_use]
pub fn now_iso8601() -> String {
    jiff::Timestamp::now().to_string()
}

/// The current Unix epoch in seconds.
#[must_use]
pub fn now_unix() -> i64 {
    jiff::Timestamp::now().as_second()
}
