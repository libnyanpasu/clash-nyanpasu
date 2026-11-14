//! Human tables, `--json` output, NDJSON streams and degradation reporting.

use std::io::{self, Write};

use serde_json::Value;

use crate::error::ExitStatus;

const MAX_COLUMNS: usize = 8;
const MAX_CELL: usize = 60;

/// Where results and diagnostics go, and in which form.
pub struct Output<'a> {
    /// Print the RPC result unchanged instead of a table.
    pub json: bool,
    /// Results.
    pub stdout: &'a mut dyn Write,
    /// Diagnostics and degradation reports.
    pub stderr: &'a mut dyn Write,
}

impl Output<'_> {
    /// Prints a command result and reports its degradation.
    ///
    /// A committed-but-degraded mutation prints its degradation on standard error
    /// and returns [`ExitStatus::Degraded`], never success.
    ///
    /// # Errors
    /// Reports a failed write.
    pub fn result(&mut self, value: &Value) -> io::Result<ExitStatus> {
        if self.json {
            writeln!(self.stdout, "{value}")?;
        } else {
            let text = render_human(value);
            if !text.is_empty() {
                writeln!(self.stdout, "{text}")?;
            }
        }
        let degradations = degradations(value);
        if degradations.is_empty() {
            return Ok(ExitStatus::Success);
        }
        writeln!(self.stderr, "warning: applied with degradation:")?;
        for degradation in degradations {
            writeln!(self.stderr, "  - {degradation}")?;
        }
        Ok(ExitStatus::Degraded)
    }

    /// Prints one streamed value as a line of JSON.
    ///
    /// # Errors
    /// Reports a failed write.
    pub fn stream_line(&mut self, value: &Value) -> io::Result<()> {
        writeln!(self.stdout, "{value}")?;
        self.stdout.flush()
    }
}

/// The degradations of a `CommittedDegraded` mutation outcome, one line each.
/// Anything else, including a fully applied outcome, yields none.
pub fn degradations(value: &Value) -> Vec<String> {
    if value.get("status").and_then(Value::as_str) != Some("committed_degraded") {
        return Vec::new();
    }
    let items = value
        .get("degradations")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if items.is_empty() {
        return vec!["the shell reported a degraded outcome without details".to_owned()];
    }
    items
        .iter()
        .map(|item| {
            let field = |name| item.get(name).and_then(Value::as_str);
            match (field("phase"), field("message")) {
                (Some(phase), Some(message)) => format!("{phase}: {message}"),
                (None, Some(message)) => message.to_owned(),
                _ => item.to_string(),
            }
        })
        .collect()
}

/// Renders a value for a human: a table for rows, key/value pairs for an object.
pub fn render_human(value: &Value) -> String {
    // A mutation outcome shows what it returned; its degradation goes to stderr.
    if matches!(
        value.get("status").and_then(Value::as_str),
        Some("committed" | "committed_degraded")
    ) && let Some(inner) = value.get("value")
    {
        return match inner {
            Value::Null => "ok".to_owned(),
            other => render_human(other),
        };
    }
    match value {
        Value::Null => "ok".to_owned(),
        Value::String(text) => text.clone(),
        Value::Array(rows) if rows.iter().all(Value::is_object) && !rows.is_empty() => {
            rows_table(rows)
        }
        Value::Array(items) => items.iter().map(cell).collect::<Vec<_>>().join("\n"),
        Value::Object(map) => align(
            &["KEY", "VALUE"],
            map.iter()
                .map(|(key, value)| vec![key.clone(), cell(value)])
                .collect(),
        ),
        scalar => scalar.to_string(),
    }
}

fn rows_table(rows: &[Value]) -> String {
    let mut columns: Vec<&str> = Vec::new();
    for row in rows {
        let Some(map) = row.as_object() else { continue };
        for (key, value) in map {
            if !matches!(value, Value::Array(_) | Value::Object(_))
                && !columns.contains(&key.as_str())
            {
                columns.push(key);
            }
        }
    }
    columns.truncate(MAX_COLUMNS);
    if columns.is_empty() {
        return rows
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
    }
    let headers: Vec<String> = columns.iter().map(|name| name.to_uppercase()).collect();
    let header_refs: Vec<&str> = headers.iter().map(String::as_str).collect();
    align(
        &header_refs,
        rows.iter()
            .map(|row| {
                columns
                    .iter()
                    .map(|column| row.get(*column).map_or_else(String::new, cell))
                    .collect()
            })
            .collect(),
    )
}

