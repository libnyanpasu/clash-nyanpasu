//! Network environment adapter: the primary network read from the platform's
//! interface, route and neighbour tables through `netdev`, plus the Wi-Fi
//! name where the platform gives it up without a consent prompt.

mod wifi;

use netdev::{Interface, interface::types::InterfaceType};
use nyanpasu_config::application::{InterfaceKind, NetworkObservation, NetworkState, WifiName};
use nyanpasu_contracts::network_environment::NetworkEnvironmentPort;

/// The host's network, read fresh on every call.
pub struct NetdevNetworkEnvironment;

impl NetworkEnvironmentPort for NetdevNetworkEnvironment {
    fn current(&self) -> NetworkState {
        let links: Vec<Link> = netdev::get_interfaces().iter().filter_map(link).collect();
        let Some(primary) = primary_link(&links) else {
            return NetworkState::Offline;
        };

        NetworkState::Online {
            network: NetworkObservation {
                interface_kind: primary.kind,
                wifi: match primary.kind {
                    InterfaceKind::Wifi => wifi::name(&primary.name),
                    InterfaceKind::Ethernet | InterfaceKind::Other => WifiName::NotWifi,
                },
                gateway_mac: primary.gateway_mac.clone(),
            },
        }
    }
}

/// An interface that can carry the default route to a real network.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Link {
    name: String,
    index: u32,
    kind: InterfaceKind,
    /// Whether the system default route currently leaves through it.
    default: bool,
    gateway_mac: Option<String>,
}

/// Keeps an interface that is up, has a link-layer address of its own and
/// knows a gateway. A TUN device has no link-layer address, so this
/// application's own tunnel never qualifies, whatever routes it installs.
fn link(interface: &Interface) -> Option<Link> {
    let has_address = interface
        .mac_addr
        .is_some_and(|address| address.octets() != [0; 6]);
    let gateway = interface.gateway.as_ref()?;
    let reachable = !(gateway.ipv4.is_empty() && gateway.ipv6.is_empty());
    if !(interface.is_up() && has_address && reachable)
        || interface.is_loopback()
        || interface.is_tun()
        || matches!(
            interface.if_type,
            InterfaceType::Loopback | InterfaceType::Tunnel | InterfaceType::ProprietaryVirtual
        )
    {
        return None;
    }

    Some(Link {
        name: interface.name.clone(),
        index: interface.index,
        kind: match interface.if_type {
            InterfaceType::Wireless80211 => InterfaceKind::Wifi,
            InterfaceType::Ethernet
            | InterfaceType::Ethernet3Megabit
            | InterfaceType::FastEthernetT
            | InterfaceType::FastEthernetFx
            | InterfaceType::GigabitEthernet => InterfaceKind::Ethernet,
            _ => InterfaceKind::Other,
        },
        default: interface.default,
        // An unresolved neighbour reads as the zero address.
        gateway_mac: (gateway.mac_addr.octets() != [0; 6])
            .then(|| gateway.mac_addr.to_string().to_ascii_lowercase()),
    })
}

/// The link the default route uses, or, when a tunnel has taken the default
/// route, the first remaining link in interface order.
fn primary_link(links: &[Link]) -> Option<&Link> {
    links
        .iter()
        .find(|link| link.default)
        .or_else(|| links.iter().min_by_key(|link| link.index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(name: &str, index: u32, default: bool) -> Link {
        Link {
            name: name.into(),
            index,
            kind: InterfaceKind::Ethernet,
            default,
            gateway_mac: None,
        }
    }

    #[test]
    fn the_default_route_link_is_primary() {
        let links = [link("eth0", 2, false), link("wlan0", 3, true)];

        assert_eq!(
            primary_link(&links).map(|link| link.name.as_str()),
            Some("wlan0")
        );
    }

    #[test]
    fn a_tunnel_holding_the_default_route_falls_back_to_the_first_link() {
        // The tunnel itself was filtered out, so no remaining link is default.
        let links = [link("wlan0", 3, false), link("eth0", 2, false)];

        assert_eq!(
            primary_link(&links).map(|link| link.name.as_str()),
            Some("eth0")
        );
    }

    #[test]
    fn no_link_means_offline() {
        assert_eq!(primary_link(&[]), None);
    }

    #[test]
    fn an_interface_without_a_link_layer_address_is_not_a_link() {
        let mut tunnel = Interface::dummy();
        tunnel.gateway = Some(netdev::NetworkDevice {
            mac_addr: netdev::MacAddr::zero(),
            ipv4: vec![std::net::Ipv4Addr::new(198, 18, 0, 1)],
            ipv6: Vec::new(),
        });
        tunnel.mac_addr = None;

        assert_eq!(super::link(&tunnel), None);
    }
}
