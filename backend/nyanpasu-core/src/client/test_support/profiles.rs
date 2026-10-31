//! Profile values and mailbox observations shared by application and host tests.

use crate::{
    client::profiles::ProfilesClient,
    state::profiles::{
        NewProfileRequest, ProfilesActorMessage, ProfilesError,
        ports::{FetchedSubscription, MockSubscriptionFetcher},
        sources::SourceStatus,
    },
};
use nyanpasu_config::profile::{
    ConfigDefinition, ExternalMode, ExternalProfilePath, FileConfig, LocalBinding,
    ManagedProfilePath, MaterializedFile, OverlayTransform, ProfileDefinition, ProfileId,
    ProfileMetadata, ProfileSource, Profiles, RemoteProfileOptions, SubscriptionInfo,
    TransformDefinition,
};
pub fn add_placeholder_request() -> NewProfileRequest {
    NewProfileRequest {
        metadata: file_config_item("placeholder").metadata,
        definition: file_config_item("placeholder").definition,
        folder: None,
    }
}

pub fn file_config_item(uid: &str) -> nyanpasu_config::profile::ProfileItem {
    nyanpasu_config::profile::ProfileItem {
        uid: ProfileId(uid.into()),
        metadata: ProfileMetadata {
            name: uid.to_uppercase(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(format!("{uid}.yaml")).unwrap(),
                            updated_at: None,
                        },
                    },
                },
                transforms: vec![],
            }),
        },
    }
}

pub fn remote_config_item(uid: &str) -> nyanpasu_config::profile::ProfileItem {
    nyanpasu_config::profile::ProfileItem {
        uid: ProfileId(uid.into()),
        metadata: ProfileMetadata {
            name: uid.to_uppercase(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Remote {
                    materialized: MaterializedFile {
                        file: ManagedProfilePath::new(format!("{uid}.yaml")).unwrap(),
                        updated_at: None,
                    },
                    url: url::Url::parse("https://example.com/sub").unwrap(),
                    option: RemoteProfileOptions::default(),
                    subscription: SubscriptionInfo::default(),
                },
                transforms: vec![],
            }),
        },
    }
}

pub fn external_item(
    uid: &str,
    mode: ExternalMode,
    target: ExternalProfilePath,
) -> nyanpasu_config::profile::ProfileItem {
    nyanpasu_config::profile::ProfileItem {
        uid: ProfileId(uid.into()),
        metadata: ProfileMetadata {
            name: uid.to_uppercase(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::External {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(format!("{uid}.yaml")).unwrap(),
                            updated_at: None,
                        },
                        target,
                        mode,
                    },
                },
                transforms: vec![],
            }),
        },
    }
}

pub fn external_path(path: &std::path::Path) -> ExternalProfilePath {
    ExternalProfilePath::new(path.to_string_lossy().into_owned()).unwrap()
}

pub fn ok_fetch(content: &'static str) -> MockSubscriptionFetcher {
    suggested_fetch(content, None)
}

pub(crate) fn suggested_fetch(
    content: &'static str,
    suggested_update_interval_minutes: Option<u64>,
) -> MockSubscriptionFetcher {
    let mut fetcher = MockSubscriptionFetcher::new();
    fetcher.expect_fetch().returning(move |_, _| {
        Ok(FetchedSubscription {
            content: content.to_string(),
            filename: None,
            subscription: SubscriptionInfo {
                upload: Some(1),
                ..Default::default()
            },
            suggested_update_interval_minutes,
            content_type: None,
            file_name: None,
        })
    });
    fetcher
}

/// A round trip through the mailbox: every message cast before it has
/// been handled once it returns, whatever the gate answers.
pub async fn round_trip(client: &ProfilesClient) {
    let result = client.refresh(ProfileId("ghost".into()), None).await;
    assert!(
        !matches!(
            result,
            Err(ProfilesError::ProfilesActorStopped | ProfilesError::ProfilesReplyDropped)
        ),
        "profiles actor must survive: {result:?}"
    );
}

pub async fn wait_for_source(
    client: &ProfilesClient,
    uid: &str,
    accept: impl Fn(&SourceStatus) -> bool,
) -> SourceStatus {
    let mut sources = client.subscribe_sources();
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let found = sources
                .borrow_and_update()
                .entries
                .iter()
                .find(|status| status.profile.0 == uid && accept(status))
                .cloned();
            if let Some(status) = found {
                return status;
            }
            sources.changed().await.expect("profiles actor is alive");
        }
    })
    .await
    .expect("source receipt did not arrive")
}

pub fn yaml(profiles: &Profiles) -> String {
    serde_yaml_ng::to_string(profiles).unwrap()
}

pub fn cast(client: &ProfilesClient, message: ProfilesActorMessage) {
    client.cast_for_test(message);
}

pub(crate) fn overlay_item(uid: &str) -> nyanpasu_config::profile::ProfileItem {
    nyanpasu_config::profile::ProfileItem {
        uid: ProfileId(uid.into()),
        metadata: ProfileMetadata {
            name: uid.to_uppercase(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(OverlayTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(format!("{uid}.yaml")).unwrap(),
                            updated_at: None,
                        },
                    },
                },
            }),
        },
    }
}

pub fn seeded_profiles() -> Profiles {
    let mut profiles = Profiles::default();
    profiles.append_item(file_config_item("cfg1"));
    profiles.append_item(file_config_item("cfg2"));
    profiles.append_item(overlay_item("ovl1"));
    profiles
}
