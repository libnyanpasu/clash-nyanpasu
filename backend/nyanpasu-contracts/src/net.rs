//! Network capabilities shared by application consumers and infrastructure adapters.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

use anyhow::Result;
use bytes::Bytes;
use nyanpasu_config::application::{
    IpLookupProvider, NyanpasuAppConfig, StunFamilies, StunServer, StunServerCapabilities,
};
use serde::Serialize;
use serde_json::Value;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DirectEgress {
    Disabled,
    /// Nothing was probed: TUN mode captures the app's own requests and routes
    /// them by rule, so an echo could come back from a proxy exit.
    TunEnabled,
    /// Local transparent capture can route the probe through a proxy.
    TransparentProxyEnabled,
    /// `None` for a family no source gave an address over, such as a network
    /// without IPv6.
    Probed {
        ipv4: Option<EgressAddress<Ipv4Addr>>,
        ipv6: Option<EgressAddress<Ipv6Addr>>,
    },
}

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

/// Asked again right before each request, so switching detection off,
/// turning a server off or withdrawing a provider's consent stops the
/// requests a running probe has not sent yet.
pub trait ProbePermission: Send + Sync {
    fn allows_stun(&self, server: &StunServer) -> bool;
    fn allows_lookup(&self, provider: IpLookupProvider) -> bool;
}

/// One family's direct egress address, as the app reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct EgressAddress<A> {
    pub address: A,
    pub source: EgressSource,
    /// An upper-case ISO code from the core's country database. `None` while
    /// the core has no database, or the address is not in it.
    pub country: Option<String>,
}

/// One family's answer, as the probe found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressAnswer<A> {
    pub address: A,
    pub source: EgressSource,
}

/// What answered a probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EgressSource {
    Stun {
        host: String,
        port: u16,
        transport: StunTransport,
    },
    Lookup {
        provider: IpLookupProvider,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum StunTransport {
    Udp,
    Tcp,
}

/// Where a probe asks, in order: STUN servers, then accepted HTTP providers.
#[derive(Clone)]
pub struct ProbeSources {
    pub stun: Vec<StunServer>,
    pub lookups: Vec<IpLookupProvider>,
    pub permission: Arc<dyn ProbePermission>,
}

impl ProbeSources {
    /// The enabled servers, in the order the user keeps them.
    pub fn from_config(config: &NyanpasuAppConfig, permission: Arc<dyn ProbePermission>) -> Self {
        Self {
            stun: config
                .stun_servers
                .iter()
                .filter(|server| server.enabled)
                .cloned()
                .collect(),
            lookups: config.ip_lookup_providers.clone(),
            permission,
        }
    }
}

/// One Binding request over one transport and family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StunAttempt {
    /// The server named the address the request came from.
    Answered {
        address: IpAddr,
        latency_ms: u64,
    },
    /// The host has no address of this family it may be asked at, or did not
    /// resolve.
    NoAddress,
    NoAnswer,
}

impl StunAttempt {
    fn answered(self) -> bool {
        matches!(self, Self::Answered { .. })
    }
}

/// What a test of a STUN server found over each transport and family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
pub struct StunServerTest {
    pub udp_ipv4: StunAttempt,
    pub udp_ipv6: StunAttempt,
    pub tcp_ipv4: StunAttempt,
    pub tcp_ipv6: StunAttempt,
    /// The transports and families that answered.
    pub capabilities: StunServerCapabilities,
}

impl StunServerTest {
    pub fn new(
        udp_ipv4: StunAttempt,
        udp_ipv6: StunAttempt,
        tcp_ipv4: StunAttempt,
        tcp_ipv6: StunAttempt,
    ) -> Self {
        Self {
            udp_ipv4,
            udp_ipv6,
            tcp_ipv4,
            tcp_ipv6,
            capabilities: StunServerCapabilities {
                udp: StunFamilies {
                    ipv4: udp_ipv4.answered(),
                    ipv6: udp_ipv6.answered(),
                },
                tcp: StunFamilies {
                    ipv4: tcp_ipv4.answered(),
                    ipv6: tcp_ipv6.answered(),
                },
            },
        }
    }
}

/// Asks network services, past every proxy, which address a request came from.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait DirectEgressProbe: Send + Sync + 'static {
    async fn ipv4(&self, sources: &ProbeSources) -> Option<EgressAnswer<Ipv4Addr>>;
    async fn ipv6(&self, sources: &ProbeSources) -> Option<EgressAnswer<Ipv6Addr>>;
    /// Sends one Binding request over each transport and family.
    async fn test_stun_server(&self, host: &str, port: u16) -> StunServerTest;
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

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn sources_ask_the_enabled_servers_in_list_order() {
        let mut config = NyanpasuAppConfig::default();
        let mut servers = nyanpasu_config::application::default_stun_servers();
        servers.insert(
            0,
            StunServer {
                host: "stun.example.com".into(),
                port: 3479,
                enabled: true,
                capabilities: StunServerCapabilities::default(),
                builtin: false,
            },
        );
        for server in &mut servers[1..] {
            server.enabled = server.host == "stun.nextcloud.com";
        }
        config.stun_servers = servers;
        config.ip_lookup_providers = vec![IpLookupProvider::Ipify, IpLookupProvider::Dnspod];

        struct Anything;
        impl ProbePermission for Anything {
            fn allows_stun(&self, _: &StunServer) -> bool {
                true
            }
            fn allows_lookup(&self, _: IpLookupProvider) -> bool {
                true
            }
        }

        let sources = ProbeSources::from_config(&config, Arc::new(Anything));

        let hosts: Vec<&str> = sources
            .stun
            .iter()
            .map(|server| server.host.as_str())
            .collect();
        assert_eq!(hosts, ["stun.example.com", "stun.nextcloud.com"]);
        assert_eq!(
            sources.lookups,
            [IpLookupProvider::Ipify, IpLookupProvider::Dnspod]
        );
    }

    #[test]
    fn a_test_reports_what_answered_as_capabilities() {
        let answered = StunAttempt::Answered {
            address: Ipv4Addr::new(203, 0, 113, 7).into(),
            latency_ms: 12,
        };

        let test = StunServerTest::new(
            answered,
            StunAttempt::NoAddress,
            StunAttempt::NoAnswer,
            answered,
        );

        assert_eq!(
            test.capabilities,
            StunServerCapabilities {
                udp: StunFamilies {
                    ipv4: true,
                    ipv6: false
                },
                tcp: StunFamilies {
                    ipv4: false,
                    ipv6: true
                },
            }
        );
    }
}
