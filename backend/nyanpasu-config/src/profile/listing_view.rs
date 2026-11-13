//! The profile listing a `read` caller may see.
//!
//! [`Profiles`] holds each subscription's URL, its custom user agent, the external file path and
//! the free-form description. [`ProfilesListing`] keeps the structure a dashboard or a CLI needs:
//! ids, names, kinds, the selected profile, the order, the folders and the transform wiring. A
//! secret-bearing field becomes a presence flag, never a filtered string; a form that edits a
//! subscription reads the full document.

use indexmap::IndexMap;
use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use super::{
    CompositionConfig, ConfigDefinition, ExternalMode, FileConfig, LocalBinding, ProfileDefinition,
    ProfileFolder, ProfileId, ProfileItem, ProfileMetadata, ProfileSource, ProxyGroupLayout,
    RemoteProfileOptions, RemoteSyncInterval, ScriptRuntime, SubscriptionInfo, TransformDefinition,
};

/// The secret-free view of [`Profiles`].
#[derive(Debug, Clone, Serialize, Type)]
pub struct ProfilesListing {
    pub current: Option<ProfileId>,
    pub global_transforms: Vec<ProfileId>,
    pub folders: Vec<ProfileFolder>,
    /// The sidebar arrangement of each profile's proxy groups. The names are the groups
    /// `get_proxies` already returns, and the folder names are the user's own labels.
    pub proxy_group_layouts: IndexMap<ProfileId, ProxyGroupLayout>,
    pub items: Vec<ProfileListingItem>,
}

/// One profile without its source secrets.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ProfileListingItem {
    pub uid: ProfileId,
    #[serde(flatten)]
    pub metadata: ProfileListingMetadata,
    #[serde(flatten)]
    pub definition: ProfileListingDefinition,
}

/// The name stays; the description is free text a user may paste a link into, so it is a flag.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ProfileListingMetadata {
    pub name: String,
    pub custom_name: bool,
    pub has_desc: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileListingDefinition {
    Config { config: ProfileListingConfig },
    Transform { transform: ProfileListingTransform },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileListingConfig {
    File {
        source: ProfileListingSource,
        transforms: Vec<ProfileId>,
    },
    /// Ids only: it holds no source.
    Composition {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<ProfileId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        extend_proxies_from: Vec<ProfileId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        transforms: Vec<ProfileId>,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileListingTransform {
    Overlay {
        source: ProfileListingSource,
    },
    Script {
        source: ProfileListingSource,
        runtime: ScriptRuntime,
    },
}

/// Where a profile's content comes from, without the URL, the file paths or the request headers.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileListingSource {
    Local {
        binding: ProfileListingBinding,
    },
    Remote {
        #[serde(
            default,
            with = "time::serde::timestamp::option",
            skip_serializing_if = "Option::is_none"
        )]
        #[specta(type = Option<i64>)]
        updated_at: Option<OffsetDateTime>,
        option: RemoteListingOptions,
        /// Byte counts and an expiry: numbers cannot carry a credential.
        #[serde(skip_serializing_if = "SubscriptionInfo::is_empty")]
        subscription: SubscriptionInfo,
    },
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileListingBinding {
    Managed {
        #[serde(
            default,
            with = "time::serde::timestamp::option",
            skip_serializing_if = "Option::is_none"
        )]
        #[specta(type = Option<i64>)]
        updated_at: Option<OffsetDateTime>,
    },
    /// The target path is the user's file system layout: only its mode is listed.
    External {
        #[serde(
            default,
            with = "time::serde::timestamp::option",
            skip_serializing_if = "Option::is_none"
        )]
        #[specta(type = Option<i64>)]
        updated_at: Option<OffsetDateTime>,
        mode: ExternalMode,
    },
}

/// The sync policy; the user agent is a request header value, so it is a flag.
#[derive(Debug, Clone, Serialize, Type)]
pub struct RemoteListingOptions {
    pub has_user_agent: bool,
    pub with_proxy: bool,
    pub self_proxy: bool,
    pub update_interval_minutes: RemoteSyncInterval,
}

