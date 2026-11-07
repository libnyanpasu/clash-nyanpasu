//! Transport values for profiles; application behavior remains in its owner.

use nyanpasu_config::profile::{FolderId, ProfileDefinition, ProfileMetadata, SubscriptionInfo};

/// An agent skill a core's release archive ships. The core updater keeps it in the
/// core's data directory, beside the executable it drives.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CoreAgentSkill {
    /// The skill's name, which is also the name of its folder and of its executable.
    pub name: String,
    /// The skill's folder, which agents link so that core updates refresh it in place.
    pub dir: String,
    /// Whether a core update has installed the skill.
    pub installed: bool,
}

/// A profile URL downloaded once so the user can confirm how to import it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ProfileUrlProbe {
    pub classification: ImportClassification,
    /// The kinds whose import validation accepts the content.
    pub accepted: Vec<ProfileImportKind>,
    /// `profile-title`, else the name the import would derive from the URL.
    pub suggested_name: String,
    pub subscription: SubscriptionInfo,
    pub suggested_update_interval_minutes: Option<u64>,
    /// Imported as is when the user picks a local import, so it is not
    /// downloaded twice.
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ProfileSyncStatus {
    pub scheduled: bool,
    pub next_run_at: Option<String>,
    pub active: Vec<nyanpasu_jobs::dto::RunDto>,
    pub journal_degraded: bool,
    pub registration_error: Option<String>,
    pub history_limit: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, PartialEq, Eq)]
pub struct ImportClassification {
    /// None when the signals do not settle on a kind.
    pub suggested: Option<ProfileImportKind>,
    /// Every signal found, whether or not it decided the suggestion.
    pub evidence: Vec<ImportEvidence>,
}

/// What a downloaded profile can be imported as.
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, PartialEq, Eq, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum ProfileImportKind {
    /// A Clash config file.
    Config,
    /// A merge: a YAML overlay applied to a config.
    Overlay,
    #[serde(rename = "javascript")]
    JavaScript,
    Lua,
}

/// A signal that a downloaded profile is of some kind.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportSignal {
    /// `subscription-userinfo` carries the traffic quota of a subscription.
    SubscriptionQuota,
    /// `profile-title` names a subscription.
    ProfileTitle,
    /// `profile-update-interval` asks a subscription to be refreshed.
    UpdateInterval,
    /// The extension of the file name in `Content-Disposition` or the URL path.
    FileExtension,
    /// The media type in `Content-Type`.
    ContentType,
    /// A YAML mapping with `proxies` or `proxy-providers`.
    ProxiesKey,
    /// A YAML mapping with `prepend-`, `append-`, `override__` or `filter__` keys.
    OverlayDirectives,
    /// Lines that read as the script language.
    ScriptSyntax,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, PartialEq, Eq)]
pub struct ImportEvidence {
    pub kind: ProfileImportKind,
    pub signal: ImportSignal,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct NewProfileRequest {
    pub metadata: ProfileMetadata,
    /// Add rewrites the materialized path to `{uid}.{ext}`.
    pub definition: ProfileDefinition,
    /// Folder the new Config profile joins in the same commit.
    #[serde(default)]
    pub folder: Option<FolderId>,
}
