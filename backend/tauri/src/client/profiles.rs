//! Typed client for the ProfilesActor. Read Some(5s) / write None.

use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::profile::{
    ProfileDefinition, ProfileId, ProfileMetadataPatch, Profiles, RemoteProfileOptionsPatch,
};
use nyanpasu_core::state::PersistentStateManagerSetup;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};

use crate::state::profiles::{
    CommitReport, NewProfileRequest, ProfilesActor, ProfilesActorArgs, ProfilesActorMessage,
    ProfilesError, ReorderOp,
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

    pub async fn reorder(&self, op: ReorderOp) -> Result<CommitReport, ProfilesError> {
        self.call(|reply| ProfilesActorMessage::Reorder { op, reply }, None)
            .await
    }

    pub async fn patch_metadata(
        &self,
        uid: ProfileId,
        patch: ProfileMetadataPatch,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::PatchMetadata { uid, patch, reply },
            None,
        )
        .await
    }

    pub async fn patch_remote_options(
        &self,
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::PatchRemoteOptions { uid, patch, reply },
            None,
        )
        .await
    }

    pub async fn refresh(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::RefreshRemote {
                uid,
                patch,
                reply: Some(reply),
            },
            None,
        )
        .await
    }

    pub async fn replace_definition(
        &self,
        uid: ProfileId,
        definition: ProfileDefinition,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::ReplaceDefinition {
                uid,
                definition,
                reply,
            },
            None,
        )
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

    #[cfg(test)]
    fn debug_pending_refresh(
        &self,
        uid: ProfileId,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::task::JoinHandle<Result<CommitReport, ProfilesError>>,
    ) {
        let (inserted, inserted_rx) = tokio::sync::oneshot::channel();
        let client = self.clone();
        let handle = tokio::spawn(async move {
            client
                .call(
                    |reply| ProfilesActorMessage::DebugInsertPendingRefresh {
                        uid,
                        reply,
                        inserted,
                    },
                    None,
                )
                .await
        });
        (inserted_rx, handle)
    }

    #[cfg(test)]
    async fn debug_cast_commit_refreshed(
        &self,
        uid: ProfileId,
        outcome: crate::state::profiles::RefreshOutcome,
    ) {
        let _ = self
            .inner
            .actor_ref
            .cast(ProfilesActorMessage::CommitRefreshed { uid, outcome });
        tokio::task::yield_now().await;
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
        FetchedSubscription, MockProfileFsPort, MockRebuildNotifier, MockSubscriptionFetcher,
    };
    use nyanpasu_config::profile::{
        ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
        OverlayTransform, ProfileDefinition, ProfileId, ProfileMetadata, ProfileSource, Profiles,
        RemoteProfileOptions, ScriptRuntime, ScriptTransform, SubscriptionInfo,
        TransformDefinition,
    };
    use struct_patch::Patch as _;
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

    pub(crate) fn remote_config_item(uid: &str) -> nyanpasu_config::profile::ProfileItem {
        nyanpasu_config::profile::ProfileItem {
            uid: ProfileId(uid.into()),
            metadata: ProfileMetadata {
                name: uid.to_uppercase(),
                desc: None,
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

    fn ok_fetch(content: &'static str) -> MockSubscriptionFetcher {
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().returning(move |_, _| {
            Ok(FetchedSubscription {
                content: content.to_string(),
                filename: None,
                subscription: SubscriptionInfo {
                    upload: Some(1),
                    ..Default::default()
                },
            })
        });
        fetcher
    }

    async fn remote_seeded_client(
        fs: MockProfileFsPort,
        fetcher: MockSubscriptionFetcher,
        notifier: MockRebuildNotifier,
    ) -> (ProfilesClient, TempDir) {
        remote_seeded_client_with_fetcher(fs, std::sync::Arc::new(fetcher), notifier).await
    }

    async fn remote_seeded_client_with_fetcher(
        fs: MockProfileFsPort,
        fetcher: std::sync::Arc<dyn SubscriptionFetcher>,
        notifier: MockRebuildNotifier,
    ) -> (ProfilesClient, TempDir) {
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            fetcher,
            std::sync::Arc::new(notifier),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(remote_config_item("r1"));
        client.replace(profiles).await.unwrap();
        (client, dir)
    }

    struct HoldingFetcher {
        started: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        release: std::sync::Arc<tokio::sync::Notify>,
    }

    #[async_trait::async_trait]
    impl SubscriptionFetcher for HoldingFetcher {
        async fn fetch(
            &self,
            _url: &url::Url,
            _options: &RemoteProfileOptions,
        ) -> anyhow::Result<FetchedSubscription> {
            if let Some(started) = self.started.lock().unwrap().take() {
                let _ = started.send(());
            }
            self.release.notified().await;
            Ok(FetchedSubscription {
                content: "a: 1\n".into(),
                filename: None,
                subscription: SubscriptionInfo::default(),
            })
        }
    }

    #[tokio::test]
    async fn refresh_downloads_writes_and_commits_subscription() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_ensure_not_symlink().returning(|_| Ok(()));
        fs.expect_write_atomic()
            .withf(|path, content| path.as_str() == "r1.yaml" && content == "proxies: []\n")
            .times(1)
            .returning(|_, _| Ok(()));
        let (client, _dir) =
            remote_seeded_client(fs, ok_fetch("proxies: []\n"), MockRebuildNotifier::new()).await;

        let report = client
            .refresh(ProfileId("r1".into()), None)
            .await
            .expect("refresh ok");
        let item = &report.snapshot.items[&ProfileId("r1".into())];
        let source = item.definition.source().unwrap();
        assert!(source.materialized().updated_at.is_some());
        match source {
            ProfileSource::Remote { subscription, .. } => {
                assert_eq!(subscription.upload, Some(1));
            }
            _ => unreachable!(),
        }
        assert!(!report.affects_current);
    }

    #[tokio::test]
    async fn refresh_failure_settles_reply_with_error() {
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher
            .expect_fetch()
            .returning(|_, _| anyhow::bail!("dns exploded"));
        let (client, _dir) = remote_seeded_client(
            MockProfileFsPort::new(),
            fetcher,
            MockRebuildNotifier::new(),
        )
        .await;
        let err = client
            .refresh(ProfileId("r1".into()), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::RefreshFailed { .. }));
        let snapshot = client.get().await.unwrap();
        let source = snapshot.items[&ProfileId("r1".into())]
            .definition
            .source()
            .unwrap();
        assert!(source.materialized().updated_at.is_none());
    }

    #[tokio::test]
    async fn refresh_rejects_non_remote_and_unknown_and_concurrent() {
        let (client, _dir) = seeded_client().await;
        let err = client
            .refresh(ProfileId("cfg1".into()), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::NotARemoteProfile));
        let err = client
            .refresh(ProfileId("ghost".into()), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ProfileNotFound(_)));

        let (started, started_rx) = tokio::sync::oneshot::channel();
        let release_fetch = std::sync::Arc::new(tokio::sync::Notify::new());
        let fetcher = HoldingFetcher {
            started: std::sync::Mutex::new(Some(started)),
            release: std::sync::Arc::clone(&release_fetch),
        };
        let mut fs = MockProfileFsPort::new();
        fs.expect_ensure_not_symlink().returning(|_| Ok(()));
        fs.expect_write_atomic().returning(|_, _| Ok(()));
        let (client, _dir) = remote_seeded_client_with_fetcher(
            fs,
            std::sync::Arc::new(fetcher),
            MockRebuildNotifier::new(),
        )
        .await;
        let c2 = client.clone();
        let first = tokio::spawn(async move { c2.refresh(ProfileId("r1".into()), None).await });
        started_rx.await.unwrap();
        let err = loop {
            match client.refresh(ProfileId("r1".into()), None).await {
                Err(ProfilesError::RefreshFailed { message })
                    if message.contains("in progress") =>
                {
                    break ProfilesError::RefreshFailed { message };
                }
                Ok(_) => panic!("second refresh must not both succeed before first settles"),
                Err(_) => tokio::task::yield_now().await,
            }
        };
        assert!(matches!(err, ProfilesError::RefreshFailed { .. }));
        release_fetch.notify_waiters();
        first.await.unwrap().expect("first refresh completes");
    }

    #[tokio::test]
    async fn refresh_commit_refreshed_deleted_profile_settles_reply_and_cleans_orphan() {
        let removals = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let mut fs = MockProfileFsPort::new();
        let observed = std::sync::Arc::clone(&removals);
        fs.expect_remove().returning(move |path| {
            observed.lock().unwrap().push(path.as_str().to_string());
            Ok(())
        });
        let (client, _dir) = remote_seeded_client(
            fs,
            MockSubscriptionFetcher::new(),
            MockRebuildNotifier::new(),
        )
        .await;

        let (inserted, pending) = client.debug_pending_refresh(ProfileId("r1".into()));
        inserted.await.unwrap();
        client.delete(ProfileId("r1".into())).await.unwrap();
        client
            .debug_cast_commit_refreshed(
                ProfileId("r1".into()),
                crate::state::profiles::RefreshOutcome::Succeeded {
                    subscription: SubscriptionInfo::default(),
                },
            )
            .await;

        let err = pending.await.unwrap().unwrap_err();
        assert!(
            matches!(err, ProfilesError::RefreshFailed { message } if message.contains("deleted"))
        );
        let removals = removals.lock().unwrap();
        assert!(removals.iter().filter(|path| *path == "r1.yaml").count() >= 2);
        assert!(removals.iter().any(|path| path == "r1.js"));
        assert!(removals.iter().any(|path| path == "r1.lua"));
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

    #[tokio::test]
    async fn reorder_move_and_by_list() {
        let (client, _dir) = seeded_client().await;
        let report = client
            .reorder(ReorderOp::Move {
                active: ProfileId("cfg2".into()),
                over: ProfileId("cfg1".into()),
            })
            .await
            .unwrap();
        assert!(!report.affects_current);
        let uids: Vec<_> = report.snapshot.items.keys().map(|u| u.0.clone()).collect();
        assert_eq!(uids, vec!["cfg2", "cfg1", "ovl1"]);

        let report = client
            .reorder(ReorderOp::ByList(vec![
                ProfileId("ovl1".into()),
                ProfileId("cfg1".into()),
                ProfileId("cfg2".into()),
            ]))
            .await
            .unwrap();
        let uids: Vec<_> = report.snapshot.items.keys().map(|u| u.0.clone()).collect();
        assert_eq!(uids, vec!["ovl1", "cfg1", "cfg2"]);

        let err = client
            .reorder(ReorderOp::ByList(vec![ProfileId("cfg1".into())]))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ProfilesError::ValidationFailed(_) | ProfilesError::ProfileNotFound(_)
        ));
    }

    #[tokio::test]
    async fn patch_metadata_and_remote_options() {
        let (client, _dir) = seeded_client().await;
        let mut patch = ProfileMetadata::new_empty_patch();
        patch.name = Some("Renamed".into());
        let report = client
            .patch_metadata(ProfileId("cfg1".into()), patch)
            .await
            .unwrap();
        assert!(!report.affects_current);
        assert_eq!(
            report.snapshot.items[&ProfileId("cfg1".into())]
                .metadata
                .name,
            "Renamed"
        );

        let options_patch = nyanpasu_config::profile::RemoteProfileOptions::new_empty_patch();
        let err = client
            .patch_remote_options(ProfileId("cfg1".into()), options_patch)
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::NotARemoteProfile));
    }

    #[tokio::test]
    async fn replace_definition_is_atomic_and_reports_closure_hit() {
        let (client, _dir) = seeded_client().await;
        client
            .set_current(Some(ProfileId("cfg1".into())))
            .await
            .unwrap();

        let mut definition = seeded_profiles().items[&ProfileId("cfg1".into())]
            .definition
            .clone();
        if let ProfileDefinition::Config {
            config: ConfigDefinition::File(file),
        } = &mut definition
        {
            file.transforms = vec![ProfileId("ovl1".into())];
        }
        let report = client
            .replace_definition(ProfileId("cfg1".into()), definition)
            .await
            .unwrap();
        assert!(report.affects_current);

        let definition = seeded_profiles().items[&ProfileId("cfg2".into())]
            .definition
            .clone();
        let report = client
            .replace_definition(ProfileId("cfg2".into()), definition)
            .await
            .unwrap();
        assert!(!report.affects_current);

        let err = client.delete(ProfileId("ovl1".into())).await.unwrap_err();
        match err {
            ProfilesError::ProfileInUse { referrers } => {
                assert_eq!(referrers, vec![ProfileId("cfg1".into())]);
            }
            other => panic!("expected ProfileInUse, got {other:?}"),
        }
    }
}
