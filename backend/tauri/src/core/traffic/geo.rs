//! Regions of a connection's ends, looked up in the app's own country index.
use std::net::IpAddr;

use clash_api::{ConfigEnum, ConnectionNetwork, DnsMode, api::ConnectionMetadataFields};
use nyanpasu_geodata::IpIndex;
use nyanpasu_traffic::{GeoBasis, normalize_region};

use super::LocalSourceIps;

const UNKNOWN: &str = "unknown";

/// The exit the core's built-in direct outbound reports. A custom `type: direct` proxy has its
/// own name and counts as a proxy, which only understates what was dialed.
const DIRECT: &str = "DIRECT";

/// A country for an address: an upper-case ISO code, as the map's markers are keyed.
pub(crate) trait CountryLookup {
    fn country(&self, ip: IpAddr) -> Option<String>;
}

impl CountryLookup for IpIndex {
    fn country(&self, ip: IpAddr) -> Option<String> {
        Some(self.lookup(ip)?.country()?.to_ascii_uppercase())
    }
}

fn address(value: Option<&String>) -> Option<IpAddr> {
    value?.parse().ok()
}

fn is_tcp(meta: &ConnectionMetadataFields) -> bool {
    matches!(
        meta.network,
        Some(ConfigEnum::Known(ConnectionNetwork::Tcp))
    )
}

/// Private, loopback and link-local addresses describe the local network, not a country.
fn is_local(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(ip) => {
            ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified()
        }
        IpAddr::V6(ip) => {
            ip.is_unique_local()
                || ip.is_loopback()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
        }
    }
}

pub(crate) fn locate_source(
    meta: &ConnectionMetadataFields,
    index: Option<&dyn CountryLookup>,
    local: LocalSourceIps,
) -> String {
    let source = meta.source_ip.as_deref().unwrap_or("");
    let ip = address(meta.source_ip.as_ref()).map(|ip| ip.to_canonical());
    let local_label = source.eq_ignore_ascii_case("local") || source.eq_ignore_ascii_case("lan");
    let local_source = local_label || ip.is_some_and(is_local);
    let ip = if local_source {
        match ip {
            Some(IpAddr::V6(_)) => local.ipv6.map(IpAddr::V6),
            Some(IpAddr::V4(_)) => local.ipv4.map(IpAddr::V4),
            None => local
                .ipv4
                .map(IpAddr::V4)
                .or_else(|| local.ipv6.map(IpAddr::V6)),
        }
    } else {
        ip
    };
    match index {
        Some(index) => ip
            .and_then(|ip| index.country(ip))
            .unwrap_or_else(|| UNKNOWN.to_owned()),
        // The core's code describes the original address, never the substituted public IP.
        None if local_source => UNKNOWN.to_owned(),
        None => normalize_region(meta.source_geo_ip.iter().flatten()),
    }
}

/// What `destinationIP` is to the outbound. DIRECT dials over UDP the address it resolved into
/// `destinationIP`; any outbound dials it when the client gave no host, or over TCP when the
/// core's hosts answered. Otherwise the outbound was handed the host and resolves it itself.
fn destination_ip_basis(meta: &ConnectionMetadataFields, exit: Option<&str>) -> GeoBasis {
    let tcp = is_tcp(meta);
    let hosts = matches!(meta.dns_mode, Some(ConfigEnum::Known(DnsMode::Hosts)));
    let no_host = meta.host.as_deref().is_none_or(str::is_empty);
    if (exit == Some(DIRECT) && !tcp) || no_host || (tcp && hosts) {
        GeoBasis::Dialed
    } else {
        GeoBasis::Resolved
    }
}

/// The address that stands for the destination, and its basis. A DIRECT TCP connection's
/// socket peer is what it dialed; behind a proxy the peer is the proxy, never the destination.
fn destination(meta: &ConnectionMetadataFields, exit: Option<&str>) -> Option<(IpAddr, GeoBasis)> {
    if exit == Some(DIRECT)
        && is_tcp(meta)
        && let Some(peer) = address(meta.remote_destination.as_ref())
    {
        return Some((peer, GeoBasis::Dialed));
    }
    let ip = address(meta.destination_ip.as_ref())?;
    Some((ip, destination_ip_basis(meta, exit)))
}

