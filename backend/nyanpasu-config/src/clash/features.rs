//! Which runtime config keys each Clash core reads, and which values it
//! accepts there. Config generation, final-config validation and the UI read
//! this one matrix instead of branching on core names
//! (docs/design/core-specific-config.md).
//!
//! The matrix is keyed by core kind only: Nyanpasu answers for the cores it
//! ships, not for builds a user swaps in.

use std::{str::FromStr, sync::Arc};

use serde::Serialize;
use strum::VariantArray;

use crate::{
    application::ClashCore,
    clash::config::{
        dns::{DnsKey, EnhancedMode, FakeIpFilterMode},
        tun_stack::{TunCongestionController, TunStack},
    },
    runtime::value::{ConfigObject, ConfigValue},
};

/// Whether a core reads one config key, and which values it accepts there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FieldSupport<T> {
    /// The core does not read the key: Nyanpasu writes none and checks none.
    Ignored,
    /// The core reads the key and fails on any value outside this list.
    Values(Vec<T>),
}

impl<T: VariantArray + Copy> FieldSupport<T> {
    /// Collects the values `reads` accepts, in declaration order; none at all
    /// means the core does not read the key.
    fn from_matrix(reads: impl Fn(T) -> bool) -> Self {
        let values: Vec<T> = T::VARIANTS
            .iter()
            .copied()
            .filter(|value| reads(*value))
            .collect();
        if values.is_empty() {
            Self::Ignored
        } else {
            Self::Values(values)
        }
    }
}

impl<T> FieldSupport<T> {
    pub fn is_ignored(&self) -> bool {
        matches!(self, Self::Ignored)
    }
}

impl<T: PartialEq> FieldSupport<T> {
    pub fn supports(&self, value: &T) -> bool {
        match self {
            Self::Ignored => false,
            Self::Values(values) => values.contains(value),
        }
    }
}

impl<T: PartialEq + FromStr + AsRef<str>> FieldSupport<T> {
    /// Reports the value at `key` in `tun` when Nyanpasu knows it and the core
    /// rejects it. A value Nyanpasu does not know passes, so the core decides:
    /// a core may gain a value before Nyanpasu does.
    fn check(&self, key: TunKey, tun: Option<&ConfigObject>) -> Option<UnsupportedConfigValue> {
        let Self::Values(supported) = self else {
            return None;
        };
        let Some(ConfigValue::String(text)) = key.get(tun) else {
            return None;
        };
        let value: T = text.parse().ok()?;
        if supported.contains(&value) {
            return None;
        }
        Some(UnsupportedConfigValue {
            path: format!("{}.{}", TunKey::TABLE, key.as_str()),
            value: text.to_string(),
            supported: supported
                .iter()
                .map(|value| value.as_ref().to_owned())
                .collect(),
        })
    }
}

/// A value in the final runtime config that the selected core rejects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
pub struct UnsupportedConfigValue {
    /// Dotted path in the final config, such as `tun.stack`.
    pub path: String,
    /// The value as written in the config.
    pub value: String,
    /// The values the core accepts at `path`.
    pub supported: Vec<String>,
}

/// What the selected core reads, by config domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct ConfigFeatures {
    pub tun: TunFeatures,
    pub dns: DnsFeatures,
}

impl ConfigFeatures {
    /// The values in `config` that the core rejects, wherever they came from.
    pub fn unsupported_values(&self, config: &ConfigValue) -> Vec<UnsupportedConfigValue> {
        let tun = config
            .as_object_arc()
            .and_then(|root| root.get(TunKey::TABLE))
            .and_then(ConfigValue::as_object_arc)
            .map(Arc::as_ref);
        let mut values: Vec<UnsupportedConfigValue> = self
            .tun
            .stack
            .check(TunKey::Stack, tun)
            .into_iter()
            .collect();
        // mihomo reads the controller only on the mips stack.
        let on_mips = matches!(
            TunKey::Stack.get(tun),
            Some(ConfigValue::String(stack)) if stack.parse() == Ok(TunStack::Mips)
        );
        if on_mips {
            values.extend(
                self.tun
                    .congestion_controller
                    .check(TunKey::CongestionController, tun),
            );
        }
        values
    }
}

/// The `tun` keys the matrix covers, spelled as in the runtime config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum TunKey {
    Stack,
    CongestionController,
}

impl TunKey {
    /// The runtime config mapping that holds these keys.
    pub const TABLE: &'static str = "tun";

    pub fn as_str(self) -> &'static str {
        self.into()
    }

    fn get(self, tun: Option<&ConfigObject>) -> Option<&ConfigValue> {
        tun?.get(self.as_str())
    }
}

