//! Credential redaction shared by runtime inspection and diagnostic archives.
use regex::Regex;
use std::{borrow::Cow, sync::LazyLock};

pub fn redact_config(mut value: serde_json::Value) -> serde_json::Value {
    match &mut value {
        serde_json::Value::Object(fields) => {
            for (key, value) in fields {
                if matches!(
                    key.as_str(),
                    "secret" | "password" | "token" | "private-key" | "auth-str" | "authentication"
                ) {
                    *value = serde_json::Value::String("<redacted>".into());
                } else if matches!(
                    key.as_str(),
                    "external-controller" | "external-controller-tls"
                ) {
                    if let Some(address) = value.as_str()
                        && let Some((_, authority)) = address.rsplit_once('@')
                    {
                        *value = serde_json::Value::String(format!("<redacted>@{authority}"));
                    }
                } else {
                    *value = redact_config(std::mem::take(value));
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                *value = redact_config(std::mem::take(value));
            }
        }
        _ => {}
    }
    value
}

// Immutable compiled rules; no application state or dependencies.
static TEXT_RULES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
    (Regex::new(r"(?i)([a-z][a-z0-9+.-]*://)[^\s/@]+@").unwrap(), "${1}<redacted>@"),
    (Regex::new(r#"([?&][^\s=&#?]+=)[^\s&#\"']*"#).unwrap(), "${1}<redacted>"),
    (Regex::new(r#"(?i)(\b(?:secret|password|token|private-key|auth-str|authentication)\b[\"']?\s*[:=]\s*\")(?:\\.|[^\"\\])*"#).unwrap(), "${1}<redacted>"),
    (Regex::new(r#"(?i)(\b(?:secret|password|token|private-key|auth-str|authentication)\b[\"']?\s*[:=]\s*')(?:\\.|[^'\\])*"#).unwrap(), "${1}<redacted>"),
    (Regex::new(r#"(?i)(\b(?:secret|password|token|private-key|auth-str|authentication)\b[\"']?\s*[:=]\s*)[^\s,;}&\"']+"#).unwrap(), "${1}<redacted>"),
    (Regex::new(r"(?i)(\bAuthorization:\s*)[^\r\n]+").unwrap(), "${1}<redacted>"),
    (Regex::new(r"(?i)(\bBearer\s+)[^\s,;]+" ).unwrap(), "${1}<redacted>"),
    (Regex::new(r#"(?i)(\bexternal-controller(?:-tls)?[\"']?\s*[:=]\s*[\"']?)[^\s@]+@"#).unwrap(), "${1}<redacted>@"),
]
});

/// Redacts a line, borrowing it when none of the rules changes it.
pub fn redact_text<'a>(line: &'a str, home: Option<&str>) -> Cow<'a, str> {
    let mut result = Cow::Borrowed(line);
    for (pattern, replacement) in TEXT_RULES.iter() {
        // All rules retain capture 1 and replace the remainder with a literal.
        // Already redacted matches need no replacement allocation.
        if pattern.captures_iter(&result).any(|captures| {
            captures[0].strip_prefix(&captures[1]) != replacement.strip_prefix("${1}")
        }) {
            let next = pattern.replace_all(&result, *replacement);
            if next != result {
                result = Cow::Owned(next.into_owned());
            }
        }
    }
    if let Some(home) = home.filter(|home| !home.is_empty()) {
        let mut from = 0;
        while let Some((start, end)) = result[from..].char_indices().find_map(|(offset, _)| {
            let start = from + offset;
            let bytes = result.as_bytes();
            let mut end = start;
            for expected in home.bytes() {
                let actual = *bytes.get(end)?;
                if expected == b'\\' && actual == b'\\' && bytes.get(end + 1) == Some(&b'\\') {
                    // JSON and Debug paths escape Windows separators.
                    end += 2;
                } else if actual == expected || (expected == b'\\' && actual == b'/') {
                    end += 1;
                } else {
                    return None;
                }
            }
            Some((start, end))
        }) {
            result.to_mut().replace_range(start..end, "~");
            from = start + 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redact_text_rules_are_idempotent_and_preserve_unicode() {
        for (line, expected) in [
            (
                "https://user:pass@host/path?token=abc&x=1",
                "https://<redacted>@host/path?token=<redacted>&x=<redacted>",
            ),
            (
                "password=abc token: xyz",
                "password=<redacted> token: <redacted>",
            ),
            (
                r#"{"secret":"你好 世界","private-key":"abc"}"#,
                r#"{"secret":"<redacted>","private-key":"<redacted>"}"#,
            ),
            ("Authorization: Bearer abc", "Authorization: <redacted>"),
            ("Bearer abc", "Bearer <redacted>"),
            (
                "external-controller: user:pass@host:9090",
                "external-controller: <redacted>@host:9090",
            ),
            ("你好 C:/Users/test/file", "你好 ~/file"),
        ] {
            let redacted = redact_text(line, Some("C:/Users/test"));
            assert_eq!(redacted, expected);
            assert_eq!(redact_text(&redacted, Some("C:/Users/test")), expected);
        }
    }
    #[test]
    fn redact_text_covers_every_credential_key_and_keeps_json_quotes() {
        for key in [
            "secret",
            "password",
            "token",
            "private-key",
            "auth-str",
            "authentication",
        ] {
            let input = format!(
                "{{{}:{}}}",
                serde_json::to_string(key).unwrap(),
                serde_json::to_string("中文 secret with \"quotes\" and \\slashes").unwrap()
            );
            let redacted = redact_text(&input, None);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&redacted).unwrap()[key],
                "<redacted>"
            );
            assert!(matches!(redact_text(&redacted, None), Cow::Borrowed(_)));
            assert_eq!(
                redact_text(&format!("{key}='two words'"), None),
                format!("{key}='<redacted>'")
            );
        }
    }
    #[test]
    fn redact_home_accepts_windows_slashes() {
        assert_eq!(
            redact_text("C:/Users/test/file", Some(r"C:\Users\test")),
            "~/file"
        );
        assert_eq!(
            redact_text(r"C:\\Users\\test\\file", Some(r"C:\Users\test")),
            r"~\\file"
        );
        assert!(matches!(
            redact_text("ordinary /other/path", Some(r"C:\Users\test")),
            Cow::Borrowed(_)
        ));
    }
    #[test]
    fn redact_text_borrows_unchanged_lines() {
        assert!(matches!(
            redact_text("你好 ordinary log", None),
            Cow::Borrowed(_)
        ));
    }
    #[test]
    fn redact_config_keeps_the_existing_rules() {
        assert_eq!(
            redact_config(
                serde_json::json!({"nested":[{"token":"abc"}],"external-controller":"u:p@host"})
            ),
            serde_json::json!({"nested":[{"token":"<redacted>"}],"external-controller":"<redacted>@host"})
        );
    }
}
