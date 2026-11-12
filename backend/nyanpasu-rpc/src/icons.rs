//! Transport-neutral icon parsing and byte reads over existing application ports.

use std::collections::BTreeMap;

use nyanpasu_rpc_protocol::{RpcError, values::TrayIcon};
use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::{error, registry::RpcDependencies};

/// Input to the icon URL transport; versions refresh mutable URLs only.
#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IconRequest {
    Cached {
        url: String,
    },
    Tray {
        mode: TrayIcon,
        version: Option<u32>,
    },
    Widget {
        mode: TrayIcon,
        version: Option<u32>,
    },
}

/// Browser cache policy, independent of the underlying seven-day file cache.
#[derive(Debug, Clone, Copy)]
pub enum IconCachePolicy {
    Remote,
    Mutable,
}

impl IconCachePolicy {
    /// Authenticated resources are private; mutable icons must revalidate.
    pub fn header(self) -> &'static str {
        match self {
            Self::Remote => "private, max-age=3600",
            Self::Mutable => "private, no-cache",
        }
    }
}

/// Validated response metadata and untrusted image bytes.
pub struct IconBody {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
    pub cache_policy: IconCachePolicy,
}

/// Required on both successful and failed responses, including cached content.
pub const SECURITY_HEADERS: [(&str, &str); 2] = [
    ("x-content-type-options", "nosniff"),
    ("content-security-policy", "sandbox; default-src 'none'"),
];

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("invalid icon path"))]
    ParseIconPath,
    #[snafu(display("invalid icon query"))]
    ParseIconQuery,
    #[snafu(display("icon URL must be absolute HTTP or HTTPS"))]
    ParseRemoteUrl,
    #[snafu(display("icon content type is not an allowed image type"))]
    CheckImageContentType,
}

fn invalid_params(error: Error) -> RpcError {
    RpcError::invalid_params(error.to_string())
}

/// Parses the single path/query grammar used by HTTP and desktop protocols.
///
/// # Errors
/// Rejects ambiguous fields, invalid modes/versions and non-HTTP remote URLs.
/// Error messages never contain the supplied URL or query.
pub fn parse(path: &str, query: Option<&str>) -> Result<IconRequest, RpcError> {
    let mut fields = unique_query(query.unwrap_or("")).map_err(invalid_params)?;
    let request = match path {
        "/cached" => {
            let url = required(&mut fields, "url").map_err(invalid_params)?;
            let parsed =
                url::Url::parse(&url).map_err(|_| invalid_params(Error::ParseRemoteUrl))?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                return Err(invalid_params(Error::ParseRemoteUrl));
            }
            IconRequest::Cached { url }
        }
        "/tray" | "/widget" => {
            let mode = match required(&mut fields, "mode")
                .map_err(invalid_params)?
                .as_str()
            {
                "normal" => TrayIcon::Normal,
                "tun" => TrayIcon::Tun,
                "system_proxy" => TrayIcon::SystemProxy,
                _ => return Err(invalid_params(Error::ParseIconQuery)),
            };
            let version = fields
                .remove("version")
                .map(|value| {
                    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                        return Err(invalid_params(Error::ParseIconQuery));
                    }
                    value
                        .parse::<u32>()
                        .map_err(|_| invalid_params(Error::ParseIconQuery))
                })
                .transpose()?;
            if path == "/tray" {
                IconRequest::Tray { mode, version }
            } else {
                IconRequest::Widget { mode, version }
            }
        }
        _ => return Err(invalid_params(Error::ParseIconPath)),
    };
    if !fields.is_empty() {
        return Err(invalid_params(Error::ParseIconQuery));
    }
    Ok(request)
}

fn unique_query(query: &str) -> Result<BTreeMap<String, String>, Error> {
    // form_urlencoded deliberately tolerates malformed escapes. The entry does not.
    for (index, byte) in query.bytes().enumerate() {
        if byte == b'%'
            && !query
                .as_bytes()
                .get(index + 1..index + 3)
                .is_some_and(|digits| digits.iter().all(u8::is_ascii_hexdigit))
        {
            return Err(Error::ParseIconQuery);
        }
    }
    let mut fields = BTreeMap::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if value.is_empty()
            || fields
                .insert(key.into_owned(), value.into_owned())
                .is_some()
        {
            return Err(Error::ParseIconQuery);
        }
    }
    Ok(fields)
}

fn required(fields: &mut BTreeMap<String, String>, key: &str) -> Result<String, Error> {
    fields.remove(key).ok_or(Error::ParseIconQuery)
}

fn allowed_content_type(mime: &str) -> Result<&'static str, RpcError> {
    let essence = mime.split(';').next().unwrap_or("").trim();
    [
        "image/png",
        "image/apng",
        "image/jpeg",
        "image/gif",
        "image/webp",
        "image/avif",
        "image/bmp",
        "image/x-icon",
        "image/vnd.microsoft.icon",
        "image/svg+xml",
    ]
    .into_iter()
    .find(|allowed| essence.eq_ignore_ascii_case(allowed))
    .ok_or_else(|| RpcError::new("invalid_icon", Error::CheckImageContentType.to_string()))
}

/// Reads through the existing cache/host ports and checks MIME on every exit.
///
/// # Errors
/// Returns application failures, unavailable host files, or disallowed image MIME.
pub async fn read(
    dependencies: &RpcDependencies,
    request: IconRequest,
) -> Result<IconBody, RpcError> {
    let (mime, bytes, cache_policy) = match request {
        IconRequest::Cached { url } => {
            let (mime, bytes) = dependencies
                .client
                .get_cached_icon(&url)
                .await
                .map_err(|error| error::application_error(error::IpcError::from(error)))?;
            (mime, bytes.to_vec(), IconCachePolicy::Remote)
        }
        IconRequest::Tray { mode, .. } | IconRequest::Widget { mode, .. } => {
            let icons = dependencies
                .icon_files
                .as_deref()
                .ok_or_else(|| RpcError::new("unavailable", "host icon files are unavailable"))?;
            let bytes = if matches!(request, IconRequest::Tray { .. }) {
                icons.tray_icon(&dependencies.paths, mode)
            } else {
                icons.widget_icon(&dependencies.paths, mode)
            };
            ("image/png".to_owned(), bytes, IconCachePolicy::Mutable)
        }
    };
    Ok(IconBody {
        content_type: allowed_content_type(&mime)?,
        bytes,
        cache_policy,
    })
}

#[cfg(test)]
pub(crate) mod tests;
