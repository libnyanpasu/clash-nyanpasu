use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;
use time::OffsetDateTime;
use url::Url;

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize, Type)]
pub struct SubscriptionInfo {
    /// Uploaded bytes
    #[serde(default)]
    pub upload: Option<usize>,
    /// Downloaded bytes
    #[serde(default)]
    pub download: Option<usize>,
    /// Total bytes
    #[serde(default)]
    pub total: Option<usize>,
    /// Expire time of the subscription.
    ///
    /// Original `profiles.yaml` stores this as a unix timestamp in seconds and
    /// uses `0` to mean "no expiry"; we keep that wire shape and map `0`/absent
    /// to `None`.
    #[specta(type = i64)]
    #[serde(with = "expire_serde", default)]
    pub expire: Option<OffsetDateTime>,
}

/// Serde adapter for [`SubscriptionInfo::expire`]: unix-seconds on the wire,
/// `0` (and absent) decoded as `None`, `None` encoded as `0`.
mod expire_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use time::OffsetDateTime;

    pub fn serialize<S: Serializer>(
        value: &Option<OffsetDateTime>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(value.map(|t| t.unix_timestamp()).unwrap_or(0))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<OffsetDateTime>, D::Error> {
        match Option::<i64>::deserialize(deserializer)? {
            None | Some(0) => Ok(None),
            Some(ts) => OffsetDateTime::from_unix_timestamp(ts)
                .map(Some)
                .map_err(serde::de::Error::custom),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, Serialize, Type)))]
pub struct RemoteProfileOptions {
    /// see issue #13
    #[serde(skip_serializing_if = "Option::is_none")]
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    pub user_agent: Option<String>,

    /// for `remote` profile
    /// use system proxy
    #[serde(default)]
    pub with_proxy: bool,

    /// use self proxy
    #[serde(default = "default_self_proxy")]
    pub self_proxy: bool,

    /// subscription update interval
    #[serde(alias = "update_interval")]
    #[patch(attribute(serde(alias = "update_interval")))]
    pub update_interval_minutes: u64,
}

/// Serde default for [`RemoteProfileOptions::self_proxy`]: matches the original
/// profile semantics where an absent `self_proxy` means "use self proxy".
fn default_self_proxy() -> bool {
    true
}

impl Default for RemoteProfileOptions {
    fn default() -> Self {
        Self {
            user_agent: None,
            with_proxy: true,
            self_proxy: default_self_proxy(),
            update_interval_minutes: 120,
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
