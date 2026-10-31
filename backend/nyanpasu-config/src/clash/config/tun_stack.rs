use serde::{Deserialize, Serialize};
use specta::Type;
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
}
