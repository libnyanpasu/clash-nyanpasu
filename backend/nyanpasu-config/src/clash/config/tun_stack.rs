use serde::{Deserialize, Serialize};
use specta::Type;
use strum::AsRefStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type, AsRefStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TunStack {
    System,
    #[default]
    #[strum(serialize = "gvisor")]
    Gvisor,
    Mixed,
}
