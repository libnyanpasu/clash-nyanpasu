//! Wall-clock strings and epochs for sync records.

/// The current UTC instant as RFC 3339, as commit manifests record it.
#[must_use]
pub fn now_iso8601() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

/// The current Unix epoch in seconds.
#[must_use]
pub fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}
