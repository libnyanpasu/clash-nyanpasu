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
    clash::config::tun_stack::{TunCongestionController, TunStack},
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
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

impl ClashCore {
    /// The capability matrix for this core.
    pub fn config_features(self) -> ConfigFeatures {
        ConfigFeatures {
            tun: TunFeatures {
                stack: FieldSupport::from_matrix(|stack| reads_tun_stack(self, stack)),
                congestion_controller: FieldSupport::from_matrix(|controller| {
                    reads_tun_congestion_controller(self, controller)
                }),
            },
        }
    }
}

/// mihomo parses `tun.stack` case-insensitively into gVisor, System, Mixed and
/// Mips (`constant/tun.go`; Mips since v1.19.31, commit `ab405bad`) and fails
/// the whole config on anything else. Clash Premium is closed source; it never
/// offered Mixed, which the legacy derivation downgraded, and predates Mips.
/// clash-rs has no `stack` key and ignores unknown keys
/// (`clash-lib/src/config/def.rs`, `TunConfig`); meow-rs runs lwIP only and
/// warns about `stack` before ignoring it (`meow-config/src/raw.rs`, `RawTun`).
fn reads_tun_stack(core: ClashCore, stack: TunStack) -> bool {
    use ClashCore::*;
    use TunStack::*;
    match (core, stack) {
        (Mihomo | MihomoAlpha, System | Gvisor | Mixed | Mips) => true,
        (ClashPremium, System | Gvisor) => true,
        (ClashPremium, Mixed | Mips) => false,
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
        (ClashPremium | ClashRs | ClashRsAlpha | Meow | MeowAlpha, Cubic | Reno | Bbr | Bbr3) => {
            false
        }
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
            (
                ClashCore::ClashPremium,
                TunFeatures {
                    stack: FieldSupport::Values(vec![System, Gvisor]),
                    congestion_controller: FieldSupport::Ignored,
                },
            ),
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
            ClashCore::ClashPremium,
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
        let features = ClashCore::ClashPremium.config_features();
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
        let premium = ClashCore::ClashPremium.config_features();
        for stack in [json!("gVisor"), json!("system"), json!("future"), json!(1)] {
            let config = config(json!({ "tun": { "enable": false, "stack": stack } }));
            assert!(premium.unsupported_values(&config).is_empty(), "{stack}");
        }
        assert!(premium.unsupported_values(&config(json!({}))).is_empty());

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
            serde_json::to_value(ClashCore::ClashPremium.config_features()).unwrap(),
            json!({ "tun": {
                "stack": { "kind": "values", "value": ["system", "gvisor"] },
                "congestion_controller": { "kind": "ignored" },
            } })
        );
    }
}
