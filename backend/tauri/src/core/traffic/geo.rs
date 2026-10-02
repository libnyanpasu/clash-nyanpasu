//! Regions of a connection's ends, looked up in the app's own country index.
use std::net::IpAddr;

use clash_api::api::ConnectionMetadataFields;
use nyanpasu_geodata::IpIndex;
use nyanpasu_traffic::normalize_region;

const UNKNOWN: &str = "unknown";

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

fn locate(
    ip: Option<&String>,
    core_codes: Option<&Vec<String>>,
    index: Option<&dyn CountryLookup>,
) -> String {
    match index {
        Some(index) => address(ip)
            .and_then(|ip| index.country(ip))
            .unwrap_or_else(|| UNKNOWN.to_owned()),
        // Before the index loads, the core's codes are the only answer.
        None => normalize_region(core_codes.into_iter().flatten()),
    }
}

pub(crate) fn locate_source(
    meta: &ConnectionMetadataFields,
    index: Option<&dyn CountryLookup>,
) -> String {
    locate(meta.source_ip.as_ref(), meta.source_geo_ip.as_ref(), index)
}

pub(crate) fn locate_destination(
    meta: &ConnectionMetadataFields,
    index: Option<&dyn CountryLookup>,
) -> String {
    locate(
        meta.destination_ip.as_ref(),
        meta.destination_geo_ip.as_ref(),
        index,
    )
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

        assert_eq!(locate_source(&meta, Some(&countries)), "JP");
        assert_eq!(locate_destination(&meta, Some(&countries)), "US");
    }

    #[test]
    fn an_address_the_index_does_not_know_is_unknown() {
        let countries = Countries::of(&[]);
        let meta = meta(serde_json::json!({
            "sourceIP": "192.168.1.2",
            "destinationIP": "not an address",
            "sourceGeoIP": ["cn"],
        }));

        assert_eq!(locate_source(&meta, Some(&countries)), UNKNOWN);
        assert_eq!(locate_destination(&meta, Some(&countries)), UNKNOWN);
    }

    #[test]
    fn without_an_index_the_core_s_codes_answer() {
        let meta = meta(serde_json::json!({
            "sourceGeoIP": ["cn"],
            "destinationGeoIP": ["google", "us"],
        }));

        assert_eq!(locate_source(&meta, None), "CN");
        assert_eq!(locate_destination(&meta, None), "US");
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
