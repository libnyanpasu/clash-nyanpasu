use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;
use strum::{AsRefStr, EnumString, VariantArray};

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    Default,
    Type,
    AsRefStr,
    EnumString,
    VariantArray,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum TunStack {
    System,
    #[default]
    #[strum(serialize = "gvisor")]
    Gvisor,
    Mixed,
    /// mihomo's own user-space network stack (Mihomo IP Stack), not the MIPS
    /// processor architecture.
    Mips,
}

/// mihomo `tun.congestion-controller`, read only by the mips stack.
#[derive(
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
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum TunCongestionController {
    Cubic,
    Reno,
    Bbr,
    Bbr3,
}

/// The options of the mips stack. Kept while another stack is selected, so
/// switching back restores them; read them through
/// `ClashConfig::selected_tun_stack`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, Type)))]
#[patch(attribute(serde(default)))]
#[serde(default)]
pub struct MipsConfig {
    /// `None` writes no key, so the core uses its own default.
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    #[patch(attribute(specta(type = Option<Option<TunCongestionController>>)))]
    pub congestion_controller: Option<TunCongestionController>,
}

/// The selected stack with only its own options reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SelectedTunStack<'a> {
    System,
    Gvisor,
    Mixed,
    Mips(&'a MipsConfig),
}

impl SelectedTunStack<'_> {
    pub fn stack(self) -> TunStack {
        match self {
            Self::System => TunStack::System,
            Self::Gvisor => TunStack::Gvisor,
            Self::Mixed => TunStack::Mixed,
            Self::Mips(_) => TunStack::Mips,
        }
    }
}