impl From<&ProfileItem> for ProfileListingItem {
    fn from(item: &ProfileItem) -> Self {
        let ProfileItem {
            uid,
            metadata,
            definition,
        } = item;
        let ProfileMetadata {
            name,
            desc,
            custom_name,
        } = metadata;

        Self {
            uid: uid.clone(),
            metadata: ProfileListingMetadata {
                name: name.clone(),
                custom_name: *custom_name,
                has_desc: desc.is_some(),
            },
            definition: definition.into(),
        }
    }
}

impl From<&ProfileDefinition> for ProfileListingDefinition {
    fn from(definition: &ProfileDefinition) -> Self {
        match definition {
            ProfileDefinition::Config { config } => Self::Config {
                config: match config {
                    ConfigDefinition::File(FileConfig { source, transforms }) => {
                        ProfileListingConfig::File {
                            source: source.into(),
                            transforms: transforms.clone(),
                        }
                    }
                    ConfigDefinition::Composition(CompositionConfig {
                        base,
                        extend_proxies_from,
                        transforms,
                    }) => ProfileListingConfig::Composition {
                        base: base.clone(),
                        extend_proxies_from: extend_proxies_from.clone(),
                        transforms: transforms.clone(),
                    },
                },
            },
            ProfileDefinition::Transform { transform } => Self::Transform {
                transform: match transform {
                    TransformDefinition::Overlay(overlay) => ProfileListingTransform::Overlay {
                        source: (&overlay.source).into(),
                    },
                    TransformDefinition::Script(script) => ProfileListingTransform::Script {
                        source: (&script.source).into(),
                        runtime: script.runtime,
                    },
                },
            },
        }
    }
}