/// What the selected core reads under `tun`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct TunFeatures {
    pub stack: FieldSupport<TunStack>,
    /// Read only while the stack is mips.
    pub congestion_controller: FieldSupport<TunCongestionController>,
}

impl TunFeatures {
    /// Written in place of a selected stack the core does not support. Every
    /// core that reads `stack` supports it.
    pub const FALLBACK_STACK: TunStack = TunStack::Gvisor;
}

/// What the selected core reads under `dns`, and in the `hosts` table that
/// `dns.use-hosts` consults. Nyanpasu writes neither table for a core whose
/// `keys` is [`FieldSupport::Ignored`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct DnsFeatures {
    /// The typed `dns` keys the core reads.
    pub keys: FieldSupport<DnsKey>,
    pub enhanced_mode: FieldSupport<EnhancedMode>,
    pub fake_ip_filter_mode: FieldSupport<FakeIpFilterMode>,
    /// Whether a `dns.nameserver-policy` or `hosts` value may be a list, and
    /// a `hosts` value a domain alias. Otherwise each is one address.
    pub list_values: bool,
    /// Whether the core reads `dns.fallback-filter.geosite`.
    pub fallback_filter_geosite: bool,
}

impl ClashCore {
    /// The capability matrix for this core.
    pub fn config_features(self) -> ConfigFeatures {
        use ClashCore::*;
        ConfigFeatures {
            tun: TunFeatures {
                stack: FieldSupport::from_matrix(|stack| reads_tun_stack(self, stack)),
                congestion_controller: FieldSupport::from_matrix(|controller| {
                    reads_tun_congestion_controller(self, controller)
                }),
            },
            dns: DnsFeatures {
                keys: FieldSupport::from_matrix(|key| reads_dns_key(self, key)),
                enhanced_mode: FieldSupport::from_matrix(|mode| reads_enhanced_mode(self, mode)),
                fake_ip_filter_mode: FieldSupport::from_matrix(|mode| {
                    reads_fake_ip_filter_mode(self, mode)
                }),
                list_values: match self {
                    Mihomo | MihomoAlpha | Meow | MeowAlpha => true,
                    ClashRs | ClashRsAlpha => false,
                },
                fallback_filter_geosite: match self {
                    Mihomo | MihomoAlpha => true,
                    ClashRs | ClashRsAlpha | Meow | MeowAlpha => false,
                },
            },
        }
    }
}

/// Checked against the shipped versions on 2026-11-14 (spec §6.1): mihomo
/// v1.19.34 `config/config.go` `RawDNS`; clash-rs v0.10.11
/// `clash-lib/src/config/def.rs` `DNS`; meow-rs v0.29.0
/// `crates/meow-config/src/raw.rs` `RawDns`. Each ignores keys it does not
/// know.
fn reads_dns_key(core: ClashCore, key: DnsKey) -> bool {
    use ClashCore::*;
    use DnsKey::*;
    match (core, key) {
        (
            Mihomo | MihomoAlpha | ClashRs | ClashRsAlpha | Meow | MeowAlpha,
            Enable
            | Listen
            | EnhancedMode
            | FakeIpRange
            | FakeIpFilter
            | UseHosts
            | DefaultNameserver
            | Nameserver
            | Fallback
            | ProxyServerNameserver
            | NameserverPolicy
            | FallbackFilter,
        ) => true,
        (
            Mihomo | MihomoAlpha,
            Ipv6
            | FakeIpRange6
            | FakeIpFilterMode
            | UseSystemHosts
            | RespectRules
            | PreferH3
            | DirectNameserver
            | DirectNameserverFollowPolicy,
        ) => true,
        (ClashRs | ClashRsAlpha, Ipv6 | RespectRules) => true,
        (
            ClashRs | ClashRsAlpha,
            FakeIpRange6
            | FakeIpFilterMode
            | UseSystemHosts
            | PreferH3
            | DirectNameserver
            | DirectNameserverFollowPolicy,
        ) => false,
        (
            Meow | MeowAlpha,
            FakeIpFilterMode | UseSystemHosts | DirectNameserver | DirectNameserverFollowPolicy,
        ) => true,
        (Meow | MeowAlpha, Ipv6 | RespectRules | PreferH3 | FakeIpRange6) => false,
    }
}

/// mihomo and clash-rs parse all three modes. meow-rs knows `fake-ip` and
/// `redir-host` and falls back to its default resolver mode, which is
/// `normal`, on anything else (`dns_parser.rs`).
fn reads_enhanced_mode(core: ClashCore, mode: EnhancedMode) -> bool {
    use ClashCore::*;
    use EnhancedMode::*;
    match (core, mode) {
        (Mihomo | MihomoAlpha | ClashRs | ClashRsAlpha | Meow | MeowAlpha, FakeIp | RedirHost) => {
            true
        }
        (Mihomo | MihomoAlpha | ClashRs | ClashRsAlpha, Normal) => true,
        (Meow | MeowAlpha, Normal) => false,
    }
}

