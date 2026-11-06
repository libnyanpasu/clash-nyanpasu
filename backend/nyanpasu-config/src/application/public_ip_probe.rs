//! Where the public IP probe may ask for the machine's address: STUN servers
//! first, then HTTP lookup services the user accepted.
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use specta::Type;

/// Which address families a transport answered over.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct StunFamilies {
    pub ipv4: bool,
    pub ipv6: bool,
}

/// Which transports and families a STUN server answers Binding requests over.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct StunServerCapabilities {
    pub udp: StunFamilies,
    pub tcp: StunFamilies,
}

/// A STUN server the probe may ask. The built-in ones are ordinary entries of
/// the list, so the user can turn them off or reorder them like their own.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct StunServer {
    pub host: String,
    pub port: u16,
    pub enabled: bool,
    /// What the operator documents for a built-in server, or what the test
    /// found when the user added it.
    pub capabilities: StunServerCapabilities,
    /// Shipped with the app rather than added by the user.
    #[serde(default)]
    pub builtin: bool,
}

/// The built-in servers, in the order the probe asks them. Capabilities are
/// what each operator documents or what its DNS records offer (AAAA records
/// for IPv6), checked on 2026-11-06.
pub fn default_stun_servers() -> Vec<StunServer> {
    const BOTH: StunFamilies = StunFamilies {
        ipv4: true,
        ipv6: true,
    };
    const IPV4: StunFamilies = StunFamilies {
        ipv4: true,
        ipv6: false,
    };
    const NONE: StunFamilies = StunFamilies {
        ipv4: false,
        ipv6: false,
    };
    [
        ("stun.cloudflare.com", 3478, true, BOTH, NONE),
        ("stun.l.google.com", 19302, true, BOTH, NONE),
        ("stun.miwifi.com", 3478, true, IPV4, NONE),
        ("stun.chat.bilibili.com", 3478, false, IPV4, NONE),
        ("stun.nextcloud.com", 3478, false, BOTH, BOTH),
    ]
    .into_iter()
    .map(|(host, port, enabled, udp, tcp)| StunServer {
        host: host.into(),
        port,
        enabled,
        capabilities: StunServerCapabilities { udp, tcp },
        builtin: true,
    })
    .collect()
}

/// HTTP services that answer with the caller's address. Each one receives
/// the address and ordinary request metadata, so none is used until the user
/// accepts its privacy notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IpLookupProvider {
    Dnspod,
    IpSb,
    Ipify,
    IdentMe,
}

impl IpLookupProvider {
    pub const ALL: [Self; 4] = [Self::Dnspod, Self::IpSb, Self::Ipify, Self::IdentMe];

    /// Endpoints that answer with the address as plain text. Each host
    /// resolves in one family only, so a request leaves over that family.
    pub fn ipv4_url(self) -> &'static str {
        match self {
            Self::Dnspod => "https://ipv4.ddnspod.com",
            Self::IpSb => "https://api-ipv4.ip.sb/ip",
            Self::Ipify => "https://api4.ipify.org",
            Self::IdentMe => "https://4.ident.me",
        }
    }

    pub fn ipv6_url(self) -> &'static str {
        match self {
            Self::Dnspod => "https://ipv6.ddnspod.com",
            Self::IpSb => "https://api-ipv6.ip.sb/ip",
            Self::Ipify => "https://api6.ipify.org",
            Self::IdentMe => "https://6.ident.me",
        }
    }
}

/// A provider and the endpoints it is asked at, for its privacy notice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IpLookupProviderInfo {
    pub provider: IpLookupProvider,
    pub ipv4_url: String,
    pub ipv6_url: String,
}

impl From<IpLookupProvider> for IpLookupProviderInfo {
    fn from(provider: IpLookupProvider) -> Self {
        Self {
            provider,
            ipv4_url: provider.ipv4_url().into(),
            ipv6_url: provider.ipv6_url().into(),
        }
    }
}

/// Rejects malformed servers and repeated entries in either list.
pub fn validate_public_ip_probe_sources(
    servers: &[StunServer],
    providers: &[IpLookupProvider],
) -> Result<(), &'static str> {
    if providers
        .iter()
        .enumerate()
        .any(|(index, provider)| providers[..index].contains(provider))
    {
        return Err("IP lookup providers must not contain duplicates");
    }
    validate_stun_servers(servers)
}

fn validate_stun_servers(servers: &[StunServer]) -> Result<(), &'static str> {
    for (index, server) in servers.iter().enumerate() {
        if server.port == 0 {
            return Err("a STUN server port must not be 0");
        }
        if !is_stun_host(&server.host) {
            return Err("a STUN server host must be an IP address or a host name");
        }
        if servers[..index]
            .iter()
            .any(|other| other.port == server.port && other.host.eq_ignore_ascii_case(&server.host))
        {
            return Err("STUN servers must not contain duplicates");
        }
    }
    Ok(())
}

fn is_stun_host(host: &str) -> bool {
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(host: &str, port: u16) -> StunServer {
        StunServer {
            host: host.into(),
            port,
            enabled: true,
            capabilities: StunServerCapabilities::default(),
            builtin: false,
        }
    }

    #[test]
    fn custom_servers_accept_host_names_and_addresses() {
        let servers = [
            custom("stun.example.com", 3478),
            custom("192.0.2.1", 3478),
            custom("2001:db8::1", 3478),
        ];

        assert_eq!(validate_stun_servers(&servers), Ok(()));
    }

    #[test]
    fn custom_servers_reject_invalid_entries() {
        for invalid in [
            vec![custom("stun.example.com", 0)],
            vec![custom("", 3478)],
            vec![custom("stun example.com", 3478)],
            vec![custom("stun.example.com:3478", 3478)],
            vec![custom("-stun.example.com", 3478)],
            vec![
                custom("stun.example.com", 3478),
                custom("STUN.example.com", 3478),
            ],
        ] {
            assert!(validate_stun_servers(&invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn the_same_host_on_another_port_is_another_server() {
        let servers = [
            custom("stun.example.com", 3478),
            custom("stun.example.com", 19302),
        ];

        assert_eq!(validate_stun_servers(&servers), Ok(()));
    }

    #[test]
    fn the_defaults_are_valid_and_a_repeated_provider_is_not() {
        let mut servers = default_stun_servers();
        servers.push(custom("stun.example.com", 3478));
        let ipify = [IpLookupProvider::Ipify, IpLookupProvider::Ipify];

        assert_eq!(
            validate_public_ip_probe_sources(&servers, &IpLookupProvider::ALL),
            Ok(())
        );
        assert!(validate_public_ip_probe_sources(&[], &ipify).is_err());
    }
}