fn cell(value: &Value) -> String {
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Null => "-".to_owned(),
        other => other.to_string(),
    };
    let text = text.replace('\n', " ");
    if text.chars().count() > MAX_CELL {
        let shortened: String = text.chars().take(MAX_CELL - 1).collect();
        format!("{shortened}…")
    } else {
        text
    }
}

fn align(headers: &[&str], rows: Vec<Vec<String>>) -> String {
    let mut widths: Vec<usize> = headers
        .iter()
        .map(|header| header.chars().count())
        .collect();
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let mut text = cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect::<Vec<_>>()
            .join("  ");
        text.truncate(text.trim_end().len());
        text
    };
    let mut lines = vec![line(headers.to_vec())];
    lines.extend(
        rows.iter()
            .map(|row| line(row.iter().map(String::as_str).collect())),
    );
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn run(json: bool, value: &Value) -> (ExitStatus, String, String) {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let status = Output {
            json,
            stdout: &mut stdout,
            stderr: &mut stderr,
        }
        .result(value)
        .unwrap();
        (
            status,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    #[test]
    fn json_output_is_the_unchanged_result_on_one_line() {
        let value = json!({"b": [1, 2], "a": null});
        let (status, stdout, stderr) = run(true, &value);
        assert_eq!(status, ExitStatus::Success);
        assert_eq!(stdout.trim_end(), value.to_string());
        assert!(stderr.is_empty());
    }

    #[test]
    fn rows_render_as_an_aligned_table_with_scalar_columns() {
        let rows = json!([
            {"uid": "p1", "name": "Home", "active": true, "chain": ["x"]},
            {"uid": "p22", "name": "Office", "active": false},
        ]);
        assert_eq!(
            render_human(&rows),
            "UID  NAME    ACTIVE\np1   Home    true\np22  Office  false"
        );
    }

    #[test]
    fn objects_render_as_key_value_pairs_and_long_cells_are_shortened() {
        let text = render_human(&json!({"mode": "rule", "long": "x".repeat(100)}));
        assert!(text.starts_with("KEY   VALUE\nmode  rule"), "{text}");
        assert!(text.contains('…'), "{text}");
        assert_eq!(render_human(&Value::Null), "ok");
        assert_eq!(render_human(&json!("plain")), "plain");
    }

    #[test]
    fn a_degraded_outcome_prints_its_degradation_and_exits_six() {
        let outcome = json!({
            "status": "committed_degraded",
            "value": null,
            "commits": [],
            "notifications_pending": true,
            "degradations": [
                {"phase": "runtime_build", "reason": "journal_invalid", "message": "boom", "retryable": true}
            ],
        });
        let (status, stdout, stderr) = run(false, &outcome);
        assert_eq!(status, ExitStatus::Degraded);
        assert_eq!(stdout.trim_end(), "ok");
        assert!(stderr.contains("runtime_build: boom"), "{stderr}");

        let (status, _, _) = run(true, &outcome);
        assert_eq!(
            status,
            ExitStatus::Degraded,
            "--json never hides degradation"
        );
    }

    #[test]
    fn a_fully_committed_outcome_is_success() {
        let outcome = json!({"status": "committed", "value": {"uid": "p1"}, "commits": [], "notifications_pending": false});
        let (status, stdout, stderr) = run(false, &outcome);
        assert_eq!(status, ExitStatus::Success);
        assert!(stdout.contains("uid"), "{stdout}");
        assert!(stderr.is_empty());
    }

    #[test]
    fn degraded_without_details_is_still_reported() {
        let lines = degradations(&json!({"status": "committed_degraded"}));
        assert_eq!(lines.len(), 1);
        assert!(degradations(&json!({"status": "committed"})).is_empty());
        assert!(degradations(&json!([1, 2])).is_empty());
    }

    #[test]
    fn stream_lines_are_one_json_object_each() {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut output = Output {
            json: false,
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        output.stream_line(&json!({"up": 1})).unwrap();
        output.stream_line(&json!({"up": 2})).unwrap();
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "{\"up\":1}\n{\"up\":2}\n"
        );
    }
}
