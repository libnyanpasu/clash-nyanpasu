//! The host's IPv4 networks for the remote-access LAN allow list, read through `if-addrs`.

use std::net::Ipv4Addr;

use if_addrs::{IfAddr, IfOperStatus};
use ipnet::Ipv4Net;
use nyanpasu_config::application::LocalNetwork;
use nyanpasu_contracts::local_networks::LocalNetworkSource;

/// The shared address space (RFC 6598) that Tailscale and carrier-grade NAT assign from.
const SHARED_ADDRESS_SPACE: Ipv4Net = Ipv4Net::new_assert(Ipv4Addr::new(100, 64, 0, 0), 10);

/// The benchmarking range (RFC 2544), where the Clash cores put their TUN interface.
const BENCHMARKING: Ipv4Net = Ipv4Net::new_assert(Ipv4Addr::new(198, 18, 0, 0), 15);

/// Reads the interface table on every call; it holds no state.
#[derive(Debug, Clone, Copy, Default)]
pub struct IfAddrsLocalNetworks;

impl LocalNetworkSource for IfAddrsLocalNetworks {
    fn networks(&self) -> Vec<LocalNetwork> {
        let interfaces = match if_addrs::get_if_addrs() {
            Ok(interfaces) => interfaces,
            Err(error) => {
                tracing::warn!(%error, "failed to read the network interfaces");
                return Vec::new();
            }
        };
        let mut networks: Vec<LocalNetwork> = Vec::new();
        for interface in interfaces {
            let IfAddr::V4(address) = &interface.addr else {
                continue;
            };
            // Linux reports a tunnel such as `tailscale0` as unknown rather than up.
            if !matches!(
                interface.oper_status,
                IfOperStatus::Up | IfOperStatus::Unknown
            ) {
                continue;
            }
            let Some(network) = local_network(&interface.name, address.ip, address.prefixlen)
            else {
                continue;
            };
            if !networks
                .iter()
                .any(|known| known.network == network.network)
            {
                networks.push(network);
            }
        }
        networks
    }
}

/// The network of one interface address, or `None` for an address no LAN peer reaches.
///
/// A point-to-point address in the shared address space (Tailscale gives each device a `/32`) has
/// its peers elsewhere in that space, so it stands for the whole space.
fn local_network(interface: &str, address: Ipv4Addr, prefix: u8) -> Option<LocalNetwork> {
    if address.is_loopback()
        || address.is_link_local()
        || address.is_unspecified()
        || address.is_multicast()
        || address.is_broadcast()
        || address.is_documentation()
        || BENCHMARKING.contains(&address)
    {
        return None;
    }
    let network = if prefix == 32 && SHARED_ADDRESS_SPACE.contains(&address) {
        SHARED_ADDRESS_SPACE
    } else {
        Ipv4Net::new(address, prefix).ok()?.trunc()
    };
    Some(LocalNetwork {
        interface: interface.to_owned(),
        address,
        network,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(address: &str, prefix: u8) -> Option<String> {
        local_network("eth0", address.parse().unwrap(), prefix).map(|n| n.network.to_string())
    }

    #[test]
    fn an_address_stands_for_its_network() {
        assert_eq!(
            network("192.168.1.20", 24).as_deref(),
            Some("192.168.1.0/24")
        );
        assert_eq!(network("10.147.17.5", 16).as_deref(), Some("10.147.0.0/16"));
    }

    #[test]
    fn a_tailscale_address_stands_for_the_shared_address_space() {
        assert_eq!(
            network("100.101.102.103", 32).as_deref(),
            Some("100.64.0.0/10")
        );
        // A carrier-grade NAT address with a real prefix keeps its own network.
        assert_eq!(network("100.70.1.9", 22).as_deref(), Some("100.70.0.0/22"));
    }

    #[test]
    fn addresses_no_lan_peer_reaches_are_left_out() {
        for address in [
            "127.0.0.1",
            "169.254.3.4",
            "0.0.0.0",
            "198.18.0.1",
            "192.0.2.1",
        ] {
            assert_eq!(network(address, 24), None, "{address}");
        }
    }

    #[test]
    fn the_result_keeps_the_interface_and_address() {
        let network = local_network("Tailscale", "100.101.102.103".parse().unwrap(), 32).unwrap();
        assert_eq!(network.interface, "Tailscale");
        assert_eq!(network.address, Ipv4Addr::new(100, 101, 102, 103));
    }
}
