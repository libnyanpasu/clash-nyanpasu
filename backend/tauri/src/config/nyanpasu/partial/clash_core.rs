use enumflags2::bitflags;
use serde::{Deserialize, Serialize};
use specta::Type;
use strum::{AsRefStr, Display, EnumString, IntoStaticStr};

// TODO: when support sing-box, remove this struct
#[bitflags]
#[repr(u8)]
#[derive(
    Debug,
    Clone,
    Copy,
    Deserialize,
    Serialize,
    PartialEq,
    Eq,
    Type,
    Display,
    AsRefStr,
    EnumString,
    IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum ClashCore {
    #[serde(rename = "clash", alias = "clash-premium")]
    ClashPremium = 0b0001,
    #[serde(rename = "clash-rs")]
    ClashRs,
    #[serde(rename = "mihomo", alias = "clash-meta")]
    Mihomo,
    #[serde(rename = "mihomo-alpha")]
    MihomoAlpha,
    #[serde(rename = "clash-rs-alpha")]
    ClashRsAlpha,
}

impl Default for ClashCore {
    fn default() -> Self {
        match cfg!(feature = "default-meta") {
            false => Self::ClashPremium,
            true => Self::Mihomo,
        }
    }
}

impl From<&ClashCore> for nyanpasu_utils::core::CoreType {
    fn from(core: &ClashCore) -> Self {
        match core {
            ClashCore::ClashPremium => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::ClashPremium,
            ),
            ClashCore::ClashRs => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::ClashRust,
            ),
            ClashCore::Mihomo => {
                nyanpasu_utils::core::CoreType::Clash(nyanpasu_utils::core::ClashCoreType::Mihomo)
            }
            ClashCore::MihomoAlpha => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::MihomoAlpha,
            ),
            ClashCore::ClashRsAlpha => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::ClashRustAlpha,
            ),
        }
    }
}

impl TryFrom<&nyanpasu_utils::core::CoreType> for ClashCore {
    type Error = anyhow::Error;

    fn try_from(core: &nyanpasu_utils::core::CoreType) -> Result<Self, Self::Error> {
        match core {
            nyanpasu_utils::core::CoreType::Clash(clash) => match clash {
                nyanpasu_utils::core::ClashCoreType::ClashPremium => Ok(ClashCore::ClashPremium),
                nyanpasu_utils::core::ClashCoreType::ClashRust => Ok(ClashCore::ClashRs),
                nyanpasu_utils::core::ClashCoreType::ClashRustAlpha => Ok(ClashCore::ClashRsAlpha),
                nyanpasu_utils::core::ClashCoreType::Mihomo => Ok(ClashCore::Mihomo),
                nyanpasu_utils::core::ClashCoreType::MihomoAlpha => Ok(ClashCore::MihomoAlpha),
            },
            _ => Err(anyhow::anyhow!("unsupported core type")),
        }
    }
}
