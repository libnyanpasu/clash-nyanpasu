//! Session-scoped port resolution over typed `PortStrategy` values (PR-3 T07).
//! Probing (`pick_and_try_port`) is only safe while our own core is not
//! holding the ports, so picks are cached per port-config fingerprint: the
//! running session reuses its picks unless the user changes port settings.
//! Doubles as the fetcher's `SelfProxyPortSource` (sync read of the cache).

use std::sync::Mutex;

use anyhow::Context as _;
use nyanpasu_config::{
    clash::config::{
        ClashConfig,
        clash_strategy::port::{ExternalControllerStrategy, PortStrategy},
    },
    runtime::executor::ResolvedPortBindings,
};

use crate::service::profile_file::SelfProxyPortSource;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PortsFingerprint {
    mixed: PortStrategy,
    socks: Option<PortStrategy>,
    http: Option<PortStrategy>,
    external: ExternalControllerStrategy,
}

impl PortsFingerprint {
    fn of(clash: &ClashConfig) -> Self {
        Self {
            mixed: clash.mixed_port.clone(),
            socks: clash.socks_port.clone(),
            http: clash.http_port.clone(),
            external: clash.external_controller.clone(),
        }
    }
}

#[derive(Default)]
pub struct SessionPortResolver {
    cached: Mutex<Option<(PortsFingerprint, ResolvedPortBindings)>>,
}

impl SessionPortResolver {
    pub fn resolve(&self, clash: &ClashConfig) -> anyhow::Result<ResolvedPortBindings> {
        let fingerprint = PortsFingerprint::of(clash);
        let mut cached = self
            .cached
            .lock()
            .expect("port resolver cache should not poison");
        if let Some((previous, ports)) = cached.as_ref()
            && *previous == fingerprint
        {
            return Ok(ports.clone());
        }

        let mixed_port = *clash
            .mixed_port
            .pick_and_try_port()
            .context("failed to resolve mixed port")?;
        let port = clash
            .http_port
            .as_ref()
            .map(|strategy| strategy.pick_and_try_port())
            .transpose()
            .context("failed to resolve http port")?
            .map(|picked| *picked);
        let socks_port = clash
            .socks_port
            .as_ref()
            .map(|strategy| strategy.pick_and_try_port())
            .transpose()
            .context("failed to resolve socks port")?
            .map(|picked| *picked);
        let external = clash
            .external_controller
            .port
            .pick_and_try_port()
            .context("failed to resolve external controller port")?;
        let external_controller = Some(format!("{}:{}", clash.external_controller.host, *external));

        let ports = ResolvedPortBindings {
            mixed_port,
            port,
            socks_port,
            external_controller,
        };
        *cached = Some((fingerprint, ports.clone()));
        Ok(ports)
    }

    pub fn cached_ports(&self) -> Option<ResolvedPortBindings> {
        self.cached
            .lock()
            .expect("port resolver cache should not poison")
            .as_ref()
            .map(|(_, ports)| ports.clone())
    }
}

impl SelfProxyPortSource for SessionPortResolver {
    fn mixed_port(&self) -> Option<u16> {
        self.cached_ports().map(|ports| ports.mixed_port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::clash::config::{
        ClashConfig,
        clash_strategy::port::{PortStrategy, PortStrategyKind},
    };

    fn fixed(port: u16) -> PortStrategy {
        PortStrategy {
            kind: PortStrategyKind::Fixed,
            start_port: port,
        }
    }

    #[test]
    fn resolves_fixed_strategies_and_formats_external_controller() {
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = fixed(48231);
        clash.socks_port = Some(fixed(48232));
        clash.http_port = None;
        clash.external_controller.port = fixed(48233);
        let ports = resolver.resolve(&clash).unwrap();
        assert_eq!(ports.mixed_port, 48231);
        assert_eq!(ports.socks_port, Some(48232));
        assert_eq!(ports.port, None);
        assert_eq!(
            ports.external_controller.as_deref(),
            Some("127.0.0.1:48233")
        );
        assert_eq!(resolver.mixed_port(), Some(48231));
    }

    #[test]
    fn random_pick_is_sticky_until_fingerprint_changes() {
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = PortStrategy {
            kind: PortStrategyKind::Random,
            start_port: 0,
        };
        let first = resolver.resolve(&clash).unwrap();
        let second = resolver.resolve(&clash).unwrap();
        assert_eq!(
            first, second,
            "same fingerprint must reuse the session pick"
        );
        clash.socks_port = Some(fixed(48234));
        let third = resolver.resolve(&clash).unwrap();
        assert_eq!(third.socks_port, Some(48234));
    }

    #[test]
    fn allow_fallback_moves_off_an_occupied_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = listener.local_addr().unwrap().port();
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = PortStrategy::new_allow_fallback(taken);
        let ports = resolver.resolve(&clash).unwrap();
        assert_ne!(ports.mixed_port, taken);
    }
}
