//! Typed client for the ProfilesActor. Read Some(5s) / write None.

use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::profile::{ProfileId, Profiles};
use nyanpasu_core::state::PersistentStateManagerSetup;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};

use crate::state::profiles::{
    CommitReport, NewProfileRequest, ProfilesActor, ProfilesActorArgs, ProfilesActorMessage,
    ProfilesError,
    ports::{ProfileFsPort, RebuildNotifier, SubscriptionFetcher},
};

pub const PROFILES_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct ProfilesClient {
    inner: Arc<ProfilesClientInner>,
}

struct ProfilesClientInner {
    actor_ref: ActorRef<ProfilesActorMessage>,
}

impl ProfilesClient {
    pub(crate) async fn new(
        profiles_path: Utf8PathBuf,
        fs: Arc<dyn ProfileFsPort>,
        fetcher: Arc<dyn SubscriptionFetcher>,
        notifier: Arc<dyn RebuildNotifier>,
    ) -> anyhow::Result<Self> {
        let should_load = profiles_path.exists();
        let setup = PersistentStateManagerSetup::<Profiles>::builder()
            .config_path(profiles_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load profiles persistent state manager")?
        } else {
            setup
                .from_state(Profiles::default())
                .await
                .context("failed to initialize profiles persistent state manager")?
        };

        manager
            .snapshot_handle()
            .load()
            .state
            .validate()
            .map_err(|errors| anyhow::anyhow!("profiles.yaml failed validation: {errors:?}"))?;

        let actor_ref = Actor::spawn(
            None,
            ProfilesActor,
            ProfilesActorArgs {
                manager,
                fs,
                fetcher,
                notifier,
            },
        )
        .await
        .context("failed to spawn profiles actor")?
        .0;

        Ok(Self {
            inner: Arc::new(ProfilesClientInner { actor_ref }),
        })
    }

    pub async fn get(&self) -> Result<Arc<Profiles>, ProfilesError> {
        self.call(ProfilesActorMessage::Get, Some(PROFILES_READ_TIMEOUT))
            .await
    }

    pub async fn set_current(
        &self,
        current: Option<ProfileId>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SetCurrent { current, reply },
            None,
        )
        .await
    }

    pub async fn set_global_transforms(
        &self,
        ids: Vec<ProfileId>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SetGlobalTransforms { ids, reply },
            None,
        )
        .await
    }

    pub async fn replace(&self, profiles: Profiles) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::Replace { profiles, reply },
            None,
        )
        .await
    }

    pub async fn add(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::Add {
                request,
                initial_file,
                reply,
            },
            None,
        )
        .await
    }

    pub async fn delete(&self, uid: ProfileId) -> Result<CommitReport, ProfilesError> {
        self.call(|reply| ProfilesActorMessage::Delete { uid, reply }, None)
            .await
    }

    async fn call<F, T>(&self, make: F, timeout: Option<Duration>) -> Result<T, ProfilesError>
    where
        F: FnOnce(RpcReplyPort<Result<T, ProfilesError>>) -> ProfilesActorMessage,
        T: Send + 'static,
    {
        match self.inner.actor_ref.call(make, timeout).await {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::SenderError) => Err(ProfilesError::Rpc("reply dropped".into())),
            Ok(CallResult::Timeout) => Err(ProfilesError::Rpc("call timed out".into())),
            Err(e) => Err(ProfilesError::Rpc(e.to_string())),
        }
    }
}