/// `rule` is mihomo's own (`constant/dns.go` `FilterRule`); meow-rs reads the
/// two list modes and treats anything else as `blacklist`. clash-rs does not
/// read the key.
fn reads_fake_ip_filter_mode(core: ClashCore, mode: FakeIpFilterMode) -> bool {
    use ClashCore::*;
    use FakeIpFilterMode::*;
    match (core, mode) {
        (Mihomo | MihomoAlpha, Blacklist | Whitelist | Rule) => true,
        (Meow | MeowAlpha, Blacklist | Whitelist) => true,
        (Meow | MeowAlpha, Rule) => false,
        (ClashRs | ClashRsAlpha, Blacklist | Whitelist | Rule) => false,
    }
}

/// mihomo parses `tun.stack` case-insensitively into gVisor, System, Mixed and
/// Mips (`constant/tun.go`; Mips since v1.19.31, commit `ab405bad`) and fails
/// the whole config on anything else.
/// clash-rs has no `stack` key and ignores unknown keys
/// (`clash-lib/src/config/def.rs`, `TunConfig`); meow-rs runs lwIP only and
/// warns about `stack` before ignoring it (`meow-config/src/raw.rs`, `RawTun`).
fn reads_tun_stack(core: ClashCore, stack: TunStack) -> bool {
    use ClashCore::*;
    use TunStack::*;
    match (core, stack) {
        (Mihomo | MihomoAlpha, System | Gvisor | Mixed | Mips) => true,
        (ClashRs | ClashRsAlpha | Meow | MeowAlpha, System | Gvisor | Mixed | Mips) => false,
    }
}

