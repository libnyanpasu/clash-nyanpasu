//! The `dns` and `hosts` tables Nyanpasu writes over the profiles while the
//! user takes them over (docs/spec/2026-11-14-dns-settings). Pure data and the
//! rendering of it for the selected core's [`DnsFeatures`].

use std::net::IpAddr;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use strum::{AsRefStr, EnumString, IntoStaticStr, VariantArray};

use crate::{clash::features::DnsFeatures, runtime::value::ConfigValue};

/// Who decides one runtime config table.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ControlSource {
    /// The profiles and transforms decide the table.
    #[default]
    Profile,
    /// Nyanpasu writes the whole table over whatever the profiles produced.
    Nyanpasu,
}

/// mihomo `dns.enhanced-mode`.
#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    Type,
    AsRefStr,
    EnumString,
    VariantArray,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum EnhancedMode {
    /// Every core's default when the key is absent.
    Normal,
    #[default]
    FakeIp,
    RedirHost,
}

/// mihomo `dns.fake-ip-filter-mode`.
#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    Type,
    AsRefStr,
    EnumString,
    VariantArray,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum FakeIpFilterMode {
    #[default]
    Blacklist,
    Whitelist,
    /// The filter entries are rules (`DOMAIN-SUFFIX,lan,real-ip`), not
    /// domains.
    Rule,
}

/// A `nameserver-policy` or `hosts` value: one entry or a list of them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn as_slice(&self) -> &[String] {
        match self {
            Self::One(value) => std::slice::from_ref(value),
            Self::Many(values) => values,
        }
    }
}

/// mihomo `dns.fallback-filter`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(default, rename_all = "kebab-case")]
pub struct FallbackFilter {
    pub geoip: bool,
    pub geoip_code: String,
    pub ipcidr: Vec<String>,
    pub domain: Vec<String>,
    /// Read by mihomo only.
    pub geosite: Vec<String>,
}

impl Default for FallbackFilter {
    fn default() -> Self {
        Self {
            geoip: true,
            geoip_code: "CN".to_owned(),
            ipcidr: strings(&["240.0.0.0/4", "0.0.0.0/32"]),
            domain: strings(&["+.google.com", "+.facebook.com", "+.youtube.com"]),
            geosite: Vec::new(),
        }
    }
}

/// The `dns` table Nyanpasu writes while it controls DNS. Field names are the
/// runtime config keys. Upstream addresses are kept as written: the core
/// checks their syntax.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Type)]
#[serde(default, rename_all = "kebab-case")]
pub struct DnsSettings {
    pub enable: bool,
    pub listen: Option<String>,
    pub ipv6: bool,
    pub enhanced_mode: EnhancedMode,
    pub fake_ip_range: Option<String>,
    pub fake_ip_range6: Option<String>,
    pub fake_ip_filter_mode: FakeIpFilterMode,
    pub fake_ip_filter: Vec<String>,
    pub use_hosts: bool,
    pub use_system_hosts: bool,
    pub respect_rules: bool,
    pub prefer_h3: bool,
    pub default_nameserver: Vec<String>,
    pub nameserver: Vec<String>,
    pub fallback: Vec<String>,
    pub proxy_server_nameserver: Vec<String>,
    pub direct_nameserver: Vec<String>,
    pub direct_nameserver_follow_policy: bool,
    /// Ordered: the first matching key wins.
    pub nameserver_policy: IndexMap<String, OneOrMany>,
    pub fallback_filter: FallbackFilter,
    /// Keys without a typed field, such as `cache-algorithm`, written as they
    /// are after the typed ones. A key that has a typed field is ignored.
    #[specta(type = IndexMap<String, specta_typescript::Any>)]
    pub extra: IndexMap<String, serde_json::Value>,
}

