//! Sequential DoH JSON fallback and capped direct HTTP echo helpers.

use std::{net::IpAddr, time::Duration};

use tokio::time::Instant;

use super::{Family, MAX_RESOLVED_IPS, is_fake_ip};

/// Upper bound on an on-demand DoH fallback lookup.
pub(super) const DOH_TIMEOUT: Duration = Duration::from_millis(1500);
/// A family without a route ends here rather than at the OS connect timeout.
const ECHO_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum HTTP response body accepted from a DoH JSON endpoint.
pub(super) const MAX_DOH_BODY_LEN: usize = 16 * 1024;
/// Maximum HTTP response body accepted from a plain-text IP echo provider.
const MAX_ECHO_BODY_LEN: usize = 256;
/// IP-literal DoH JSON endpoints used only when system DNS returns Fake-IPs
/// (`198.18.0.0/15`), omits the requested family, fails, or returns unreachable
/// (e.g. DNS-poisoned) addresses. Using IP literals avoids depending on system DNS.
pub(super) const DEFAULT_DOH_ENDPOINTS: [&str; 3] = [
    "https://223.5.5.5/resolve",
    "https://1.1.1.1/dns-query",
    "https://1.12.12.12/resolve",
];

/// Outcome of querying a single DoH JSON endpoint.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DohEndpointOutcome {
    /// Endpoint returned usable, public, non-fake addresses for the family.
    Addresses(Vec<IpAddr>),
    /// Endpoint returned HTTP 200 with valid JSON `Status == 0` and no matching
    /// `A`/`AAAA` answer records (authoritative NOERROR/NODATA).
    AuthoritativeEmpty,
    /// Endpoint failed, timed out, returned non-zero status, or yielded only unusable IPs.
    RetryNext,
}

#[derive(Debug, snafu::Snafu)]
pub(super) enum HttpBodyError {
    #[snafu(context(false), display("HTTP request error"))]
    Request { source: reqwest::Error },
    #[snafu(display("HTTP response body exceeded {limit} bytes"))]
    TooLarge { limit: usize },
    #[snafu(context(false), display("HTTP response was not valid UTF-8"))]
    InvalidUtf8 { source: std::string::FromUtf8Error },
}

pub(super) fn build_direct_http_client() -> Option<reqwest::Client> {
    // The system proxy and the core's ports would both route requests by rule,
    // and redirects could bounce a probe onto an unintended or internal host.
    match reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => Some(client),
        Err(error) => {
            tracing::warn!(%error, "failed to construct direct HTTP client for egress probing");
            None
        }
    }
}

/// Queries configured DoH JSON endpoints sequentially in priority order within
/// `deadline`, stopping as soon as one returns usable addresses or an
/// authoritative empty (`Status == 0`, no matching records) response.
pub(super) async fn resolve_doh_ips(
    client: &reqwest::Client,
    endpoints: &[String],
    host: &str,
    family: Family,
    destinations: fn(IpAddr) -> bool,
    deadline: Instant,
) -> Vec<IpAddr> {
    if endpoints.is_empty() {
        return Vec::new();
    }
    let doh_deadline = deadline.min(Instant::now() + DOH_TIMEOUT);
    for (index, endpoint) in endpoints.iter().enumerate() {
        let now = Instant::now();
        let Some(remaining) = doh_deadline.checked_duration_since(now) else {
            break;
        };
        if remaining.is_zero() {
            break;
        }
        let remaining_endpoints = (endpoints.len() - index) as u32;
        let timeout = (remaining / remaining_endpoints)
            .max(Duration::from_millis(250))
            .min(remaining);
        match query_doh_endpoint(client, endpoint, host, family, destinations, timeout).await {
            DohEndpointOutcome::Addresses(addresses) => return addresses,
            DohEndpointOutcome::AuthoritativeEmpty => return Vec::new(),
            DohEndpointOutcome::RetryNext => {}
        }
    }
    Vec::new()
}

async fn query_doh_endpoint(
    client: &reqwest::Client,
    endpoint: &str,
    host: &str,
    family: Family,
    destinations: fn(IpAddr) -> bool,
    timeout: Duration,
) -> DohEndpointOutcome {
    let Ok(mut url) = url::Url::parse(endpoint) else {
        return DohEndpointOutcome::RetryNext;
    };
    url.query_pairs_mut()
        .append_pair("name", host)
        .append_pair("type", family.dns_record_type());
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/dns-json")
        .timeout(timeout)
        .send()
        .await;
    let body: serde_json::Value = match response {
        Ok(resp) => match resp.error_for_status() {
            Ok(ok) => match read_capped_body(ok, MAX_DOH_BODY_LEN).await {
                Ok(bytes) => match serde_json::from_slice(&bytes) {
                    Ok(json) => json,
                    Err(error) => {
                        tracing::debug!(endpoint, host, %error, "DoH response was not valid JSON");
                        return DohEndpointOutcome::RetryNext;
                    }
                },
                Err(error) => {
                    tracing::debug!(
                        endpoint,
                        host,
                        %error,
                        "DoH response body exceeded limit or failed"
                    );
                    return DohEndpointOutcome::RetryNext;
                }
            },
            Err(error) => {
                tracing::debug!(endpoint, host, %error, "DoH endpoint returned an error status");
                return DohEndpointOutcome::RetryNext;
            }
        },
        Err(error) => {
            tracing::debug!(endpoint, host, %error, "DoH request failed");
            return DohEndpointOutcome::RetryNext;
        }
    };
    parse_doh_ips(&body, family, destinations)
}

pub(super) fn parse_doh_ips(
    body: &serde_json::Value,
    family: Family,
    destinations: fn(IpAddr) -> bool,
) -> DohEndpointOutcome {
    if body.get("Status").and_then(serde_json::Value::as_u64) != Some(0) {
        return DohEndpointOutcome::RetryNext;
    }
    let expected_type = family.dns_type_code();
    let Some(answers) = body.get("Answer").and_then(serde_json::Value::as_array) else {
        return DohEndpointOutcome::AuthoritativeEmpty;
    };
    let mut saw_matching_record = false;
    let mut addresses = Vec::new();
    for answer in answers {
        if answer.get("type").and_then(serde_json::Value::as_u64) != Some(expected_type) {
            continue;
        }
        saw_matching_record = true;
        let Some(ip) = answer
            .get("data")
            .and_then(serde_json::Value::as_str)
            .and_then(|raw| raw.parse::<IpAddr>().ok())
        else {
            continue;
        };
        if family.matches(ip) && destinations(ip) && !is_fake_ip(ip) && !addresses.contains(&ip) {
            addresses.push(ip);
            if addresses.len() >= MAX_RESOLVED_IPS {
                break;
            }
        }
    }
    if !addresses.is_empty() {
        DohEndpointOutcome::Addresses(addresses)
    } else if saw_matching_record {
        // Matching-type records existed but were all Fake-IP, private, or unparseable.
        DohEndpointOutcome::RetryNext
    } else {
        DohEndpointOutcome::AuthoritativeEmpty
    }
}

async fn read_capped_body(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, HttpBodyError> {
    if let Some(len) = response.content_length()
        && len > limit as u64
    {
        return Err(HttpBodyError::TooLarge { limit });
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(HttpBodyError::TooLarge { limit });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(super) async fn fetch(client: &reqwest::Client, url: &str) -> Result<String, HttpBodyError> {
    let response = client
        .get(url)
        .timeout(ECHO_TIMEOUT)
        .send()
        .await?
        .error_for_status()?;
    let bytes = read_capped_body(response, MAX_ECHO_BODY_LEN).await?;
    Ok(String::from_utf8(bytes)?)
}