/// mihomo added `tun.congestion-controller` in v1.19.32 (commit `63bd52ec`).
/// Its mips stack lowercases the value and rejects any controller outside
/// `mipstack.AvailableCongestionControls()` when the TUN starts
/// (`metacubex/sing-tun` `stack_mipstack.go`); the other stacks never read it.
/// No other core has the key.
fn reads_tun_congestion_controller(core: ClashCore, controller: TunCongestionController) -> bool {
    use ClashCore::*;
    use TunCongestionController::*;
    match (core, controller) {
        (Mihomo | MihomoAlpha, Cubic | Reno | Bbr | Bbr3) => true,
        (ClashRs | ClashRsAlpha | Meow | MeowAlpha, Cubic | Reno | Bbr | Bbr3) => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn config(json: serde_json::Value) -> ConfigValue {
        ConfigValue::try_from(json).unwrap()
    }

    #[test]
    fn every_core_has_its_tun_features() {
        use TunCongestionController::*;
        use TunStack::*;
        let mihomo = TunFeatures {
            stack: FieldSupport::Values(vec![System, Gvisor, Mixed, Mips]),
            congestion_controller: FieldSupport::Values(vec![Cubic, Reno, Bbr, Bbr3]),
        };
        let ignored = TunFeatures {
            stack: FieldSupport::Ignored,
            congestion_controller: FieldSupport::Ignored,
        };
        let cases = [
            (ClashCore::Mihomo, mihomo.clone()),
            (ClashCore::MihomoAlpha, mihomo),
            (ClashCore::ClashRs, ignored.clone()),
            (ClashCore::ClashRsAlpha, ignored.clone()),
            (ClashCore::Meow, ignored.clone()),
            (ClashCore::MeowAlpha, ignored),
        ];
        for (core, tun) in cases {
            assert_eq!(core.config_features().tun, tun, "{core}");
        }
    }

    #[test]
    fn the_fallback_stack_is_supported_wherever_the_stack_is_read() {
        for core in [
            ClashCore::ClashRs,
            ClashCore::Mihomo,
            ClashCore::MihomoAlpha,
            ClashCore::ClashRsAlpha,
            ClashCore::Meow,
            ClashCore::MeowAlpha,
        ] {
            let stack = core.config_features().tun.stack;
            if let FieldSupport::Values(_) = stack {
                assert!(stack.supports(&TunFeatures::FALLBACK_STACK), "{core}");
            }
        }
    }

    #[test]
    fn a_known_unsupported_stack_is_reported() {
        let features = ConfigFeatures {
            tun: TunFeatures {
                stack: FieldSupport::Values(vec![TunStack::System, TunStack::Gvisor]),
                congestion_controller: FieldSupport::Ignored,
            },
            ..ClashCore::Mihomo.config_features()
        };
        assert_eq!(
            features.unsupported_values(&config(json!({ "tun": { "stack": "Mixed" } }))),
            vec![UnsupportedConfigValue {
                path: "tun.stack".into(),
                value: "Mixed".into(),
                supported: vec!["system".into(), "gvisor".into()],
            }]
        );
    }

    #[test]
    fn supported_unknown_and_ignored_stacks_pass() {
        let features = ClashCore::Mihomo.config_features();
        for stack in [json!("gVisor"), json!("system"), json!("future"), json!(1)] {
            let config = config(json!({ "tun": { "enable": false, "stack": stack } }));
            assert!(features.unsupported_values(&config).is_empty(), "{stack}");
        }
        assert!(features.unsupported_values(&config(json!({}))).is_empty());

        let config = config(json!({ "tun": { "stack": "mixed" } }));
        assert!(
            ClashCore::ClashRs
                .config_features()
                .unsupported_values(&config)
                .is_empty()
        );
    }

    #[test]
    fn the_congestion_controller_is_checked_only_on_the_mips_stack() {
        let features = ConfigFeatures {
            tun: TunFeatures {
                stack: FieldSupport::Values(vec![TunStack::Gvisor, TunStack::Mips]),
                congestion_controller: FieldSupport::Values(vec![TunCongestionController::Cubic]),
            },
            dns: ClashCore::Mihomo.config_features().dns,
        };
        let on = |stack: &str, controller: &str| {
            features.unsupported_values(&config(json!({
                "tun": { "stack": stack, "congestion-controller": controller },
            })))
        };
        assert_eq!(
            on("Mips", "BBR"),
            vec![UnsupportedConfigValue {
                path: "tun.congestion-controller".into(),
                value: "BBR".into(),
                supported: vec!["cubic".into()],
            }]
        );
        assert!(on("mips", "cubic").is_empty());
        assert!(on("mips", "future").is_empty());
        assert!(on("gvisor", "bbr").is_empty());
    }

    #[test]
    fn support_serializes_adjacently_tagged() {
        assert_eq!(
            serde_json::to_value(ClashCore::Mihomo.config_features()).unwrap()["tun"],
            json!({
                "stack": { "kind": "values", "value": ["system", "gvisor", "mixed", "mips"] },
                "congestion_controller": { "kind": "values", "value": ["cubic", "reno", "bbr", "bbr3"] },
            })
        );
    }

    /// Spec §6.1, one row per core family.
    #[test]
    fn every_core_has_its_dns_features() {
        let unread = |core: ClashCore| -> Vec<DnsKey> {
            let keys = core.config_features().dns.keys;
            DnsKey::VARIANTS
                .iter()
                .copied()
                .filter(|key| !keys.supports(key))
                .collect()
        };
        for core in [ClashCore::Mihomo, ClashCore::MihomoAlpha] {
            assert!(unread(core).is_empty(), "{core}");
            let dns = core.config_features().dns;
            assert!(dns.list_values && dns.fallback_filter_geosite, "{core}");
            assert_eq!(
                dns.fake_ip_filter_mode,
                FieldSupport::Values(vec![
                    FakeIpFilterMode::Blacklist,
                    FakeIpFilterMode::Whitelist,
                    FakeIpFilterMode::Rule,
                ])
            );
        }
        for core in [ClashCore::ClashRs, ClashCore::ClashRsAlpha] {
            assert_eq!(
                unread(core),
                vec![
                    DnsKey::FakeIpRange6,
                    DnsKey::FakeIpFilterMode,
                    DnsKey::UseSystemHosts,
                    DnsKey::PreferH3,
                    DnsKey::DirectNameserver,
                    DnsKey::DirectNameserverFollowPolicy,
                ],
                "{core}"
            );
            let dns = core.config_features().dns;
            assert!(!dns.list_values && !dns.fallback_filter_geosite, "{core}");
            assert_eq!(dns.fake_ip_filter_mode, FieldSupport::Ignored);
        }
        for core in [ClashCore::Meow, ClashCore::MeowAlpha] {
            assert_eq!(
                unread(core),
                vec![
                    DnsKey::Ipv6,
                    DnsKey::FakeIpRange6,
                    DnsKey::RespectRules,
                    DnsKey::PreferH3,
                ],
                "{core}"
            );
            let dns = core.config_features().dns;
            assert!(dns.list_values && !dns.fallback_filter_geosite, "{core}");
            assert_eq!(
                dns.enhanced_mode,
                FieldSupport::Values(vec![EnhancedMode::FakeIp, EnhancedMode::RedirHost])
            );
            assert_eq!(
                dns.fake_ip_filter_mode,
                FieldSupport::Values(vec![
                    FakeIpFilterMode::Blacklist,
                    FakeIpFilterMode::Whitelist,
                ])
            );
        }
    }
}
