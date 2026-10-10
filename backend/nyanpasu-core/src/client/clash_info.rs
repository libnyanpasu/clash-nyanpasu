//! The endpoint local clients (the web UI, the frontend) use to reach the core,
//! derived from typed state only.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use nyanpasu_config::{clash::config::ClashConfig, runtime::executor::ResolvedPortBindings};
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq, specta::Type)]
pub struct ClashInfo {
    /// clash core port
    pub port: u16,
    /// same as `external-controller`
    pub server: String,
    /// clash secret
    pub secret: Option<String>,
}

impl ClashInfo {
    /// Confirmed session ports win, so a fallback or random pick is reported
    /// as actually bound; without one the configured start ports are the best
    /// answer. An unspecified listen host is not dialable and maps to loopback.
    pub fn derive(confirmed: Option<&ResolvedPortBindings>, clash: &ClashConfig) -> Self {
        let configured = SocketAddr::new(
            clash.external_controller.host,
            clash.external_controller.port.start_port,
        );
        let mut server = confirmed
            .and_then(|ports| ports.external_controller.as_deref())
            .and_then(parse_controller)
            .unwrap_or(configured);
        if server.ip().is_unspecified() {
            server.set_ip(IpAddr::V4(Ipv4Addr::LOCALHOST));
        }

        Self {
            port: confirmed.map_or(clash.mixed_port.start_port, |ports| ports.mixed_port),
            server: server.to_string(),
            secret: Some(clash.overrides.secret().to_owned()),
        }
    }
}

/// The session resolver formats the controller as `{host}:{port}`, which
/// leaves an IPv6 host unbracketed; split on the last colon instead of
/// parsing it as a socket address.
fn parse_controller(raw: &str) -> Option<SocketAddr> {
    let (host, port) = raw.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    Some(SocketAddr::new(host.parse().ok()?, port.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::clash::config::clash_strategy::port::{
        ExternalControllerStrategy, PortStrategy,
    };

    fn configured(host: IpAddr, mixed: u16, controller: u16) -> ClashConfig {
        ClashConfig {
            mixed_port: PortStrategy::new_allow_fallback(mixed),
            external_controller: ExternalControllerStrategy {
                host,
                port: PortStrategy::new_allow_fallback(controller),
            },
            ..ClashConfig::default()
        }
    }

    fn confirmed(mixed: u16, controller: &str) -> ResolvedPortBindings {
        ResolvedPortBindings {
            mixed_port: mixed,
            external_controller: Some(controller.into()),
            ..ResolvedPortBindings::default()
        }
    }

    #[test]
    fn a_confirmed_binding_wins_over_the_configured_ports() {
        let clash = configured(IpAddr::V4(Ipv4Addr::LOCALHOST), 7890, 9090);

        let info = ClashInfo::derive(Some(&confirmed(7891, "127.0.0.1:9091")), &clash);

        assert_eq!(info.port, 7891);
        assert_eq!(info.server, "127.0.0.1:9091");
        assert_eq!(info.secret.as_deref(), Some(clash.overrides.secret()));
    }

    #[test]
    fn without_a_confirmed_binding_the_configured_ports_are_reported() {
        let clash = configured("192.168.1.1".parse().unwrap(), 7890, 9090);

        let info = ClashInfo::derive(None, &clash);

        assert_eq!(info.port, 7890);
        assert_eq!(info.server, "192.168.1.1:9090");
        assert_eq!(info.secret.as_deref(), Some(clash.overrides.secret()));
    }

    #[test]
    fn a_confirmed_binding_without_a_controller_keeps_the_configured_controller() {
        let clash = configured(IpAddr::V4(Ipv4Addr::LOCALHOST), 7890, 9090);
        let ports = ResolvedPortBindings {
            mixed_port: 7891,
            ..ResolvedPortBindings::default()
        };

        let info = ClashInfo::derive(Some(&ports), &clash);

        assert_eq!(info.port, 7891);
        assert_eq!(info.server, "127.0.0.1:9090");
    }

    #[test]
    fn an_unspecified_host_is_reported_as_loopback() {
        let unspecified_v4 = configured(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 7890, 9090);
        assert_eq!(
            ClashInfo::derive(None, &unspecified_v4).server,
            "127.0.0.1:9090"
        );

        let unspecified_v6 = configured("::".parse().unwrap(), 7890, 9090);
        assert_eq!(
            ClashInfo::derive(None, &unspecified_v6).server,
            "127.0.0.1:9090"
        );

        let clash = configured(IpAddr::V4(Ipv4Addr::LOCALHOST), 7890, 9090);
        assert_eq!(
            ClashInfo::derive(Some(&confirmed(7890, "0.0.0.0:9091")), &clash).server,
            "127.0.0.1:9091"
        );
        assert_eq!(
            ClashInfo::derive(Some(&confirmed(7890, ":::9091")), &clash).server,
            "127.0.0.1:9091"
        );
    }

    #[test]
    fn a_confirmed_ipv6_controller_is_bracketed() {
        let clash = configured(IpAddr::V4(Ipv4Addr::LOCALHOST), 7890, 9090);

        let info = ClashInfo::derive(Some(&confirmed(7890, "::1:9091")), &clash);

        assert_eq!(info.server, "[::1]:9091");
    }
}