impl From<&ProfileSource> for ProfileListingSource {
    fn from(source: &ProfileSource) -> Self {
        match source {
            ProfileSource::Local { binding } => Self::Local {
                binding: match binding {
                    LocalBinding::Managed { materialized } => ProfileListingBinding::Managed {
                        updated_at: materialized.updated_at,
                    },
                    LocalBinding::External {
                        materialized,
                        // A file system path: not listed.
                        target: _,
                        mode,
                    } => ProfileListingBinding::External {
                        updated_at: materialized.updated_at,
                        mode: *mode,
                    },
                },
            },
            ProfileSource::Remote {
                materialized,
                // The subscription URL carries the token.
                url: _,
                option,
                subscription,
            } => {
                let RemoteProfileOptions {
                    user_agent,
                    with_proxy,
                    self_proxy,
                    update_interval_minutes,
                } = option;
                Self::Remote {
                    updated_at: materialized.updated_at,
                    option: RemoteListingOptions {
                        has_user_agent: user_agent.is_some(),
                        with_proxy: *with_proxy,
                        self_proxy: *self_proxy,
                        update_interval_minutes: *update_interval_minutes,
                    },
                    subscription: {
                        let SubscriptionInfo {
                            upload,
                            download,
                            total,
                            expire,
                        } = subscription;
                        SubscriptionInfo {
                            upload: *upload,
                            download: *download,
                            total: *total,
                            expire: *expire,
                        }
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{super::Profiles, *};

    const URL_PATH_TOKEN: &str = "tok_7f3a91c2d4e85b06";
    const URL_HOST_TOKEN: &str = "sub-7f3a91c2.example-panel.invalid";
    const URL_USERINFO: &str = "alice:hunter2-userinfo";
    const URL_QUERY_TOKEN: &str = "query-secret-31337";
    const USER_AGENT: &str = "Bearer ua-bearer-secret-5150";
    const DESC: &str = "from https://notes.invalid/?key=desc-secret-8675309";
    const EXTERNAL_PATH: &str = "/home/alice/private-sentinel/config.yaml";

    fn fixture() -> Profiles {
        serde_yaml_ng::from_str(&format!(
            r#"current: remote
global_transforms: [shared]
valid: [dns, secret-field-name]
folders:
  - id: work
    name: Work
    members: [remote, external]
proxy_group_layouts:
  remote:
    folders:
      - id: media
        name: Media
        members: [Streaming]
    ungrouped: [Proxy]
items:
  - uid: remote
    name: Remote
    desc: "{DESC}"
    type: config
    config:
      type: file
      source:
        type: remote
        file: remote.yaml
        updated_at: 1720954186
        url: "https://{URL_USERINFO}@{URL_HOST_TOKEN}/{URL_PATH_TOKEN}/sub?token={URL_QUERY_TOKEN}"
        option:
          user_agent: "{USER_AGENT}"
          with_proxy: false
          self_proxy: true
          update_interval_minutes: 30
        subscription:
          upload: 1
          download: 2
          total: 10
          expire: 1999999999
      transforms: [shared]
  - uid: external
    name: External
    type: config
    config:
      type: file
      source:
        type: local
        binding:
          type: external
          file: external.yaml
          target: {EXTERNAL_PATH}
          mode: mirror
  - uid: shared
    name: Shared
    type: transform
    transform:
      type: script
      runtime: lua
      source:
        type: local
        binding:
          type: managed
          file: shared.lua
"#
        ))
        .expect("fixture parses")
    }

    #[test]
    fn listing_keeps_no_source_secret() {
        let listing = ProfilesListing::from(&fixture());
        let wire = serde_json::to_string(&listing).unwrap();
        for sentinel in [
            URL_PATH_TOKEN,
            URL_HOST_TOKEN,
            URL_USERINFO,
            URL_QUERY_TOKEN,
            "ua-bearer-secret",
            "desc-secret",
            "notes.invalid",
            "private-sentinel",
            EXTERNAL_PATH,
            "remote.yaml",
            "secret-field-name",
        ] {
            assert!(!wire.contains(sentinel), "{sentinel} leaked: {wire}");
        }
        assert!(!wire.contains("\"url\""), "{wire}");
        assert!(!wire.contains("\"target\""), "{wire}");
    }

    #[test]
    fn listing_keeps_structure_and_turns_secrets_into_flags() {
        let listing = serde_json::to_value(ProfilesListing::from(&fixture())).unwrap();
        assert_eq!(
            listing,
            json!({
                "current": "remote",
                "global_transforms": ["shared"],
                "folders": [{ "id": "work", "name": "Work", "members": ["remote", "external"] }],
                "proxy_group_layouts": {
                    "remote": {
                        "folders": [{ "id": "media", "name": "Media", "members": ["Streaming"] }],
                        "ungrouped": ["Proxy"]
                    }
                },
                "items": [
                    {
                        "uid": "remote",
                        "name": "Remote",
                        "custom_name": true,
                        "has_desc": true,
                        "type": "config",
                        "config": {
                            "type": "file",
                            "source": {
                                "type": "remote",
                                "updated_at": 1720954186,
                                "option": {
                                    "has_user_agent": true,
                                    "with_proxy": false,
                                    "self_proxy": true,
                                    "update_interval_minutes": 30
                                },
                                "subscription": {
                                    "upload": 1,
                                    "download": 2,
                                    "total": 10,
                                    "expire": 1999999999
                                }
                            },
                            "transforms": ["shared"]
                        }
                    },
                    {
                        "uid": "external",
                        "name": "External",
                        "custom_name": true,
                        "has_desc": false,
                        "type": "config",
                        "config": {
                            "type": "file",
                            "source": {
                                "type": "local",
                                "binding": { "type": "external", "mode": "mirror" }
                            },
                            "transforms": []
                        }
                    },
                    {
                        "uid": "shared",
                        "name": "Shared",
                        "custom_name": true,
                        "has_desc": false,
                        "type": "transform",
                        "transform": {
                            "type": "script",
                            "runtime": "lua",
                            "source": { "type": "local", "binding": { "type": "managed" } }
                        }
                    }
                ]
            })
        );
    }

    #[test]
    fn the_full_document_still_holds_the_original_secrets() {
        let wire = serde_json::to_string(&fixture()).unwrap();
        for original in [
            URL_PATH_TOKEN,
            URL_QUERY_TOKEN,
            "ua-bearer-secret",
            EXTERNAL_PATH,
        ] {
            assert!(
                wire.contains(original),
                "{original} missing from the full document"
            );
        }
    }
}
