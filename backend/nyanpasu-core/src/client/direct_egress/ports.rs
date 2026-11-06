use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    sync::Arc,
};

use nyanpasu_config::application::{
    IpLookupProvider, NyanpasuAppConfig, StunFamilies, StunServer, StunServerCapabilities,
};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DirectEgress {
    Disabled,
    /// Nothing was probed: TUN mode captures the app's own requests and routes
    /// them by rule, so an echo could come back from a proxy exit.
    TunEnabled,
    /// `None` for a family no source gave an address over, such as a network
    /// without IPv6.
    Probed {
        ipv4: Option<Ipv4Addr>,
        ipv6: Option<Ipv6Addr>,
    },
}

/// Asked again right before each request, so switching detection off,
/// turning a server off or withdrawing a provider's consent stops the
/// requests a running probe has not sent yet.
pub trait ProbePermission: Send + Sync {
    fn allows_stun(&self, server: &StunServer) -> bool;
    fn allows_lookup(&self, provider: IpLookupProvider) -> bool;
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
    async fn ipv4(&self, sources: &ProbeSources) -> Option<Ipv4Addr>;
    async fn ipv6(&self, sources: &ProbeSources) -> Option<Ipv6Addr>;
    /// Sends one Binding request over each transport and family.
    async fn test_stun_server(&self, host: &str, port: u16) -> StunServerTest;
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