/// The defaults Clash Verge Rev and Clash Party agree on; spec §3.2 records
/// the choice where they differ.
impl Default for DnsSettings {
    fn default() -> Self {
        let doh = strings(&[
            "https://doh.pub/dns-query",
            "https://dns.alidns.com/dns-query",
        ]);
        Self {
            enable: true,
            listen: None,
            ipv6: false,
            enhanced_mode: EnhancedMode::FakeIp,
            fake_ip_range: Some("198.18.0.1/16".to_owned()),
            fake_ip_range6: None,
            fake_ip_filter_mode: FakeIpFilterMode::Blacklist,
            fake_ip_filter: strings(&[
                "*",
                "+.lan",
                "+.local",
                "*.arpa",
                "time.*.com",
                "ntp.*.com",
                "+.market.xiaomi.com",
                "localhost.ptlogin2.qq.com",
                "*.msftncsi.com",
                "www.msftconnecttest.com",
            ]),
            use_hosts: false,
            use_system_hosts: false,
            respect_rules: false,
            prefer_h3: false,
            default_nameserver: strings(&["tls://223.5.5.5"]),
            nameserver: doh.clone(),
            fallback: Vec::new(),
            proxy_server_nameserver: doh,
            direct_nameserver: Vec::new(),
            direct_nameserver_follow_policy: false,
            nameserver_policy: IndexMap::new(),
            fallback_filter: FallbackFilter::default(),
            extra: IndexMap::new(),
        }
    }
}

/// The typed keys of [`DnsSettings`], spelled as in the runtime config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type, IntoStaticStr, VariantArray)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum DnsKey {
    Enable,
    Listen,
    Ipv6,
    EnhancedMode,
    FakeIpRange,
    FakeIpRange6,
    FakeIpFilterMode,
    FakeIpFilter,
    UseHosts,
    UseSystemHosts,
    RespectRules,
    PreferH3,
    DefaultNameserver,
    Nameserver,
    Fallback,
    ProxyServerNameserver,
    DirectNameserver,
    DirectNameserverFollowPolicy,
    NameserverPolicy,
    FallbackFilter,
}

impl DnsKey {
    /// The runtime config mapping that holds these keys.
    pub const TABLE: &'static str = "dns";

    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// A table rendered for the selected core, with what had to be left out.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedTable {
    pub table: ConfigValue,
    /// One line per setting the core cannot take as saved.
    pub warnings: Vec<String>,
}

impl DnsSettings {
    /// The `dns` table for a core with `features`, or `None` when the core
    /// has no `dns` table Nyanpasu writes. Keys the core does not read are
    /// left out; a saved value that differs from the default gets a warning.
    pub fn render(&self, features: &DnsFeatures) -> Option<RenderedTable> {
        if features.keys.is_ignored() {
            return None;
        }
        let defaults = Self::default();
        let mut warnings = Vec::new();
        let mut table = serde_json::Map::new();

        // A filter mode the core does not apply turns a rule or allow list
        // into a deny list, so the list is dropped with it.
        let drop_filter = self.fake_ip_filter_mode != FakeIpFilterMode::Blacklist
            && !features
                .fake_ip_filter_mode
                .supports(&self.fake_ip_filter_mode);
        if drop_filter {
            warnings.push(format!(
                "the selected core does not support `dns.fake-ip-filter-mode: {}`; \
                 `fake-ip-filter-mode` and `fake-ip-filter` are not written",
                self.fake_ip_filter_mode.as_ref(),
            ));
        }

        for &key in DnsKey::VARIANTS {
            if drop_filter && matches!(key, DnsKey::FakeIpFilterMode | DnsKey::FakeIpFilter) {
                continue;
            }
            if !features.keys.supports(&key) {
                let mut unread = Vec::new();
                if self.value(key, features, &mut unread)
                    != defaults.value(key, features, &mut unread)
                {
                    warnings.push(format!(
                        "the selected core does not read `dns.{}`; it is not written",
                        key.as_str(),
                    ));
                }
                continue;
            }
            if key == DnsKey::EnhancedMode && !features.enhanced_mode.supports(&self.enhanced_mode)
            {
                // `normal` is every core's default, so leaving it out keeps it.
                if self.enhanced_mode != EnhancedMode::Normal {
                    warnings.push(format!(
                        "the selected core does not support `dns.enhanced-mode: {}`; \
                         it is not written",
                        self.enhanced_mode.as_ref(),
                    ));
                }
                continue;
            }
            if let Some(value) = self.value(key, features, &mut warnings) {
                table.insert(key.as_str().to_owned(), value);
            }
        }
        for (key, value) in &self.extra {
            if DnsKey::VARIANTS.iter().any(|typed| typed.as_str() == key) {
                continue;
            }
            table.insert(key.clone(), value.clone());
        }

        Some(RenderedTable {
            table: ConfigValue::from_json_value(serde_json::Value::Object(table)),
            warnings,
        })
    }

