//! Native command values; native channel descriptors remain shell bindings.

use nyanpasu_config::application::ReleaseChannel;

#[derive(serde::Serialize, Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NativeDialogKind {
    Info,
    Warning,
    Error,
}

#[derive(serde::Serialize, Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NativeDialogButtons {
    Ok,
    OkCancel,
    YesNo,
    YesNoCancel,
    OkCustom {
        ok: String,
    },
    OkCancelCustom {
        ok: String,
        cancel: String,
    },
    YesNoCancelCustom {
        yes: String,
        no: String,
        cancel: String,
    },
}

#[derive(serde::Serialize, Debug, Clone, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileDialogFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[derive(serde::Deserialize, Debug, specta::Type, serde::Serialize)]
pub struct ConfigBackupInfo {
    pub name: String,
    pub path: String,
}

#[derive(serde::Deserialize, Debug, Clone, serde::Serialize, specta::Type)]
pub struct ReleaseChannelInfo {
    /// The feed update checks follow.
    pub current: ReleaseChannel,
    /// The channel of the installed build; a nightly build cannot leave Nightly.
    pub installed: ReleaseChannel,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, strum::EnumString, specta::Type)]
#[strum(serialize_all = "kebab-case")]
pub enum CopyEnvOption {
    #[serde(rename = "shell")]
    Shell,
    #[serde(rename = "cmd")]
    Cmd,
    #[serde(rename = "pwsh")]
    Pwsh,
}

/// Type of content the editor window displays.
/// Used to derive the window label (for singleton logic) and URL path params.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum EditorWindowType {
    Profile,
    CssEditor,
}

/// Identifies one `subscribe_clash_connection_details` call, for a later
/// `unsubscribe_clash_connection_details`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct SubscriptionId(pub u64);
