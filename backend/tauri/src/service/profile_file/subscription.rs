//! Pure parsing of subscription response headers.

use nyanpasu_config::profile::SubscriptionInfo;

pub(super) fn parse_media_type(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let value = headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())?;
    let media_type = value.split(';').next()?.trim().to_ascii_lowercase();
    (!media_type.is_empty()).then_some(media_type)
}

pub(super) fn parse_suggested_update_interval(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let hours = headers
        .get("profile-update-interval")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|hours| *hours > 0)?;
    let minutes = hours.checked_mul(60)?;
    minutes.checked_mul(60)?;
    Some(minutes)
}

pub(super) fn parse_subscription_userinfo(
    headers: &reqwest::header::HeaderMap,
) -> SubscriptionInfo {
    let Some(value) = headers
        .get("subscription-userinfo")
        .or_else(|| headers.get("Subscription-Userinfo"))
    else {
        return SubscriptionInfo::default();
    };
    let raw = value.to_str().unwrap_or("");
    let field = |key: &str| nyanpasu_core::utils::key_value::parse_str::<u64>(raw, key);
    SubscriptionInfo {
        upload: field("upload"),
        download: field("download"),
        total: field("total"),
        expire: field("expire")
            .filter(|secs| *secs != 0)
            .and_then(|secs| i64::try_from(secs).ok())
            .and_then(|secs| time::OffsetDateTime::from_unix_timestamp(secs).ok()),
    }
}

pub(super) fn parse_profile_title(headers: &reqwest::header::HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("profile-title").and_then(|v| v.to_str().ok()) {
        if value.trim().is_empty() {
            return parse_filename_from_content_disposition(headers);
        }
        if let Some(encoded) = value.strip_prefix("base64:") {
            use base64::Engine;
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded)
                && let Ok(decoded) = String::from_utf8(bytes)
                && !decoded.trim().is_empty()
            {
                return Some(decoded);
            }
        } else {
            return Some(value.to_owned());
        }
    }

    parse_filename_from_content_disposition(headers)
}

pub(super) fn parse_filename_from_content_disposition(
    headers: &reqwest::header::HeaderMap,
) -> Option<String> {
    let value = headers
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())?;

    value
        .split(';')
        .map(str::trim)
        .filter_map(|part| part.split_once('='))
        .find_map(|(name, value)| {
            let name = name.trim();
            let value = value.trim();
            if name.eq_ignore_ascii_case("filename*") {
                decode_rfc5987_filename(value)
            } else if name.eq_ignore_ascii_case("filename") {
                Some(value.trim_matches(['"', '\'']).to_owned())
            } else {
                None
            }
        })
        .filter(|filename| !filename.is_empty())
}

fn decode_rfc5987_filename(value: &str) -> Option<String> {
    let value = value.trim().trim_matches(['"', '\'']);
    let encoded = value
        .split_once('\'')
        .and_then(|(_, rest)| rest.split_once('\'').map(|(_, encoded)| encoded))
        .unwrap_or(value);
    percent_encoding::percent_decode(encoded.as_bytes())
        .decode_utf8()
        .ok()
        .map(|decoded| decoded.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_type_parsing_drops_parameters_and_case() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(parse_media_type(&headers), None);

        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static("Text/JavaScript; charset=utf-8"),
        );
        assert_eq!(
            parse_media_type(&headers).as_deref(),
            Some("text/javascript")
        );

        headers.insert(
            reqwest::header::CONTENT_TYPE,
            reqwest::header::HeaderValue::from_static(" ; charset=utf-8"),
        );
        assert_eq!(parse_media_type(&headers), None);
    }

    #[test]
    fn content_disposition_filename_parsing_matches_legacy_variants() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_static("attachment; Filename=\"a.yaml\""),
        );
        assert_eq!(
            parse_filename_from_content_disposition(&headers).as_deref(),
            Some("a.yaml")
        );

        headers.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_static(
                "attachment; filename*=UTF-8'en'my%20cfg.yaml",
            ),
        );
        assert_eq!(
            parse_filename_from_content_disposition(&headers).as_deref(),
            Some("my cfg.yaml")
        );

        headers.insert(
            "profile-title",
            reqwest::header::HeaderValue::from_static("   "),
        );
        headers.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_static("attachment; filename=fallback.yaml"),
        );
        assert_eq!(
            parse_profile_title(&headers).as_deref(),
            Some("fallback.yaml")
        );
    }

    #[test]
    fn subscription_expire_ignores_absurd_overflow_values() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "subscription-userinfo",
            reqwest::header::HeaderValue::from_static(
                "upload=1; download=2; total=3; expire=18446744073709551615",
            ),
        );
        let parsed = parse_subscription_userinfo(&headers);
        assert_eq!(parsed.upload, Some(1));
        assert!(parsed.expire.is_none());
    }

    #[test]
    fn suggested_update_interval_parses_hours_and_ignores_invalid_values() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(parse_suggested_update_interval(&headers), None);

        headers.insert(
            "profile-update-interval",
            reqwest::header::HeaderValue::from_static("2"),
        );
        assert_eq!(parse_suggested_update_interval(&headers), Some(120));

        for invalid in ["not-a-number", "0", "18446744073709551615"] {
            headers.insert(
                "profile-update-interval",
                reqwest::header::HeaderValue::from_str(invalid).unwrap(),
            );
            assert_eq!(parse_suggested_update_interval(&headers), None);
        }
    }
}
