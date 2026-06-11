use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;
use url::Url;

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, Type)]
pub struct SubscriptionInfo {
    /// Uploaded bytes
    pub upload: Option<usize>,
    /// Downloaded bytes
    pub download: Option<usize>,
    /// Total bytes
    pub total: Option<usize>,
    #[specta(type = Option<String>)]
    #[serde(with = "time::serde::rfc3339::option")]
    /// Expire time of the subscription
    pub expire: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Type, Builder)]
#[builder(default, derive(Debug, Deserialize, Serialize, Type))]
pub struct RemoteProfileOptions {
    /// see issue #13
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,

    /// for `remote` profile
    /// use system proxy
    pub with_proxy: bool,

    /// use self proxy
    pub self_proxy: bool,

    /// subscription update interval
    #[serde(alias = "update_interval")]
    pub update_interval_seconds: u64,
}

impl Default for RemoteProfileOptions {
    fn default() -> Self {
        Self {
            user_agent: None,
            with_proxy: true,
            self_proxy: false,
            update_interval_seconds: 3600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RemoteSource {
    /// profile url
    pub url: Url,

    #[serde(default)]
    pub option: RemoteProfileOptions,

    #[serde(default)]
    pub extra: SubscriptionInfo,

    #[serde(default, alias = "chains", skip_serializing_if = "Vec::is_empty")]
    pub chain: Vec<super::kind::ProfileId>,
}