/// The destination's region and how it was located; `exit` is `chains[0]`.
pub(crate) fn locate_destination(
    meta: &ConnectionMetadataFields,
    exit: Option<&str>,
    index: Option<&dyn CountryLookup>,
) -> (String, Option<GeoBasis>) {
    let Some(index) = index else {
        // The core's codes describe destinationIP, whatever the outbound dialed.
        let region = normalize_region(meta.destination_geo_ip.iter().flatten());
        let basis = (region != UNKNOWN).then(|| destination_ip_basis(meta, exit));
        return (region, basis);
    };
    match destination(meta, exit).and_then(|(ip, basis)| Some((index.country(ip)?, basis))) {
        Some((region, basis)) => (region, Some(basis)),
        None => (UNKNOWN.to_owned(), None),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Countries by address, for tests that do not build an index.
    pub(crate) struct Countries(pub HashMap<IpAddr, &'static str>);

    impl Countries {
        pub(crate) fn of(entries: &[(&str, &'static str)]) -> Self {
            Self(
                entries
                    .iter()
                    .map(|(ip, country)| (ip.parse().unwrap(), *country))
                    .collect(),
            )
        }
    }

    impl CountryLookup for Countries {
        fn country(&self, ip: IpAddr) -> Option<String> {
            self.0.get(&ip).map(|country| (*country).to_owned())
        }
    }

    fn meta(fields: serde_json::Value) -> ConnectionMetadataFields {
        serde_json::from_value(fields).unwrap()
    }

    #[test]
    fn the_index_locates_both_addresses() {
        let countries = Countries::of(&[("203.0.113.7", "JP"), ("8.8.8.8", "US")]);
        let meta = meta(serde_json::json!({
            "sourceIP": "203.0.113.7",
            "destinationIP": "8.8.8.8",
            // The index answers instead of the core.
            "destinationGeoIP": ["cn"],
        }));

        assert_eq!(
            locate_source(&meta, Some(&countries), LocalSourceIps::default()),
            "JP"
        );
        // No host: whatever the exit, it dialed this address.
        assert_eq!(
            locate_destination(&meta, Some("Proxy"), Some(&countries)),
            ("US".to_owned(), Some(GeoBasis::Dialed))
        );
    }

    #[test]
    fn an_address_the_index_does_not_know_is_unknown() {
        let countries = Countries::of(&[]);
        let meta = meta(serde_json::json!({
            "sourceIP": "192.168.1.2",
            "destinationIP": "not an address",
            "sourceGeoIP": ["cn"],
        }));

        assert_eq!(
            locate_source(&meta, Some(&countries), LocalSourceIps::default()),
            UNKNOWN
        );
        assert_eq!(
            locate_destination(&meta, Some("DIRECT"), Some(&countries)),
            (UNKNOWN.to_owned(), None)
        );
    }

    #[test]
    fn without_an_index_the_core_s_codes_answer() {
        let meta = meta(serde_json::json!({
            "sourceGeoIP": ["cn"],
            "destinationGeoIP": ["google", "us"],
        }));

        assert_eq!(locate_source(&meta, None, LocalSourceIps::default()), "CN");
        assert_eq!(
            locate_destination(&meta, Some("Proxy"), None),
            ("US".to_owned(), Some(GeoBasis::Dialed))
        );
    }

    #[test]
    fn local_sources_use_the_public_address_of_their_family() {
        let countries = Countries::of(&[("203.0.113.7", "JP"), ("2001:db8::7", "US")]);
        let local = LocalSourceIps {
            ipv4: Some("203.0.113.7".parse().unwrap()),
            ipv6: Some("2001:db8::7".parse().unwrap()),
        };
        for source in [
            "127.0.0.1",
            "127.7.8.9",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.2",
            "169.254.1.2",
            "0.0.0.0",
            "::ffff:192.168.1.2",
            "local",
            "LAN",
        ] {
            let meta = meta(serde_json::json!({ "sourceIP": source }));
            assert_eq!(
                locate_source(&meta, Some(&countries), local),
                "JP",
                "{source}"
            );
        }
        for source in ["::1", "::", "fc00::1", "fd12::7", "fe80::1"] {
            let meta = meta(serde_json::json!({ "sourceIP": source }));
            assert_eq!(
                locate_source(&meta, Some(&countries), local),
                "US",
                "{source}"
            );
        }
    }

    #[test]
    fn local_mapping_requires_an_answer_and_an_index() {
        let countries = Countries::of(&[("203.0.113.7", "JP")]);
        let ipv4_only = LocalSourceIps {
            ipv4: Some("203.0.113.7".parse().unwrap()),
            ipv6: None,
        };
        for source in ["192.168.1.2", "local", "::1"] {
            let meta = meta(serde_json::json!({ "sourceIP": source, "sourceGeoIP": ["cn"] }));
            assert_eq!(
                locate_source(&meta, Some(&countries), LocalSourceIps::default()),
                UNKNOWN
            );
            assert_eq!(locate_source(&meta, None, ipv4_only), UNKNOWN);
        }
        let meta = meta(serde_json::json!({ "sourceIP": "::1" }));
        assert_eq!(locate_source(&meta, Some(&countries), ipv4_only), UNKNOWN);
    }

    #[test]
    fn public_and_invalid_sources_are_never_replaced() {
        let countries = Countries::of(&[
            ("8.8.8.8", "US"),
            ("172.32.0.1", "DE"),
            ("203.0.113.7", "JP"),
        ]);
        let local = LocalSourceIps {
            ipv4: Some("203.0.113.7".parse().unwrap()),
            ipv6: None,
        };
        for (source, expected) in [
            ("8.8.8.8", "US"),
            ("::ffff:8.8.8.8", "US"),
            ("172.32.0.1", "DE"),
            ("", UNKNOWN),
            ("invalid", UNKNOWN),
        ] {
            let meta = meta(serde_json::json!({ "sourceIP": source }));
            assert_eq!(
                locate_source(&meta, Some(&countries), local),
                expected,
                "{source}"
            );
        }
    }

    /// The peer, the address the core resolved, and a proxy's entry each sit in their own country.
    fn countries() -> Countries {
        Countries::of(&[
            ("203.0.113.7", "US"),
            ("198.51.100.1", "CN"),
            ("192.0.2.1", "JP"),
        ])
    }

    fn destination_of(exit: &str, fields: serde_json::Value) -> (String, Option<GeoBasis>) {
        locate_destination(&meta(fields), Some(exit), Some(&countries()))
    }

    fn located(region: &str, basis: GeoBasis) -> (String, Option<GeoBasis>) {
        (region.to_owned(), Some(basis))
    }

    #[test]
    fn a_direct_tcp_connection_is_located_at_its_peer() {
        let fields = |destination_ip: serde_json::Value| {
            serde_json::json!({
                "network": "tcp",
                "host": "example.com",
                "destinationIP": destination_ip,
                "remoteDestination": "203.0.113.7",
            })
        };

        assert_eq!(
            destination_of("DIRECT", fields(serde_json::Value::Null)),
            located("US", GeoBasis::Dialed)
        );
        // The peer is what DIRECT dialed, whatever the core resolved for its rules.
        assert_eq!(
            destination_of("DIRECT", fields("198.51.100.1".into())),
            located("US", GeoBasis::Dialed)
        );
    }

    #[test]
    fn direct_udp_dials_the_address_it_resolved() {
        let fields = serde_json::json!({
            "network": "udp",
            "host": "example.com",
            "destinationIP": "198.51.100.1",
        });

        assert_eq!(
            destination_of("DIRECT", fields),
            located("CN", GeoBasis::Dialed)
        );
    }

    #[test]
    fn a_proxy_handed_the_host_only_has_the_core_s_resolution() {
        let fields = serde_json::json!({
            "network": "tcp",
            "host": "example.com",
            "destinationIP": "198.51.100.1",
            "remoteDestination": "192.0.2.1",
        });

        assert_eq!(
            destination_of("Node-A", fields),
            located("CN", GeoBasis::Resolved)
        );
    }

    #[test]
    fn a_proxy_s_entry_never_locates_the_destination() {
        let fields = serde_json::json!({
            "network": "tcp",
            "host": "example.com",
            "remoteDestination": "192.0.2.1",
        });

        assert_eq!(destination_of("Node-A", fields), (UNKNOWN.to_owned(), None));
    }

    #[test]
    fn a_proxy_dials_an_address_without_a_host_or_from_hosts() {
        let without_host = serde_json::json!({
            "network": "tcp",
            "destinationIP": "198.51.100.1",
        });
        let from_hosts = serde_json::json!({
            "network": "tcp",
            "host": "example.com",
            "dnsMode": "hosts",
            "destinationIP": "198.51.100.1",
        });

        assert_eq!(
            destination_of("Node-A", without_host),
            located("CN", GeoBasis::Dialed)
        );
        assert_eq!(
            destination_of("Node-A", from_hosts),
            located("CN", GeoBasis::Dialed)
        );
    }

    #[test]
    fn the_core_s_codes_keep_the_basis_of_the_address_they_describe() {
        let resolved = meta(serde_json::json!({
            "network": "tcp",
            "host": "example.com",
            "destinationIP": "198.51.100.1",
            "destinationGeoIP": ["us"],
        }));
        let unknown = meta(serde_json::json!({ "destinationGeoIP": [] }));

        assert_eq!(
            locate_destination(&resolved, Some("Node-A"), None),
            located("US", GeoBasis::Resolved)
        );
        assert_eq!(
            locate_destination(&unknown, Some("DIRECT"), None),
            (UNKNOWN.to_owned(), None)
        );
    }

    #[test]
    fn an_index_reads_one_country_among_category_tags() {
        let dat = crate::core::geo::fixtures::geoip_dat(&[
            ("US", &["8.0.0.0/8"]),
            ("GOOGLE", &["8.8.8.0/24"]),
        ]);
        let index = IpIndex::from_geoip_dat(&dat).unwrap();

        assert_eq!(
            index.country("8.8.8.8".parse().unwrap()).as_deref(),
            Some("US")
        );
        assert_eq!(index.country("9.9.9.9".parse().unwrap()), None);
    }
}