    /// The value written at `key`, or `None` when nothing is written. Values
    /// the core reads only in part are reduced here, with a warning.
    fn value(
        &self,
        key: DnsKey,
        features: &DnsFeatures,
        warnings: &mut Vec<String>,
    ) -> Option<serde_json::Value> {
        use serde_json::json;
        let value = match key {
            DnsKey::Enable => json!(self.enable),
            DnsKey::Listen => json!(self.listen.as_ref()?),
            DnsKey::Ipv6 => json!(self.ipv6),
            DnsKey::EnhancedMode => json!(self.enhanced_mode),
            DnsKey::FakeIpRange => json!(self.fake_ip_range.as_ref()?),
            DnsKey::FakeIpRange6 => json!(self.fake_ip_range6.as_ref()?),
            DnsKey::FakeIpFilterMode => json!(self.fake_ip_filter_mode),
            DnsKey::FakeIpFilter => json!(self.fake_ip_filter),
            DnsKey::UseHosts => json!(self.use_hosts),
            DnsKey::UseSystemHosts => json!(self.use_system_hosts),
            DnsKey::RespectRules => json!(self.respect_rules),
            DnsKey::PreferH3 => json!(self.prefer_h3),
            DnsKey::DefaultNameserver => json!(self.default_nameserver),
            DnsKey::Nameserver => json!(self.nameserver),
            DnsKey::Fallback => json!(self.fallback),
            DnsKey::ProxyServerNameserver => json!(self.proxy_server_nameserver),
            DnsKey::DirectNameserver => json!(self.direct_nameserver),
            DnsKey::DirectNameserverFollowPolicy => json!(self.direct_nameserver_follow_policy),
            DnsKey::NameserverPolicy => {
                if self.nameserver_policy.is_empty() {
                    return None;
                }
                let policy: serde_json::Map<String, serde_json::Value> = self
                    .nameserver_policy
                    .iter()
                    .map(|(domain, servers)| {
                        let value = single_or_list(
                            servers,
                            features.list_values,
                            || format!("dns.nameserver-policy.{domain}"),
                            warnings,
                        );
                        (domain.clone(), value)
                    })
                    .collect();
                serde_json::Value::Object(policy)
            }
            DnsKey::FallbackFilter => {
                let mut filter = json!({
                    "geoip": self.fallback_filter.geoip,
                    "geoip-code": self.fallback_filter.geoip_code,
                    "ipcidr": self.fallback_filter.ipcidr,
                    "domain": self.fallback_filter.domain,
                });
                if !self.fallback_filter.geosite.is_empty() {
                    if features.fallback_filter_geosite {
                        filter["geosite"] = json!(self.fallback_filter.geosite);
                    } else {
                        warnings.push(
                            "the selected core does not read `dns.fallback-filter.geosite`; \
                             it is not written"
                                .to_owned(),
                        );
                    }
                }
                filter
            }
        };
        Some(value)
    }
}

/// The `hosts` table for a core with `features`, or `None` when the core has
/// no `hosts` table Nyanpasu writes. An empty result means Nyanpasu removes
/// the table.
pub fn render_hosts(
    hosts: &IndexMap<String, OneOrMany>,
    features: &DnsFeatures,
) -> Option<RenderedTable> {
    if features.keys.is_ignored() {
        return None;
    }
    let mut warnings = Vec::new();
    let mut table = serde_json::Map::new();
    for (domain, targets) in hosts {
        if !features.list_values {
            // Without list values the core reads an IP address only.
            if let Some(alias) = targets
                .as_slice()
                .iter()
                .find(|target| target.parse::<IpAddr>().is_err())
            {
                warnings.push(format!(
                    "the selected core does not read the alias `{alias}` in `hosts.{domain}`; \
                     the entry is not written"
                ));
                continue;
            }
        }
        let value = single_or_list(
            targets,
            features.list_values,
            || format!("hosts.{domain}"),
            &mut warnings,
        );
        table.insert(domain.clone(), value);
    }
    Some(RenderedTable {
        table: ConfigValue::from_json_value(serde_json::Value::Object(table)),
        warnings,
    })
}

