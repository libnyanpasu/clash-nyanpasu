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
    ProfilesError, RefreshOrigin, ReorderOp,
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

    /// Atomically activate `uid` only if nothing is currently selected.
    /// Returns `Some(report)` when it activated, `None` when a current already
    /// existed (so the caller's activation was intentionally skipped).
    pub async fn set_current_if_none(
        &self,
        uid: ProfileId,
    ) -> Result<Option<CommitReport>, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SetCurrentIfNone { uid, reply },
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

    pub async fn set_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SetValidFields { fields, reply },
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
                origin: RefreshOrigin::Manual,
                reply: Some(reply),
            },
            None,
        )
        .await
    }

    pub(crate) async fn refresh_import(
        &self,
        uid: ProfileId,
        update_interval_explicit: bool,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::RefreshRemote {
                uid,
                patch: None,
                origin: RefreshOrigin::Import {
                    update_interval_explicit,
                },
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
        url: url::Url,
        outcome: crate::state::profiles::RefreshOutcome,
    ) {
        let _ = self
            .inner
            .actor_ref
            .cast(ProfilesActorMessage::CommitRefreshed { uid, url, outcome });
        tokio::task::yield_now().await;
    }

    #[cfg(test)]
    async fn debug_cast_external_changed(&self, uid: ProfileId) {
        let _ = self
            .inner
            .actor_ref
            .cast(ProfilesActorMessage::ExternalFileChanged { uid });
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
    use crate::{
        service::profile_file::{ProfileFileService, SelfProxyPortSource},
        state::profiles::ports::{
            FetchedSubscription, MockProfileFsPort, MockRebuildNotifier, MockSubscriptionFetcher,
        },
        utils::path::PathResolver,
    };
    use nyanpasu_config::profile::{
        ConfigDefinition, ExternalMode, ExternalProfilePath, FileConfig, LocalBinding,
        ManagedProfilePath, MaterializedFile, OverlayTransform, ProfileDefinition, ProfileId,
        ProfileMetadata, ProfileSource, Profiles, RemoteProfileOptions, ScriptRuntime,
        ScriptTransform, SubscriptionInfo, TransformDefinition,
    };
    use struct_patch::Patch as _;
    use tempfile::{TempDir, tempdir};

    struct NoProxyPort;

    impl SelfProxyPortSource for NoProxyPort {
        fn mixed_port(&self) -> Option<u16> {
            None
        }
    }

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

    fn external_item(
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

    fn external_path(path: &std::path::Path) -> ExternalProfilePath {
        ExternalProfilePath::new(path.to_string_lossy().into_owned()).unwrap()
    }

    async fn wait_for_updated_at(client: &ProfilesClient, uid: &str) -> Arc<Profiles> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let snapshot = client.get().await.unwrap();
            let updated = snapshot
                .items
                .get(&ProfileId(uid.into()))
                .and_then(|item| item.definition.source())
                .and_then(|source| source.materialized().updated_at);
            if updated.is_some() {
                return snapshot;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "profile {uid} was not updated before timeout"
            );
            tokio::task::yield_now().await;
        }
    }

    fn ok_fetch(content: &'static str) -> MockSubscriptionFetcher {
        suggested_fetch(content, None)
    }

    fn suggested_fetch(
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
                content: "proxies: []\n".into(),
                filename: None,
                subscription: SubscriptionInfo::default(),
                suggested_update_interval_minutes: None,
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

    fn ok_fetch_named(content: &'static str, filename: &'static str) -> MockSubscriptionFetcher {
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().returning(move |_, _| {
            Ok(FetchedSubscription {
                content: content.to_string(),
                filename: Some(filename.to_string()),
                subscription: SubscriptionInfo::default(),
                suggested_update_interval_minutes: None,
            })
        });
        fetcher
    }

    /// Seed a single remote profile (`r1`, name `R1`) with an explicit
    /// `custom_name`, backed by a fs that accepts refresh writes.
    async fn remote_client_with_custom_name(
        custom_name: bool,
        fetcher: MockSubscriptionFetcher,
    ) -> (ProfilesClient, TempDir) {
        let mut fs = MockProfileFsPort::new();
        fs.expect_ensure_not_symlink().returning(|_| Ok(()));
        fs.expect_write_atomic().returning(|_, _| Ok(()));
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            std::sync::Arc::new(fetcher),
            std::sync::Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .unwrap();
        let mut item = remote_config_item("r1");
        item.metadata.custom_name = custom_name;
        let mut profiles = Profiles::default();
        profiles.append_item(item);
        client.replace(profiles).await.unwrap();
        (client, dir)
    }

    fn name_of(report: &CommitReport, uid: &str) -> String {
        report.snapshot.items[&ProfileId(uid.into())]
            .metadata
            .name
            .clone()
    }

    #[tokio::test]
    async fn refresh_syncs_server_name_when_not_user_named() {
        let (client, _dir) =
            remote_client_with_custom_name(false, ok_fetch_named("proxies: []\n", "Server Name"))
                .await;
        let report = client
            .refresh(ProfileId("r1".into()), None)
            .await
            .expect("refresh ok");
        assert_eq!(name_of(&report, "r1"), "Server Name");
    }

    #[tokio::test]
    async fn refresh_keeps_name_when_user_named() {
        let (client, _dir) =
            remote_client_with_custom_name(true, ok_fetch_named("proxies: []\n", "Server Name"))
                .await;
        let report = client
            .refresh(ProfileId("r1".into()), None)
            .await
            .expect("refresh ok");
        assert_eq!(
            name_of(&report, "r1"),
            "R1",
            "a user-named profile is pinned"
        );
    }

    #[tokio::test]
    async fn rename_pins_custom_name_and_survives_refresh() {
        let (client, _dir) =
            remote_client_with_custom_name(false, ok_fetch_named("proxies: []\n", "Server Name"))
                .await;

        let mut patch = ProfileMetadata::new_empty_patch();
        patch.name = Some("My Name".into());
        let report = client
            .patch_metadata(ProfileId("r1".into()), patch)
            .await
            .expect("rename ok");
        let item = &report.snapshot.items[&ProfileId("r1".into())];
        assert_eq!(item.metadata.name, "My Name");
        assert!(item.metadata.custom_name, "rename must pin the flag");

        // The server still advertises "Server Name", but the pin must win.
        let report = client
            .refresh(ProfileId("r1".into()), None)
            .await
            .expect("refresh ok");
        assert_eq!(name_of(&report, "r1"), "My Name");
    }

    #[tokio::test]
    async fn manual_and_explicit_import_refresh_ignore_server_interval_suggestions() {
        for import_explicit in [None, Some(true)] {
            let mut fs = MockProfileFsPort::new();
            fs.expect_ensure_not_symlink().returning(|_| Ok(()));
            fs.expect_write_atomic().returning(|_, _| Ok(()));
            let (client, _dir) = remote_seeded_client(
                fs,
                suggested_fetch("proxies: []\n", Some(360)),
                MockRebuildNotifier::new(),
            )
            .await;

            let report = if let Some(update_interval_explicit) = import_explicit {
                client
                    .refresh_import(ProfileId("r1".into()), update_interval_explicit)
                    .await
                    .unwrap()
            } else {
                client.refresh(ProfileId("r1".into()), None).await.unwrap()
            };
            let source = report.snapshot.items[&ProfileId("r1".into())]
                .definition
                .source()
                .unwrap();
            let ProfileSource::Remote { option, .. } = source else {
                unreachable!()
            };
            assert_eq!(option.update_interval_minutes, 120);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn import_suggestion_is_committed_and_reschedules_the_timer() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_ensure_not_symlink().returning(|_| Ok(()));
        fs.expect_write_atomic().returning(|_, _| Ok(()));
        let fetch_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut fetcher = MockSubscriptionFetcher::new();
        let counter = std::sync::Arc::clone(&fetch_count);
        fetcher.expect_fetch().returning(move |_, _| {
            let call = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            Ok(FetchedSubscription {
                content: "proxies: []\n".into(),
                filename: None,
                subscription: SubscriptionInfo {
                    upload: Some(call as u64),
                    ..Default::default()
                },
                suggested_update_interval_minutes: Some(if call == 1 { 60 } else { 30 }),
            })
        });
        let (client, _dir) = remote_seeded_client(fs, fetcher, MockRebuildNotifier::new()).await;

        let report = client
            .refresh_import(ProfileId("r1".into()), false)
            .await
            .unwrap();
        let source = report.snapshot.items[&ProfileId("r1".into())]
            .definition
            .source()
            .unwrap();
        let ProfileSource::Remote { option, .. } = source else {
            unreachable!()
        };
        assert_eq!(option.update_interval_minutes, 60);
        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 1);

        tokio::time::advance(std::time::Duration::from_secs(60 * 60 + 1)).await;
        for _ in 0..200 {
            let snapshot = client.get().await.unwrap();
            let source = snapshot.items[&ProfileId("r1".into())]
                .definition
                .source()
                .unwrap();
            if matches!(
                source,
                ProfileSource::Remote { subscription, .. }
                    if subscription.upload == Some(2)
            ) {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(fetch_count.load(std::sync::atomic::Ordering::SeqCst) >= 2);
        let snapshot = client.get().await.unwrap();
        let source = snapshot.items[&ProfileId("r1".into())]
            .definition
            .source()
            .unwrap();
        let ProfileSource::Remote {
            option,
            subscription,
            ..
        } = source
        else {
            unreachable!()
        };
        assert_eq!(subscription.upload, Some(2));
        assert_eq!(
            option.update_interval_minutes, 60,
            "scheduled refresh must ignore later server suggestions"
        );
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
        let ProfileSource::Remote { option, .. } = source else {
            unreachable!()
        };
        assert_eq!(option.update_interval_minutes, 120);
    }

    #[tokio::test]
    async fn import_suggestion_is_not_committed_when_materialization_fails() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_ensure_not_symlink().returning(|_| Ok(()));
        fs.expect_write_atomic()
            .returning(|_, _| Err(anyhow::anyhow!("disk full")));
        let (client, _dir) = remote_seeded_client(
            fs,
            suggested_fetch("proxies: []\n", Some(360)),
            MockRebuildNotifier::new(),
        )
        .await;

        assert!(
            client
                .refresh_import(ProfileId("r1".into()), false)
                .await
                .is_err()
        );
        let snapshot = client.get().await.unwrap();
        let ProfileSource::Remote {
            materialized,
            option,
            ..
        } = snapshot.items[&ProfileId("r1".into())]
            .definition
            .source()
            .unwrap()
        else {
            unreachable!()
        };
        assert!(materialized.updated_at.is_none());
        assert_eq!(option.update_interval_minutes, 120);
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
    async fn refresh_commit_refreshed_deleted_profile_settles_reply_without_writes() {
        let removals = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let mut fs = MockProfileFsPort::new();
        let observed = std::sync::Arc::clone(&removals);
        fs.expect_remove().returning(move |path| {
            observed.lock().unwrap().push(path.as_str().to_string());
            Ok(())
        });
        // No write_atomic expectation: with the write moved into the commit
        // phase, a refresh settling after delete must not touch the fs.
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
                url::Url::parse("https://example.com/sub").unwrap(),
                crate::state::profiles::RefreshOutcome::Succeeded {
                    subscription: SubscriptionInfo::default(),
                    suggested_update_interval_minutes: None,
                    content: "proxies: []\n".into(),
                    filename: None,
                },
            )
            .await;

        let err = pending.await.unwrap().unwrap_err();
        assert!(
            matches!(err, ProfilesError::RefreshFailed { message } if message.contains("deleted"))
        );
        // Only the delete's own post-op removal; no orphan-cleanup sweep.
        let removals = removals.lock().unwrap();
        assert_eq!(removals.iter().filter(|path| *path == "r1.yaml").count(), 1);
        assert!(!removals.iter().any(|path| path == "r1.js"));
        assert!(!removals.iter().any(|path| path == "r1.lua"));
    }

    /// Review fix regression pin (2026-07-11): a download committed after the
    /// definition was replaced with a different URL must be discarded — no
    /// file write, no metadata update.
    #[tokio::test]
    async fn refresh_commit_is_fenced_when_url_changed_mid_download() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_remove().returning(|_| Ok(()));
        // No write_atomic expectation: the stale commit must never write.
        let (client, _dir) = remote_seeded_client(
            fs,
            MockSubscriptionFetcher::new(),
            MockRebuildNotifier::new(),
        )
        .await;

        let (inserted, pending) = client.debug_pending_refresh(ProfileId("r1".into()));
        inserted.await.unwrap();
        client
            .debug_cast_commit_refreshed(
                ProfileId("r1".into()),
                url::Url::parse("https://old.example.com/replaced").unwrap(),
                crate::state::profiles::RefreshOutcome::Succeeded {
                    subscription: SubscriptionInfo::default(),
                    suggested_update_interval_minutes: None,
                    content: "proxies: []\n".into(),
                    filename: None,
                },
            )
            .await;

        let err = pending.await.unwrap().unwrap_err();
        assert!(
            matches!(err, ProfilesError::RefreshFailed { message } if message.contains("changed"))
        );
        let snapshot = client.get().await.unwrap();
        let item = snapshot.items.get(&ProfileId("r1".into())).unwrap();
        let Some(nyanpasu_config::profile::ProfileSource::Remote { materialized, .. }) =
            item.definition.source()
        else {
            panic!("seeded profile must stay remote");
        };
        assert!(
            materialized.updated_at.is_none(),
            "stale download must not stamp updated_at"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_fires_refresh_on_interval() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_ensure_not_symlink().returning(|_| Ok(()));
        fs.expect_write_atomic().returning(|_, _| Ok(()));
        let fetch_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut fetcher = MockSubscriptionFetcher::new();
        let counter = std::sync::Arc::clone(&fetch_count);
        fetcher.expect_fetch().returning(move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(FetchedSubscription {
                content: "proxies: []\n".into(),
                filename: None,
                subscription: SubscriptionInfo::default(),
                suggested_update_interval_minutes: None,
            })
        });
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            std::sync::Arc::new(fetcher),
            std::sync::Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .unwrap();
        let mut item = remote_config_item("r1");
        if let Some(source) = item.definition.source_mut() {
            source.materialized_mut().updated_at = Some(time::OffsetDateTime::now_utc());
        }
        let mut profiles = Profiles::default();
        profiles.append_item(item);
        client.replace(profiles).await.unwrap();

        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 0);
        tokio::time::advance(std::time::Duration::from_secs(120 * 60 + 1)).await;
        for _ in 0..200 {
            if fetch_count.load(std::sync::atomic::Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(fetch_count.load(std::sync::atomic::Ordering::SeqCst) >= 1);
        drop(client);
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_reconcile_add_remove_and_kind_switch() {
        let fetch_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut fetcher = MockSubscriptionFetcher::new();
        let counter = std::sync::Arc::clone(&fetch_count);
        fetcher.expect_fetch().returning(move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            anyhow::bail!("count only")
        });
        let mut fs = MockProfileFsPort::new();
        fs.expect_remove().returning(|_| Ok(()));
        let (client, _dir) = remote_seeded_client(fs, fetcher, MockRebuildNotifier::new()).await;

        client
            .replace_definition(ProfileId("r1".into()), file_config_item("r1").definition)
            .await
            .unwrap();
        tokio::time::advance(std::time::Duration::from_secs(240 * 60)).await;
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 0);

        let mut profiles = Profiles::default();
        profiles.append_item(remote_config_item("r1"));
        client.replace(profiles).await.unwrap();
        client.delete(ProfileId("r1".into())).await.unwrap();
        tokio::time::advance(std::time::Duration::from_secs(240 * 60)).await;
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_catches_up_overdue_profiles_on_start() {
        let fetch_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut fetcher = MockSubscriptionFetcher::new();
        let counter = std::sync::Arc::clone(&fetch_count);
        fetcher.expect_fetch().returning(move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            anyhow::bail!("count only")
        });
        let dir = tempdir().unwrap();
        {
            let client = ProfilesClient::new(
                temp_profiles_path(&dir),
                std::sync::Arc::new(MockProfileFsPort::new()),
                std::sync::Arc::new(MockSubscriptionFetcher::new()),
                std::sync::Arc::new(MockRebuildNotifier::new()),
            )
            .await
            .unwrap();
            let mut item = remote_config_item("r1");
            if let Some(source) = item.definition.source_mut() {
                source.materialized_mut().updated_at =
                    Some(time::OffsetDateTime::now_utc() - time::Duration::days(30));
            }
            let mut profiles = Profiles::default();
            profiles.append_item(item);
            client.replace(profiles).await.unwrap();
        }
        let _client = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(MockProfileFsPort::new()),
            std::sync::Arc::new(fetcher),
            std::sync::Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .unwrap();
        for _ in 0..200 {
            if fetch_count.load(std::sync::atomic::Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn external_mirror_change_syncs_copy_and_bumps_updated_at() {
        let target_dir = tempdir().unwrap();
        let target_path = target_dir.path().join("external.yaml");
        std::fs::write(&target_path, "proxies: []\n").unwrap();
        let target = external_path(&target_path);

        let mut fs = MockProfileFsPort::new();
        fs.expect_read_external()
            .withf({
                let target = target.clone();
                move |observed| observed == &target
            })
            .times(1)
            .returning(|_| Ok("proxies: []\n".into()));
        fs.expect_write_atomic()
            .withf(|path, content| path.as_str() == "ext1.yaml" && content == "proxies: []\n")
            .times(1)
            .returning(|_, _| Ok(()));
        let (client, _dir) = test_client_with(fs).await;
        let mut profiles = Profiles::default();
        profiles.append_item(external_item("ext1", ExternalMode::Mirror, target));
        client.replace(profiles).await.unwrap();

        client
            .debug_cast_external_changed(ProfileId("ext1".into()))
            .await;

        let snapshot = wait_for_updated_at(&client, "ext1").await;
        assert!(
            snapshot.items[&ProfileId("ext1".into())]
                .definition
                .source()
                .unwrap()
                .materialized()
                .updated_at
                .is_some()
        );
    }

    #[tokio::test]
    async fn external_symlink_change_bumps_updated_at_without_copy() {
        let target_dir = tempdir().unwrap();
        let target_path = target_dir.path().join("external.yaml");
        std::fs::write(&target_path, "proxies: []\n").unwrap();
        let target = external_path(&target_path);

        let (client, _dir) = test_client_with(MockProfileFsPort::new()).await;
        let mut profiles = Profiles::default();
        profiles.append_item(external_item("ext1", ExternalMode::Symlink, target));
        client.replace(profiles).await.unwrap();

        client
            .debug_cast_external_changed(ProfileId("ext1".into()))
            .await;

        wait_for_updated_at(&client, "ext1").await;
    }

    #[tokio::test]
    async fn external_background_commit_requests_rebuild_once_for_current() {
        let target_dir = tempdir().unwrap();
        let target_path = target_dir.path().join("external.yaml");
        std::fs::write(&target_path, "proxies: []\n").unwrap();
        let target = external_path(&target_path);

        let mut fs = MockProfileFsPort::new();
        fs.expect_read_external()
            .times(1)
            .returning(|_| Ok("proxies: []\n".into()));
        fs.expect_write_atomic().times(1).returning(|_, _| Ok(()));
        let mut notifier = MockRebuildNotifier::new();
        notifier.expect_request_rebuild().times(1).returning(|| ());
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            std::sync::Arc::new(notifier),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(external_item("ext1", ExternalMode::Mirror, target));
        profiles.set_current(Some(ProfileId("ext1".into())));
        client.replace(profiles).await.unwrap();

        client
            .debug_cast_external_changed(ProfileId("ext1".into()))
            .await;

        wait_for_updated_at(&client, "ext1").await;
    }

    #[tokio::test]
    #[ignore = "relies on real OS file events; flaky on loaded CI runners - run manually"]
    async fn external_watcher_smoke_mirror_real_file_event() {
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        let target_path = target_dir.path().join("external.yaml");
        std::fs::write(&target_path, "proxies: []\n").unwrap();
        let paths = PathResolver::with_base_dirs(
            config_dir.path().to_path_buf(),
            data_dir.path().to_path_buf(),
        );
        let fs = std::sync::Arc::new(ProfileFileService::new(
            paths,
            std::sync::Arc::new(NoProxyPort),
        ));
        let profiles_path =
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap();
        let client = ProfilesClient::new(
            profiles_path,
            fs,
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            std::sync::Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(external_item(
            "ext1",
            ExternalMode::Mirror,
            external_path(&target_path),
        ));
        client.replace(profiles).await.unwrap();

        tokio::fs::write(&target_path, "mode: rule\nproxies: []\n")
            .await
            .unwrap();

        let managed_path = config_dir.path().join("profiles").join("ext1.yaml");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let updated = client
                    .get()
                    .await
                    .unwrap()
                    .items
                    .get(&ProfileId("ext1".into()))
                    .and_then(|item| item.definition.source())
                    .and_then(|source| source.materialized().updated_at)
                    .is_some();
                let mirrored = std::fs::read_to_string(&managed_path)
                    .is_ok_and(|content| content.contains("mode: rule"));
                if updated && mirrored {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("real external watcher event should commit within 5s");
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
    async fn set_current_if_none_only_activates_when_empty() {
        let (client, _dir) = seeded_client().await;

        // Empty current -> activates and reports the change.
        let report = client
            .set_current_if_none(ProfileId("cfg1".into()))
            .await
            .expect("call ok")
            .expect("activated when current was empty");
        assert!(report.affects_current);
        assert_eq!(report.snapshot.current, Some(ProfileId("cfg1".into())));

        // A current already exists -> does NOT activate (returns None) and
        // leaves the existing selection untouched.
        let skipped = client
            .set_current_if_none(ProfileId("cfg2".into()))
            .await
            .expect("call ok");
        assert!(skipped.is_none(), "must not overwrite an existing current");
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
    async fn set_valid_fields_commits_and_always_affects_current() {
        let (client, _dir) = seeded_client().await;
        let report = client
            .set_valid_fields(vec!["dns".into(), "tun".into()])
            .await
            .expect("set valid fields");
        assert!(
            report.affects_current,
            "whitelist change must trigger rebuild"
        );
        assert_eq!(
            report.snapshot.valid,
            vec!["dns".to_string(), "tun".to_string()]
        );
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
                        custom_name: true,
                    },
                    definition: file_config_item("placeholder").definition,
                },
                Some("proxies: []\n".to_string()),
            )
            .await
            .expect("add should succeed");

        assert!(!report.affects_current);
        assert!(report.warnings.is_empty());
        let created = report
            .created
            .clone()
            .expect("add must report the server-generated uid");
        assert!(report.snapshot.items.contains_key(&created));
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
        assert!(matches!(err, ProfilesError::InvalidReorderList { .. }));

        let err = client
            .reorder(ReorderOp::ByList(vec![
                ProfileId("cfg1".into()),
                ProfileId("cfg1".into()),
                ProfileId("ovl1".into()),
            ]))
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::InvalidReorderList { .. }));
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
            ProfilesError::ProfileInUse {
                referrers,
                current,
                global_transforms,
            } => {
                assert_eq!(referrers, vec![ProfileId("cfg1".into())]);
                assert!(!current);
                assert!(!global_transforms);
            }
            other => panic!("expected ProfileInUse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn validation_failure_leaves_disk_untouched() {
        let (client, dir) = seeded_client().await;
        let err = client
            .set_current(Some(ProfileId("ghost".into())))
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed(_)));
        drop(client);

        let reopened = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(MockProfileFsPort::new()),
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            std::sync::Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .expect("reopen after failed mutation");
        let snapshot = reopened.get().await.unwrap();
        assert!(snapshot.current.is_none());
        assert_eq!(snapshot.items.len(), 3);
    }

    #[tokio::test]
    async fn delete_protects_composition_base_and_contributors() {
        let (client, _dir) = test_client_with(MockProfileFsPort::new()).await;
        let mut profiles = seeded_profiles();
        profiles.append_item(nyanpasu_config::profile::ProfileItem {
            uid: ProfileId("comp".into()),
            metadata: ProfileMetadata {
                name: "COMP".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::Composition(
                    nyanpasu_config::profile::CompositionConfig {
                        base: Some(ProfileId("cfg1".into())),
                        extend_proxies_from: vec![ProfileId("cfg2".into())],
                        transforms: vec![],
                    },
                ),
            },
        });
        client.replace(profiles).await.unwrap();

        let err = client.delete(ProfileId("cfg1".into())).await.unwrap_err();
        match err {
            ProfilesError::ProfileInUse {
                referrers,
                current,
                global_transforms,
            } => {
                assert_eq!(referrers, vec![ProfileId("comp".into())]);
                assert!(!current);
                assert!(!global_transforms);
            }
            other => panic!("expected ProfileInUse, got {other:?}"),
        }
        let err = client.delete(ProfileId("cfg2".into())).await.unwrap_err();
        match err {
            ProfilesError::ProfileInUse { referrers, .. } => {
                assert_eq!(referrers, vec![ProfileId("comp".into())]);
            }
            other => panic!("expected ProfileInUse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn delete_reports_document_level_references() {
        let (client, _dir) = seeded_client().await;
        client
            .set_current(Some(ProfileId("cfg1".into())))
            .await
            .unwrap();
        let err = client.delete(ProfileId("cfg1".into())).await.unwrap_err();
        match err {
            ProfilesError::ProfileInUse {
                referrers,
                current,
                global_transforms,
            } => {
                assert!(referrers.is_empty());
                assert!(current);
                assert!(!global_transforms);
            }
            other => panic!("expected ProfileInUse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn delete_cleanup_per_binding_kind() {
        let outside_a = if cfg!(windows) {
            "C:\\outside\\a.yaml"
        } else {
            "/outside/a.yaml"
        };
        let outside_b = if cfg!(windows) {
            "C:\\outside\\b.yaml"
        } else {
            "/outside/b.yaml"
        };

        // External Symlink: only the app-managed link is removed.
        let mut fs = MockProfileFsPort::new();
        fs.expect_remove()
            .withf(|path| path.as_str() == "sym1.yaml")
            .times(1)
            .returning(|_| Ok(()));
        let (client, _dir) = test_client_with(fs).await;
        let mut profiles = Profiles::default();
        profiles.append_item(external_item(
            "sym1",
            ExternalMode::Symlink,
            external_path(std::path::Path::new(outside_a)),
        ));
        client.replace(profiles).await.unwrap();
        client.delete(ProfileId("sym1".into())).await.unwrap();

        // External Mirror: the mirror copy is removed.
        let mut fs = MockProfileFsPort::new();
        fs.expect_remove()
            .withf(|path| path.as_str() == "mir1.yaml")
            .times(1)
            .returning(|_| Ok(()));
        let (client, _dir) = test_client_with(fs).await;
        let mut profiles = Profiles::default();
        profiles.append_item(external_item(
            "mir1",
            ExternalMode::Mirror,
            external_path(std::path::Path::new(outside_b)),
        ));
        client.replace(profiles).await.unwrap();
        client.delete(ProfileId("mir1".into())).await.unwrap();

        // Composition: no filesystem call at all (mock without expectations
        // panics on any use).
        let (client, _dir) = test_client_with(MockProfileFsPort::new()).await;
        let mut profiles = seeded_profiles();
        profiles.append_item(nyanpasu_config::profile::ProfileItem {
            uid: ProfileId("comp".into()),
            metadata: ProfileMetadata {
                name: "COMP".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::Composition(
                    nyanpasu_config::profile::CompositionConfig {
                        base: Some(ProfileId("cfg1".into())),
                        extend_proxies_from: vec![],
                        transforms: vec![],
                    },
                ),
            },
        });
        client.replace(profiles).await.unwrap();
        client.delete(ProfileId("comp".into())).await.unwrap();
    }

    #[tokio::test]
    async fn load_fails_fast_on_invalid_document() {
        let dir = tempdir().unwrap();
        let path = temp_profiles_path(&dir);
        std::fs::write(
            &path,
            "current: ghost\nitems:\n- uid: a\n  name: A\n  type: config\n  config:\n    type: file\n    source:\n      type: local\n      binding:\n        type: managed\n        file: a.yaml\n",
        )
        .unwrap();
        let result = ProfilesClient::new(
            path,
            std::sync::Arc::new(MockProfileFsPort::new()),
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            std::sync::Arc::new(MockRebuildNotifier::new()),
        )
        .await;
        assert!(result.is_err(), "invalid persisted document must fail fast");
    }

    #[tokio::test]
    async fn add_remote_resets_materialization_metadata() {
        let (client, _dir) = test_client_with(MockProfileFsPort::new()).await;
        let mut item = remote_config_item("placeholder");
        if let Some(source) = item.definition.source_mut() {
            source.materialized_mut().updated_at = Some(time::OffsetDateTime::UNIX_EPOCH);
            if let ProfileSource::Remote { subscription, .. } = source {
                subscription.upload = Some(1);
            }
        }
        let report = client
            .add(
                NewProfileRequest {
                    metadata: item.metadata.clone(),
                    definition: item.definition,
                },
                None,
            )
            .await
            .unwrap();
        let (uid, added) = report.snapshot.items.first().unwrap();
        let source = added.definition.source().unwrap();
        assert_eq!(source.materialized().file.as_str(), format!("{uid}.yaml"));
        assert!(source.materialized().updated_at.is_none());
        match source {
            ProfileSource::Remote { subscription, .. } => assert!(subscription.is_empty()),
            _ => panic!("expected remote source"),
        }
    }

    #[tokio::test]
    async fn replace_definition_rejects_invalid_and_keeps_state() {
        let (client, dir) = seeded_client().await;
        let mut definition = seeded_profiles().items[&ProfileId("cfg1".into())]
            .definition
            .clone();
        if let ProfileDefinition::Config {
            config: ConfigDefinition::File(file),
        } = &mut definition
        {
            file.transforms = vec![ProfileId("ghost".into())];
        }
        let err = client
            .replace_definition(ProfileId("cfg1".into()), definition)
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed(_)));
        drop(client);

        let reopened = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(MockProfileFsPort::new()),
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            std::sync::Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .unwrap();
        let snapshot = reopened.get().await.unwrap();
        let item = &snapshot.items[&ProfileId("cfg1".into())];
        match &item.definition {
            ProfileDefinition::Config { config } => assert!(config.transforms().is_empty()),
            other => panic!("cfg1 must stay a config, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn replace_definition_kind_switch_rewrites_path_and_cleans_orphan() {
        let mut fs = MockProfileFsPort::new();
        fs.expect_remove()
            .withf(|path| path.as_str() == "cfg2.yaml")
            .times(1)
            .returning(|_| Ok(()));
        let (client, _dir) = test_client_with(fs).await;
        client.replace(seeded_profiles()).await.unwrap();

        let definition = ProfileDefinition::Transform {
            transform: TransformDefinition::Script(nyanpasu_config::profile::ScriptTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new("client-supplied.lua").unwrap(),
                            updated_at: Some(time::OffsetDateTime::UNIX_EPOCH),
                        },
                    },
                },
                runtime: nyanpasu_config::profile::ScriptRuntime::Lua,
            }),
        };
        let report = client
            .replace_definition(ProfileId("cfg2".into()), definition)
            .await
            .unwrap();
        let item = &report.snapshot.items[&ProfileId("cfg2".into())];
        let source = item.definition.source().unwrap();
        assert_eq!(source.materialized().file.as_str(), "cfg2.lua");
        assert!(source.materialized().updated_at.is_none());
    }
}