impl Drop for ProfilesClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::profiles::ports::{
        MockProfileFsPort, MockRebuildNotifier, MockSubscriptionFetcher,
    };
    use nyanpasu_config::profile::{
        ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
        OverlayTransform, ProfileDefinition, ProfileId, ProfileMetadata, ProfileSource, Profiles,
        ScriptRuntime, ScriptTransform, TransformDefinition,
    };
    use tempfile::{TempDir, tempdir};

    pub(crate) fn temp_profiles_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).expect("utf-8 temp path")
    }

    pub(crate) async fn test_client_with(fs: MockProfileFsPort) -> (ProfilesClient, TempDir) {
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            std::sync::Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .expect("profiles client should spawn");
        (client, dir)
    }

    #[tokio::test]
    async fn fresh_store_starts_with_default_profiles() {
        let (client, _dir) = test_client_with(MockProfileFsPort::new()).await;
        let snapshot = client.get().await.expect("get should succeed");
        assert!(snapshot.current.is_none());
        assert!(snapshot.items.is_empty());
        assert_eq!(snapshot.valid.len(), 3);
    }

    pub(crate) fn file_config_item(uid: &str) -> nyanpasu_config::profile::ProfileItem {
        nyanpasu_config::profile::ProfileItem {
            uid: ProfileId(uid.into()),
            metadata: ProfileMetadata {
                name: uid.to_uppercase(),
                desc: None,
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

    pub(crate) fn overlay_item(uid: &str) -> nyanpasu_config::profile::ProfileItem {
        nyanpasu_config::profile::ProfileItem {
            uid: ProfileId(uid.into()),
            metadata: ProfileMetadata {
                name: uid.to_uppercase(),
                desc: None,
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

    pub(crate) fn seeded_profiles() -> Profiles {
        let mut profiles = Profiles::default();
        profiles.append_item(file_config_item("cfg1"));
        profiles.append_item(file_config_item("cfg2"));
        profiles.append_item(overlay_item("ovl1"));
        profiles
    }

    async fn seeded_client() -> (ProfilesClient, TempDir) {
        let (client, dir) = test_client_with(MockProfileFsPort::new()).await;
        client
            .replace(seeded_profiles())
            .await
            .expect("seed replace");
        (client, dir)
    }

    #[tokio::test]
    async fn set_current_commits_and_reports_affects_current() {
        let (client, dir) = seeded_client().await;
        let report = client
            .set_current(Some(ProfileId("cfg1".into())))
            .await
            .expect("activate cfg1");
        assert!(report.affects_current);
        assert_eq!(report.snapshot.current, Some(ProfileId("cfg1".into())));

        let report = client
            .set_current(Some(ProfileId("cfg1".into())))
            .await
            .unwrap();
        assert!(!report.affects_current);

        drop(client);
        let (client, _dir2) = {
            let path = temp_profiles_path(&dir);
            let client = ProfilesClient::new(
                path,
                std::sync::Arc::new(MockProfileFsPort::new()),
                std::sync::Arc::new(MockSubscriptionFetcher::new()),
                std::sync::Arc::new(MockRebuildNotifier::new()),
            )
            .await
            .unwrap();
            (client, dir)
        };
        assert_eq!(
            client.get().await.unwrap().current,
            Some(ProfileId("cfg1".into()))
        );
    }

    #[tokio::test]
    async fn set_current_rejects_missing_and_transform_targets() {
        let (client, _dir) = seeded_client().await;
        let err = client
            .set_current(Some(ProfileId("ghost".into())))
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed(_)));
        let err = client
            .set_current(Some(ProfileId("ovl1".into())))
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed(_)));
        assert!(client.get().await.unwrap().current.is_none());
    }

    #[tokio::test]
    async fn set_global_transforms_validates_kind_and_reports_change() {
        let (client, _dir) = seeded_client().await;
        let report = client
            .set_global_transforms(vec![ProfileId("ovl1".into())])
            .await
            .expect("set transforms");
        assert!(report.affects_current);

        let err = client
            .set_global_transforms(vec![ProfileId("cfg1".into())])
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed(_)));
    }

    #[tokio::test]
    async fn add_generates_uid_canonical_path_and_writes_initial_file() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_write_atomic()
            .withf(|path, content| path.as_str().ends_with(".yaml") && content == "proxies: []\n")
            .times(1)
            .returning(|_, _| Ok(()));
        let (client, _dir) = test_client_with(fs).await;

        let report = client
            .add(
                NewProfileRequest {
                    metadata: ProfileMetadata {
                        name: "New".into(),
                        desc: None,
                    },
                    definition: file_config_item("placeholder").definition,
                },
                Some("proxies: []\n".to_string()),
            )
            .await
            .expect("add should succeed");

        assert!(!report.affects_current);
        assert!(report.warnings.is_empty());
        let snapshot = report.snapshot;
        assert_eq!(snapshot.items.len(), 1);
        let (uid, item) = snapshot.items.first().unwrap();
        assert!(uid.0.starts_with('c'), "config uid prefixed with c: {uid}");
        let file = item
            .definition
            .source()
            .unwrap()
            .materialized()
            .file
            .as_str();
        assert_eq!(file, format!("{uid}.yaml"));
    }

    #[tokio::test]
    async fn add_script_transform_uses_runtime_extension() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_write_atomic()
            .withf(|path, _| path.as_str().ends_with(".lua"))
            .times(1)
            .returning(|_, _| Ok(()));
        let (client, _dir) = test_client_with(fs).await;
        let mut item = overlay_item("placeholder");
        item.definition = ProfileDefinition::Transform {
            transform: TransformDefinition::Script(ScriptTransform {
                source: overlay_item("p").definition.source().unwrap().clone(),
                runtime: ScriptRuntime::Lua,
            }),
        };
        let report = client
            .add(
                NewProfileRequest {
                    metadata: item.metadata.clone(),
                    definition: item.definition,
                },
                Some("-- lua".to_string()),
            )
            .await
            .unwrap();
        let (uid, _) = report.snapshot.items.first().unwrap();
        assert!(uid.0.starts_with('t'), "transform uid prefixed with t");
    }

    #[tokio::test]
    async fn delete_enforces_reference_protection() {
        let (client, _dir) = seeded_client().await;
        client
            .set_current(Some(ProfileId("cfg1".into())))
            .await
            .unwrap();
        let err = client.delete(ProfileId("cfg1".into())).await.unwrap_err();
        assert!(matches!(err, ProfilesError::ProfileInUse { .. }));

        client
            .set_global_transforms(vec![ProfileId("ovl1".into())])
            .await
            .unwrap();
        let err = client.delete(ProfileId("ovl1".into())).await.unwrap_err();
        assert!(matches!(err, ProfilesError::ProfileInUse { .. }));

        let err = client.delete(ProfileId("ghost".into())).await.unwrap_err();
        assert!(matches!(err, ProfilesError::ProfileNotFound(_)));
    }

    #[tokio::test]
    async fn delete_unreferenced_managed_profile_removes_file() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_remove()
            .withf(|path| path.as_str() == "cfg2.yaml")
            .times(1)
            .returning(|_| Ok(()));
        let (client, _dir) = test_client_with(fs).await;
        client.replace(seeded_profiles()).await.unwrap();
        let report = client
            .delete(ProfileId("cfg2".into()))
            .await
            .expect("delete cfg2");
        assert!(!report.affects_current);
        assert!(
            report
                .snapshot
                .items
                .get(&ProfileId("cfg2".into()))
                .is_none()
        );
    }

    #[tokio::test]
    async fn delete_cleanup_failure_degrades_to_warning() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_remove()
            .returning(|_| anyhow::bail!("disk on fire"));
        let (client, _dir) = test_client_with(fs).await;
        client.replace(seeded_profiles()).await.unwrap();
        let report = client
            .delete(ProfileId("cfg2".into()))
            .await
            .expect("delete commits anyway");
        assert_eq!(report.warnings.len(), 1);
        assert!(
            report
                .snapshot
                .items
                .get(&ProfileId("cfg2".into()))
                .is_none()
        );
    }
}
