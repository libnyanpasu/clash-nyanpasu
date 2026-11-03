//! Network capabilities shared by application consumers and infrastructure adapters.

use std::net::{Ipv4Addr, Ipv6Addr};

use anyhow::Result;
use bytes::Bytes;
use serde_json::Value;
use url::Url;

/// Fetches a text document; the adapter owns proxies and network deadlines.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait ChangelogHttp: Send + Sync + 'static {
    /// Returns response text or the adapter's original request failure.
    async fn get_text(&self, url: &Url) -> Result<String>;
}

/// Supplies the app's mixed port to subscription and update adapters when available.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait SelfProxyPortSource: Send + Sync + 'static {
    /// `None` means no mixed port is currently available from this source.
    fn mixed_port(&self) -> Option<u16>;
}

/// Asks an echo service, past every proxy, which address a request came from.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait DirectEgressProbe: Send + Sync + 'static {
    /// Returns the IPv4 echo answer, or no answer when the probe cannot obtain one.
    async fn ipv4(&self) -> Option<Ipv4Addr>;
    /// Returns the IPv6 echo answer, or no answer when the probe cannot obtain one.
    async fn ipv6(&self) -> Option<Ipv6Addr>;
}

/// Downloads an icon's media type and bytes using the caller's current self-proxy port.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait IconFetcher: Send + Sync + 'static {
    /// Returns the content type and response bytes, preserving request/body failures.
    async fn fetch(&self, url: &Url, self_proxy_port: u16) -> Result<(String, Bytes)>;
}

/// Probes network endpoints using the self-proxy port supplied for each operation.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait NetworkDiagnostics: Send + Sync + 'static {
    /// Measures milliseconds after a warm-up request; no matching response is `None`.
    async fn url_delay_test(
        &self,
        url: &str,
        expected_status: u16,
        self_proxy_port: u16,
    ) -> Option<u64>;

    /// Returns the ASN service's JSON document without reshaping it.
    async fn get_ipsb_asn(&self, self_proxy_port: u16) -> Result<Value>;
}

/// Reads current system proxy settings whenever an operation builds its HTTP client.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait SystemProxySource: Send + Sync + 'static {
    /// Returns the currently enabled proxy URL, or `None` when system proxying is disabled.
    fn system_proxy(&self) -> Result<Option<String>>;
}
