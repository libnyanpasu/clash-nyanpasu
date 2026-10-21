use serde::{Deserialize, Serialize};
use struct_patch::Patch;

use super::ManageableField;

/// The `tun` keys Nyanpasu may force while TUN mode is on. The TUN switch and
/// the stack stay separate settings.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    specta::Type,
    Patch,
    nyanpasu_macro::OwnedFields,
)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)))]
#[patch(attribute(serde(default, rename_all = "kebab-case")))]
#[serde(rename_all = "kebab-case")]
pub struct TunOverrides {
    /// Managed `true` by default. Unmanaged leaves a profile's value, such
    /// as `global`, and adds none when the profiles have none.
    pub auto_route: ManageableField<bool>,
}

impl Default for TunOverrides {
    fn default() -> Self {
        Self {
            auto_route: ManageableField::Managed(true),
        }
    }
}
