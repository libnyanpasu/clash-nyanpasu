use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;
use time::OffsetDateTime;
use url::Url;

use super::*;

/// Who is responsible for maintaining the locally readable file.
///
/// `Remote + External` is unrepresentable: external binding exists only inside
/// the `Local` branch, while `Remote` owns a managed materialization directly.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileSource {
    Local {
        binding: LocalBinding,
    },

    Remote {
        #[serde(flatten)]
        materialized: MaterializedFile,

        url: Url,

        #[serde(default)]
        option: RemoteProfileOptions,

        #[serde(default, skip_serializing_if = "SubscriptionInfo::is_empty")]
        subscription: SubscriptionInfo,
    },
}

impl ProfileSource {
    pub fn materialized(&self) -> &MaterializedFile {
        match self {
            Self::Local { binding } => binding.materialized(),
            Self::Remote { materialized, .. } => materialized,
        }
    }

    pub fn materialized_mut(&mut self) -> &mut MaterializedFile {
        match self {
            Self::Local { binding } => binding.materialized_mut(),
            Self::Remote { materialized, .. } => materialized,
        }
    }

    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LocalBinding {
    Managed {
        #[serde(flatten)]
        materialized: MaterializedFile,
    },

    External {
        #[serde(flatten)]
        materialized: MaterializedFile,
        target: ExternalProfilePath,
        mode: ExternalMode,
    },
}

impl LocalBinding {
    pub fn materialized(&self) -> &MaterializedFile {
        match self {
            Self::Managed { materialized } | Self::External { materialized, .. } => materialized,
        }
    }

    pub fn materialized_mut(&mut self) -> &mut MaterializedFile {
        match self {
            Self::Managed { materialized } | Self::External { materialized, .. } => materialized,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExternalMode {
    Symlink,
    Mirror,
}

/// Stable read location used by parsers and processors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct MaterializedFile {
    pub file: ManagedProfilePath,

    /// Last successful materialization time.
    #[serde(
        default,
        with = "time::serde::timestamp::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[specta(type = Option<i64>)]
    pub updated_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, Serialize, Type)))]
pub struct RemoteProfileOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    pub user_agent: Option<String>,

    #[serde(default = "default_true")]
    pub with_proxy: bool,

    #[serde(default = "default_true")]
    pub self_proxy: bool,

    #[serde(default = "default_update_interval_minutes")]
    pub update_interval_minutes: u64,
}

impl Default for RemoteProfileOptions {
    fn default() -> Self {
        Self {
            user_agent: None,
            with_proxy: true,
            self_proxy: true,
            update_interval_minutes: default_update_interval_minutes(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct SubscriptionInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,

    #[serde(
        default,
        with = "time::serde::timestamp::option",
        skip_serializing_if = "Option::is_none"
    )]
    #[specta(type = Option<i64>)]
    pub expire: Option<OffsetDateTime>,
}

impl SubscriptionInfo {
    pub fn is_empty(&self) -> bool {
        self.upload.is_none()
            && self.download.is_none()
            && self.total.is_none()
            && self.expire.is_none()
    }
}

fn default_true() -> bool {
    true
}

fn default_update_interval_minutes() -> u64 {
    120
}
