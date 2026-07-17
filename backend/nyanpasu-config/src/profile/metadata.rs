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
    #[patch(attribute(specta(type = Option<Option<String>>)))]
    pub desc: Option<String>,

    /// Provenance flag: `true` when the name was chosen by the user (manual
    /// create or rename) and must not be overwritten by subscription name-sync.
    /// Profiles persisted before this field predate provenance tracking; absence
    /// means user-owned so a refresh cannot silently rename them.
    /// The value is never trusted from an incoming patch — it is set only by
    /// rename detection and name-sync (see `ProfileItem::apply_metadata_patch`).
    /// Only the non-default `false` (an unpinned, sync-eligible profile) is
    /// persisted; a user-owned `true` is left off the wire and restored by the
    /// default, so legacy and user-named documents stay byte-identical.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    // The patch field is optional in both directions (the UI never sends it and
    // `apply_metadata_patch` zeroes it anyway); `serde(default)` makes it absent-
    // tolerant on the deserialize side, mirroring `desc`.
    #[patch(attribute(serde(default)))]
    pub custom_name: bool,
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}