/// `values` as written, or its first entry for a core that reads a single
/// one.
fn single_or_list(
    values: &OneOrMany,
    list_values: bool,
    path: impl FnOnce() -> String,
    warnings: &mut Vec<String>,
) -> serde_json::Value {
    match values {
        OneOrMany::One(value) => serde_json::Value::String(value.clone()),
        OneOrMany::Many(list) if list_values => serde_json::json!(list),
        OneOrMany::Many(list) => {
            if list.len() > 1 {
                warnings.push(format!(
                    "the selected core reads one entry at `{}`; only the first is written",
                    path(),
                ));
            }
            serde_json::Value::String(list.first().cloned().unwrap_or_default())
        }
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::application::ClashCore;

    fn features(core: ClashCore) -> DnsFeatures {
        core.config_features().dns
    }

    fn rendered(settings: &DnsSettings, core: ClashCore) -> (serde_json::Value, Vec<String>) {
        let RenderedTable { table, warnings } = settings
            .render(&features(core))
            .expect("every shipped core has a dns table");
        (table.to_json(), warnings)
    }

    /// Every serialized field but `extra` is a typed key, spelled the same,
    /// so no setting can miss the runtime config.
    #[test]
    fn typed_keys_match_the_serialized_fields() {
        let serialized = serde_json::to_value(DnsSettings::default()).unwrap();
        let fields: Vec<&str> = serialized
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .filter(|key| *key != "extra")
            .collect();
        let keys: Vec<&str> = DnsKey::VARIANTS.iter().map(|key| key.as_str()).collect();
        assert_eq!(fields, keys);
    }

    #[test]
    fn mihomo_gets_every_set_key_in_field_order() {
        let (table, warnings) = rendered(&DnsSettings::default(), ClashCore::Mihomo);
        assert!(warnings.is_empty(), "{warnings:?}");
        let keys: Vec<&str> = table
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "enable",
                "ipv6",
                "enhanced-mode",
                "fake-ip-range",
                "fake-ip-filter-mode",
                "fake-ip-filter",
                "use-hosts",
                "use-system-hosts",
                "respect-rules",
                "prefer-h3",
                "default-nameserver",
                "nameserver",
                "fallback",
                "proxy-server-nameserver",
                "direct-nameserver",
                "direct-nameserver-follow-policy",
                "fallback-filter",
            ]
        );
        assert_eq!(table["enhanced-mode"], "fake-ip");
        assert_eq!(table["fallback-filter"]["geoip-code"], "CN");
    }

    /// Unread keys are left out; only a value the user changed is reported.
    #[test]
    fn unread_keys_warn_only_when_changed() {
        let (table, warnings) = rendered(&DnsSettings::default(), ClashCore::ClashRs);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(table.get("use-system-hosts").is_none());
        assert!(table.get("fake-ip-filter-mode").is_none());
        assert_eq!(table["respect-rules"], false);

        let settings = DnsSettings {
            prefer_h3: true,
            ..DnsSettings::default()
        };
        let (table, warnings) = rendered(&settings, ClashCore::ClashRs);
        assert!(table.get("prefer-h3").is_none());
        assert_eq!(
            warnings,
            vec!["the selected core does not read `dns.prefer-h3`; it is not written"]
        );
    }

    /// A rule list read as a domain list would match nothing it means to, so
    /// both keys go.
    #[test]
    fn an_unsupported_filter_mode_drops_its_list() {
        let settings = DnsSettings {
            fake_ip_filter_mode: FakeIpFilterMode::Rule,
            fake_ip_filter: vec!["DOMAIN-SUFFIX,lan,real-ip".into()],
            ..DnsSettings::default()
        };
        for core in [ClashCore::Meow, ClashCore::ClashRs] {
            let (table, warnings) = rendered(&settings, core);
            assert!(table.get("fake-ip-filter-mode").is_none(), "{core}");
            assert!(table.get("fake-ip-filter").is_none(), "{core}");
            assert_eq!(warnings.len(), 1, "{core}: {warnings:?}");
        }
        let (table, warnings) = rendered(&settings, ClashCore::Mihomo);
        assert_eq!(table["fake-ip-filter-mode"], "rule");
        assert!(warnings.is_empty());
    }

    #[test]
    fn normal_mode_is_left_out_where_it_is_the_default() {
        let settings = DnsSettings {
            enhanced_mode: EnhancedMode::Normal,
            ..DnsSettings::default()
        };
        let (table, warnings) = rendered(&settings, ClashCore::Meow);
        assert!(table.get("enhanced-mode").is_none());
        assert!(warnings.is_empty(), "{warnings:?}");
        let (table, _) = rendered(&settings, ClashCore::ClashRs);
        assert_eq!(table["enhanced-mode"], "normal");
    }

    #[test]
    fn policy_lists_are_reduced_for_single_value_cores() {
        let settings = DnsSettings {
            nameserver_policy: IndexMap::from([
                (
                    "+.lan".to_owned(),
                    OneOrMany::Many(vec!["system".into(), "223.5.5.5".into()]),
                ),
                ("geosite:cn".to_owned(), OneOrMany::One("223.5.5.5".into())),
            ]),
            ..DnsSettings::default()
        };
        let (table, warnings) = rendered(&settings, ClashCore::Mihomo);
        assert_eq!(
            table["nameserver-policy"],
            json!({ "+.lan": ["system", "223.5.5.5"], "geosite:cn": "223.5.5.5" })
        );
        assert!(warnings.is_empty());

        let (table, warnings) = rendered(&settings, ClashCore::ClashRs);
        assert_eq!(
            table["nameserver-policy"],
            json!({ "+.lan": "system", "geosite:cn": "223.5.5.5" })
        );
        assert_eq!(
            warnings,
            vec![
                "the selected core reads one entry at `dns.nameserver-policy.+.lan`; only the \
                 first is written"
            ]
        );
    }

    #[test]
    fn extra_keys_follow_and_never_replace_typed_ones() {
        let settings = DnsSettings {
            enable: false,
            extra: IndexMap::from([
                ("cache-algorithm".to_owned(), json!("arc")),
                ("enable".to_owned(), json!(true)),
            ]),
            ..DnsSettings::default()
        };
        let (table, _) = rendered(&settings, ClashCore::Mihomo);
        assert_eq!(table["enable"], false);
        let keys: Vec<&str> = table
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys.last(), Some(&"cache-algorithm"));
    }

    #[test]
    fn hosts_keep_lists_and_aliases_only_where_read() {
        let hosts = IndexMap::from([
            (
                "router.lan".to_owned(),
                OneOrMany::One("192.168.1.1".into()),
            ),
            (
                "nas.lan".to_owned(),
                OneOrMany::Many(vec!["10.0.0.2".into(), "fd00::2".into()]),
            ),
            (
                "mirror.lan".to_owned(),
                OneOrMany::One("example.com".into()),
            ),
        ]);
        let RenderedTable { table, warnings } =
            render_hosts(&hosts, &features(ClashCore::Mihomo)).unwrap();
        assert_eq!(
            table.to_json(),
            json!({
                "router.lan": "192.168.1.1",
                "nas.lan": ["10.0.0.2", "fd00::2"],
                "mirror.lan": "example.com",
            })
        );
        assert!(warnings.is_empty());

        let RenderedTable { table, warnings } =
            render_hosts(&hosts, &features(ClashCore::ClashRs)).unwrap();
        assert_eq!(
            table.to_json(),
            json!({ "router.lan": "192.168.1.1", "nas.lan": "10.0.0.2" })
        );
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }

    /// A field absent from the saved file takes its default, so a file
    /// written before a field existed still loads.
    #[test]
    fn a_partial_document_fills_in_defaults() {
        let settings: DnsSettings =
            yaml_serde::from_str("enable: false\nnameserver-policy:\n  +.lan: [system]\n").unwrap();
        assert!(!settings.enable);
        assert_eq!(settings.nameserver, DnsSettings::default().nameserver);
        assert_eq!(
            settings.nameserver_policy["+.lan"],
            OneOrMany::Many(vec!["system".into()])
        );
    }
}
