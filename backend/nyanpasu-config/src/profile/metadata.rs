use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;

/// Public, user-editable profile metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, Type)))]
pub struct ProfileMetadata {
    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    pub desc: Option<String>,
}
