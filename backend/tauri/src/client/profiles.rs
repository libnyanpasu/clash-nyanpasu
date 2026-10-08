//! Typed client for the ProfilesActor. Committed reads come from the state
//! snapshot handle; writes go through the actor with no RPC timeout.

use crate::state::mutation::MutationCoordinator;

use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::profile::{
    ProfileDefinition, ProfileId, ProfileMetadata, ProfileMetadataPatch, Profiles,
    RemoteProfileOptions, RemoteProfileOptionsPatch, TransformKind,
};
use nyanpasu_core::state::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    core::migration::modules::profiles::ProfilesFormat,
    state::profiles::{
        CommitReport, NewProfileRequest, ProfilesActor, ProfilesActorArgs, ProfilesActorMessage,
        ProfilesActorStoppedSnafu, ProfilesError, ProfilesReplyDroppedSnafu, ReorderOp,
        ports::{ProfileFsPort, ProfileMaterializationPort, SubscriptionFetcher},
        sources::SourcesSnapshot,
    },
};

#[derive(Clone)]
pub struct ProfilesClient {
    inner: Arc<ProfilesClientInner>,
}

struct ProfilesClientInner {
    actor_ref: ActorRef<ProfilesActorMessage>,
    /// Committed state, read straight from the coordinator's store so a reader
    /// never queues behind a mutation the actor is still holding open.
    snapshot: StateSnapshot<Profiles>,
    sources: tokio::sync::watch::Receiver<SourcesSnapshot>,
    jobs: crate::state::profiles::jobs::ProfileJobs,
}

impl ProfilesClient {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn new_with_jobs(
        mutations: MutationCoordinator,
        profiles_path: Utf8PathBuf,
        jobs: nyanpasu_jobs::JobsClient,
        fs: Arc<dyn ProfileFsPort>,
        fetcher: Arc<dyn SubscriptionFetcher>,
        materialization: Arc<dyn ProfileMaterializationPort>,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        let jobs = crate::state::profiles::jobs::ProfileJobs::new(jobs);
        let should_load = profiles_path.exists();
        let setup = PersistentStateManagerSetup::<Profiles, ProfilesFormat>::builder()
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

        let snapshot = manager.snapshot_handle();
        snapshot
            .load()
            .state
            .validate()
            .map_err(|errors| anyhow::anyhow!("profiles.yaml failed validation: {errors:?}"))?;

        let (sources, sources_rx) = tokio::sync::watch::channel(SourcesSnapshot::default());
        let actor_ref = Actor::spawn(
            None,
            ProfilesActor,
            ProfilesActorArgs {
                mutations,
                manager,
                fs,
                fetcher,
                materialization,
                sources,
                jobs: jobs.clone(),
                shutdown: shutdown.clone(),
            },
        )
        .await
        .context("failed to spawn profiles actor")?
        .0;
        nyanpasu_core::tasks::drain_on_shutdown(tasks, shutdown, actor_ref.get_cell());

        Ok(Self {
            inner: Arc::new(ProfilesClientInner {
                actor_ref,
                snapshot,
                sources: sources_rx,
                jobs,
            }),
        })
    }

    #[cfg(test)]
    pub(crate) async fn new(
        mutations: MutationCoordinator,
        profiles_path: Utf8PathBuf,
        fs: Arc<dyn ProfileFsPort>,
        fetcher: Arc<dyn SubscriptionFetcher>,
        materialization: Arc<dyn ProfileMaterializationPort>,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        // The journal must not lock directories used by filesystem fault fixtures.
        let jobs = crate::client::jobs::test_client_with_owner(shutdown.clone(), tasks).await;
        Self::new_with_jobs(
            mutations,
            profiles_path,
            jobs,
            fs,
            fetcher,
            materialization,
            shutdown,
            tasks,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) fn jobs(&self) -> nyanpasu_jobs::JobsClient {
        self.inner.jobs.client.clone()
    }

    pub(crate) async fn sync(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> crate::client::Result<CommitReport> {
        self.inner.jobs.sync(uid, patch).await
    }

    /// Lets the refresh scheduler, the external watchers and the journal
    /// ticker run. Until then they stay held; only the first call counts.
    pub(crate) fn start_producers(&self) -> Result<(), ProfilesError> {
        // The send error is not kept: it dumps the whole message, file contents
        // included.
        if self
            .inner
            .actor_ref
            .cast(ProfilesActorMessage::StartProducers)
            .is_err()
        {
            return ProfilesActorStoppedSnafu.fail();
        }
        Ok(())
    }

    /// The latest background-source receipt per profile.
    pub(crate) fn sources(&self) -> SourcesSnapshot {
        self.inner.sources.borrow().clone()
    }

    pub(crate) fn subscribe_sources(&self) -> tokio::sync::watch::Receiver<SourcesSnapshot> {
        self.inner.sources.clone()
    }

    /// The last committed profiles document. Reads bypass the mailbox, so an
    /// in-flight transaction parked in `on_prepare` cannot delay them.
    pub fn snapshot(&self) -> Arc<Profiles> {
        Arc::new(self.inner.snapshot.load().state.clone())
    }

    /// Read-only handle for collaborators that must observe committed state
    /// without holding a client that could write it.
    pub(crate) fn snapshot_handle(&self) -> StateSnapshot<Profiles> {
        self.inner.snapshot.clone()
    }

    pub async fn save_file(
        &self,
        uid: ProfileId,
        content: String,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::SaveFile {
                uid,
                content,
                reply,
            },
            None,
        )
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

    #[allow(dead_code)]
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

    #[cfg(test)]
    pub async fn refresh(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::RefreshRemote {
                uid,
                patch,
                origin: crate::state::profiles::RefreshOrigin::Manual,
                reply: Some(reply),
            },
            None,
        )
        .await
    }

    /// Fetch-before-commit remote import. No durable placeholder is written until
    /// download + validation succeed and the caller is still awaiting the result.
    pub async fn import(
        &self,
        url: url::Url,
        transform: Option<TransformKind>,
        metadata: ProfileMetadata,
        option: RemoteProfileOptions,
        update_interval_explicit: bool,
    ) -> Result<CommitReport, ProfilesError> {
        self.call(
            |reply| ProfilesActorMessage::ImportRemote {
                url,
                transform,
                metadata,
                option,
                update_interval_explicit,
                reply,
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
            Ok(CallResult::SenderError) => ProfilesReplyDroppedSnafu.fail(),
            Ok(CallResult::Timeout) => unreachable!("profiles calls are made without a timeout"),
            // The send error is not kept: it dumps the whole message, file
            // contents included.
            Err(_) => ProfilesActorStoppedSnafu.fail(),
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
    use crate::{
        service::profile_file::{ProfileFileService, SelfProxyPortSource},
        state::{
            mutation::CommitAborted,
            profiles::{
                RefreshOrigin,
                error::{MaterializationOperation, ProfileFileError, SubscriptionFetchError},
                ports::{
                    CleanupOutcome, FetchedSubscription, MaterializationReconcileReport,
                    MockProfileFsPort, MockProfileMaterializationPort, MockSubscriptionFetcher,
                    PreparedCleanup, PreparedMaterialization, ProfileDegradationCode,
                    ProfileDegradationPhase, ProfileFsPort, ProfileMaterializationPort,
                },
            },
        },
    };
    use nyanpasu_config::profile::{
        ConfigDefinition, ExternalMode, ExternalProfilePath, FileConfig, LocalBinding,
        ManagedProfilePath, MaterializedFile, OverlayTransform, ProfileDefinition, ProfileId,
        ProfileMetadata, ProfileSource, Profiles, RemoteProfileOptions, ScriptRuntime,
        ScriptTransform, SubscriptionInfo, TransformDefinition,
    };
    use nyanpasu_core::state::ReplaceIfVersionError;
    use struct_patch::Patch as _;
    use tempfile::{TempDir, tempdir};

    struct NoProxyPort;

    impl SelfProxyPortSource for NoProxyPort {
        fn mixed_port(&self) -> Option<u16> {
            None
        }
    }

    fn test_materialization_port() -> Arc<dyn ProfileMaterializationPort> {
        counted_materialization_port(Arc::new(std::sync::atomic::AtomicUsize::new(0)))
    }

    fn counted_materialization_port(
        calls: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Arc<dyn ProfileMaterializationPort> {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization.expect_reconcile().returning(move |_| {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(MaterializationReconcileReport::default())
        });
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("file".into())));
        materialization.expect_promote().returning(|_| Ok(()));
        materialization.expect_complete().returning(|_| Ok(()));
        materialization.expect_compensate().returning(|_| Ok(()));
        materialization
            .expect_prepare_cleanup()
            .returning(|_, _| Ok(PreparedCleanup::new("cleanup".into())));
        materialization
            .expect_activate_cleanup()
            .returning(|_| Ok(()));
        materialization
            .expect_cancel_cleanup()
            .returning(|_| Ok(()));
        materialization
            .expect_retry_cleanup()
            .returning(|_, _| Ok(CleanupOutcome::Removed));
        Arc::new(materialization)
    }

    fn failing_state_promote_materialization_port() -> Arc<dyn ProfileMaterializationPort> {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("state".into())));
        materialization
            .expect_promote()
            .returning(|_| Err(ProfileFileError::mock("disk full")));
        materialization.expect_compensate().returning(|_| Ok(()));
        Arc::new(materialization)
    }

    fn failing_cleanup_materialization_port() -> Arc<dyn ProfileMaterializationPort> {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_cleanup()
            .returning(|_, _| Ok(PreparedCleanup::new("cleanup".into())));
        materialization
            .expect_activate_cleanup()
            .returning(|_| Ok(()));
        materialization
            .expect_retry_cleanup()
            .returning(|_, _| Err(ProfileFileError::mock("disk on fire")));
        Arc::new(materialization)
    }

    fn add_placeholder_request() -> NewProfileRequest {
        NewProfileRequest {
            metadata: file_config_item("placeholder").metadata,
            definition: file_config_item("placeholder").definition,
        }
    }

    fn kind_switch_script_definition() -> ProfileDefinition {
        ProfileDefinition::Transform {
            transform: TransformDefinition::Script(ScriptTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new("client-supplied.lua").unwrap(),
                            updated_at: Some(time::OffsetDateTime::UNIX_EPOCH),
                        },
                    },
                },
                runtime: ScriptRuntime::Lua,
            }),
        }
    }

    fn assert_no_materialization_artifact_files(config_dir: &std::path::Path) {
        let root = config_dir
            .join("profiles")
            .join(".profile-materialization-v1");
        if !root.exists() {
            return;
        }
        let mut leftovers = Vec::new();
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let file_type = entry.file_type().unwrap();
                if file_type.is_dir() {
                    stack.push(path);
                } else {
                    leftovers.push(path);
                }
            }
        }
        assert!(
            leftovers.is_empty(),
            "successful materialization must not leave private artifacts: {leftovers:?}"
        );
    }

    pub(crate) fn temp_profiles_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).expect("utf-8 temp path")
    }

    pub(crate) async fn test_client_with(fs: MockProfileFsPort) -> (ProfilesClient, TempDir) {
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("profiles client should spawn");
        (client, dir)
    }

    #[tokio::test]
    async fn fresh_store_starts_with_default_profiles() {
        let (client, _dir) = test_client_with(MockProfileFsPort::new()).await;
        let snapshot = client.snapshot();
        assert!(snapshot.current.is_none());
        assert!(snapshot.items.is_empty());
        assert_eq!(snapshot.valid.len(), 3);
    }

    #[tokio::test]
    async fn writes_keep_the_schema_stamp_and_reload() {
        let (client, dir) = test_client_with(MockProfileFsPort::new()).await;
        client
            .set_valid_fields(vec!["dns".to_string()])
            .await
            .expect("set_valid_fields should commit");

        let raw = std::fs::read_to_string(temp_profiles_path(&dir)).unwrap();
        let inspected = nyanpasu_core::format::inspect(&raw).unwrap();
        assert_eq!(
            inspected.stamp().map(|stamp| stamp.schema_revision),
            Some(4),
            "{raw}"
        );
        let revision = inspected.into_payload().get("revision").cloned();
        assert_eq!(revision, Some(serde_yaml::Value::from(1)), "{raw}");

        let reloaded = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("a stamped profiles.yaml should load");
        assert_eq!(reloaded.snapshot().valid, vec!["dns".to_string()]);
    }

    #[tokio::test]
    async fn materialization_reconciles_before_client_startup_returns() {
        let dir = tempdir().unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let _client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            counted_materialization_port(Arc::clone(&calls)),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("profiles client should spawn");

        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
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
            let snapshot = client.snapshot();
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
    ) -> (ProfilesClient, TempDir) {
        remote_seeded_client_with_fetcher(fs, std::sync::Arc::new(fetcher)).await
    }

    async fn remote_seeded_client_with_fetcher(
        fs: MockProfileFsPort,
        fetcher: std::sync::Arc<dyn SubscriptionFetcher>,
    ) -> (ProfilesClient, TempDir) {
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            fetcher,
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
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
        ) -> Result<FetchedSubscription, SubscriptionFetchError> {
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
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let paths = crate::client::tests::test_paths(config_dir.path(), data_dir.path());
        let fs = Arc::new(ProfileFileService::new(
            paths,
            Arc::new(NoProxyPort),
            Arc::new(crate::client::tests::FixedDeviceInfoSource),
        ));
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
            fs.clone() as Arc<dyn ProfileFsPort>,
            Arc::new(ok_fetch("proxies: []\n")),
            fs.clone() as Arc<dyn ProfileMaterializationPort>,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(remote_config_item("r1"));
        client.replace(profiles).await.unwrap();

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
        assert_eq!(
            fs.read(&ManagedProfilePath::new("r1.yaml").unwrap())
                .unwrap(),
            "proxies: []\n"
        );
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
        let fs = MockProfileFsPort::new();
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            std::sync::Arc::new(fetcher),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
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
    async fn manual_refresh_ignores_server_interval_suggestions() {
        let fs = MockProfileFsPort::new();
        let (client, _dir) =
            remote_seeded_client(fs, suggested_fetch("proxies: []\n", Some(360))).await;

        let report = client.refresh(ProfileId("r1".into()), None).await.unwrap();
        let source = report.snapshot.items[&ProfileId("r1".into())]
            .definition
            .source()
            .unwrap();
        let ProfileSource::Remote { option, .. } = source else {
            unreachable!()
        };
        assert_eq!(option.update_interval_minutes, 120);
    }

    #[tokio::test(start_paused = true)]
    async fn import_suggestion_is_committed_and_reschedules_the_timer() {
        let _clock = crate::client::jobs::explicit_test_time();
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
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(fetcher),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        client.start_producers().unwrap();

        let report = client
            .import(
                url::Url::parse("https://example.com/sub").unwrap(),
                None,
                ProfileMetadata {
                    name: "imported".into(),
                    desc: None,
                    custom_name: true,
                },
                RemoteProfileOptions::default(),
                false,
            )
            .await
            .unwrap();
        let uid = report.created.clone().expect("import creates a uid");
        let source = report.snapshot.items[&uid].definition.source().unwrap();
        let ProfileSource::Remote { option, .. } = source else {
            unreachable!()
        };
        assert_eq!(option.update_interval_minutes, 60);
        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 1);

        tokio::time::advance(std::time::Duration::from_secs(60 * 60 + 1)).await;
        wait_for_source(&client, &uid.0, |source| {
            source.origin == SourceOrigin::ScheduledRefresh
        })
        .await;
        crate::client::jobs::settled(&client.jobs()).await;
        assert!(fetch_count.load(std::sync::atomic::Ordering::SeqCst) >= 2);
        let snapshot = client.snapshot();
        let source = snapshot.items[&uid].definition.source().unwrap();
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
            .returning(|_, _| Err(SubscriptionFetchError::mock()));
        let (client, _dir) = remote_seeded_client(MockProfileFsPort::new(), fetcher).await;
        let err = client
            .refresh(ProfileId("r1".into()), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::FetchSubscription { .. }));
        let snapshot = client.snapshot();
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
    async fn import_is_not_committed_when_materialization_promote_fails() {
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(suggested_fetch("proxies: []\n", Some(360))),
            failing_state_promote_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();

        assert!(
            client
                .import(
                    url::Url::parse("https://example.com/sub").unwrap(),
                    None,
                    ProfileMetadata {
                        name: "imported".into(),
                        desc: None,
                        custom_name: true,
                    },
                    RemoteProfileOptions::default(),
                    false,
                )
                .await
                .is_err()
        );
        assert!(
            client.snapshot().items.is_empty(),
            "state-first promote failure with full rollback must leave zero items"
        );
    }

    #[tokio::test]
    async fn refresh_rejects_non_remote_and_unknown_and_concurrent() {
        let (client, _dir) = seeded_client().await;
        let err = client
            .refresh(ProfileId("cfg1".into()), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::NotARemoteProfile { .. }));
        let err = client
            .refresh(ProfileId("ghost".into()), None)
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ProfileNotFound { .. }));

        let (started, started_rx) = tokio::sync::oneshot::channel();
        let release_fetch = std::sync::Arc::new(tokio::sync::Notify::new());
        let fetcher = HoldingFetcher {
            started: std::sync::Mutex::new(Some(started)),
            release: std::sync::Arc::clone(&release_fetch),
        };
        let fs = MockProfileFsPort::new();
        let (client, _dir) =
            remote_seeded_client_with_fetcher(fs, std::sync::Arc::new(fetcher)).await;
        let c2 = client.clone();
        let first = tokio::spawn(async move { c2.refresh(ProfileId("r1".into()), None).await });
        started_rx.await.unwrap();
        let err = loop {
            match client.refresh(ProfileId("r1".into()), None).await {
                Err(error @ ProfilesError::RefreshInProgress { .. }) => break error,
                Ok(_) => panic!("second refresh must not both succeed before first settles"),
                Err(_) => tokio::task::yield_now().await,
            }
        };
        assert!(matches!(err, ProfilesError::RefreshInProgress { .. }));
        release_fetch.notify_waiters();
        first.await.unwrap().expect("first refresh completes");
    }

    #[tokio::test]
    async fn refresh_commit_refreshed_deleted_profile_settles_reply_without_writes() {
        // A refresh that settles after Delete must not materialize content for
        // the removed profile; Delete owns its cleanup transaction.
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let release_fetch = std::sync::Arc::new(tokio::sync::Notify::new());
        let fetcher = HoldingFetcher {
            started: std::sync::Mutex::new(Some(started)),
            release: std::sync::Arc::clone(&release_fetch),
        };
        let (client, _dir) = remote_seeded_client_with_fetcher(
            MockProfileFsPort::new(),
            std::sync::Arc::new(fetcher),
        )
        .await;

        let c = client.clone();
        let pending = tokio::spawn(async move { c.refresh(ProfileId("r1".into()), None).await });
        started_rx.await.unwrap();
        client.delete(ProfileId("r1".into())).await.unwrap();
        release_fetch.notify_waiters();

        let err = pending.await.unwrap().unwrap_err();
        assert!(matches!(
            err,
            ProfilesError::ProfileDeletedDuringRefresh { .. }
        ));
        assert!(
            client
                .snapshot()
                .items
                .get(&ProfileId("r1".into()))
                .is_none()
        );
    }

    /// Review fix regression pin (2026-07-11): a download committed after the
    /// definition was replaced with a different URL must be discarded — no
    /// file write, no metadata update.
    #[tokio::test]
    async fn refresh_commit_is_fenced_when_url_changed_mid_download() {
        let fs = MockProfileFsPort::new();
        // The stale commit must never materialize or update metadata.
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let release_fetch = std::sync::Arc::new(tokio::sync::Notify::new());
        let fetcher = HoldingFetcher {
            started: std::sync::Mutex::new(Some(started)),
            release: std::sync::Arc::clone(&release_fetch),
        };
        let (client, _dir) =
            remote_seeded_client_with_fetcher(fs, std::sync::Arc::new(fetcher)).await;

        let c = client.clone();
        let pending = tokio::spawn(async move { c.refresh(ProfileId("r1".into()), None).await });
        started_rx.await.unwrap();

        let mut replacement = remote_config_item("r1");
        if let Some(ProfileSource::Remote { url, .. }) = replacement.definition.source_mut() {
            *url = url::Url::parse("https://old.example.com/replaced").unwrap();
        }
        client
            .replace_definition(ProfileId("r1".into()), replacement.definition)
            .await
            .unwrap();
        release_fetch.notify_waiters();

        let err = pending.await.unwrap().unwrap_err();
        assert!(matches!(
            err,
            ProfilesError::ProfileChangedDuringRefresh { .. }
        ));
        let snapshot = client.snapshot();
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

    #[tokio::test]
    async fn refresh_commit_is_fenced_when_same_url_definition_changes_mid_download() {
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let release_fetch = Arc::new(tokio::sync::Notify::new());
        let fetcher = HoldingFetcher {
            started: std::sync::Mutex::new(Some(started)),
            release: Arc::clone(&release_fetch),
        };
        let (client, _dir) =
            remote_seeded_client_with_fetcher(MockProfileFsPort::new(), Arc::new(fetcher)).await;

        let refresh_client = client.clone();
        let pending =
            tokio::spawn(async move { refresh_client.refresh(ProfileId("r1".into()), None).await });
        started_rx.await.unwrap();
        let mut patch = RemoteProfileOptions::new_empty_patch();
        patch.update_interval_minutes = Some(60);
        client
            .patch_remote_options(ProfileId("r1".into()), patch)
            .await
            .unwrap();
        release_fetch.notify_waiters();

        let err = pending.await.unwrap().unwrap_err();
        assert!(matches!(
            err,
            ProfilesError::ProfileChangedDuringRefresh { .. }
        ));
        assert!(
            client.snapshot().items[&ProfileId("r1".into())]
                .definition
                .source()
                .unwrap()
                .materialized()
                .updated_at
                .is_none()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_fires_refresh_on_interval() {
        let _clock = crate::client::jobs::explicit_test_time();
        let fs = MockProfileFsPort::new();
        let fetch_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let fetched = std::sync::Arc::new(tokio::sync::Notify::new());
        let mut fetcher = MockSubscriptionFetcher::new();
        let counter = std::sync::Arc::clone(&fetch_count);
        let fetched_notice = std::sync::Arc::clone(&fetched);
        fetcher.expect_fetch().returning(move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            fetched_notice.notify_one();
            Ok(FetchedSubscription {
                content: "proxies: []\n".into(),
                filename: None,
                subscription: SubscriptionInfo::default(),
                suggested_update_interval_minutes: None,
            })
        });
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            std::sync::Arc::new(fetcher),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        client.start_producers().unwrap();
        let mut item = remote_config_item("r1");
        if let Some(source) = item.definition.source_mut() {
            source.materialized_mut().updated_at = Some(time::OffsetDateTime::now_utc());
        }
        let mut profiles = Profiles::default();
        profiles.append_item(item);
        client.replace(profiles).await.unwrap();

        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 0);
        tokio::time::advance(std::time::Duration::from_secs(120 * 60 + 1)).await;
        tokio::time::timeout(std::time::Duration::from_secs(1), fetched.notified())
            .await
            .expect("scheduled refresh should reach the fetcher");
        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 1);
        drop(client);
    }

    #[tokio::test(start_paused = true)]
    async fn scheduler_reconcile_add_remove_and_kind_switch() {
        let _clock = crate::client::jobs::explicit_test_time();
        let fetch_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut fetcher = MockSubscriptionFetcher::new();
        let counter = std::sync::Arc::clone(&fetch_count);
        fetcher.expect_fetch().returning(move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(SubscriptionFetchError::mock())
        });
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(fetcher),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        // Armed before the seed, so the seed is scheduled without a catch-up.
        client.start_producers().unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(remote_config_item("r1"));
        client.replace(profiles).await.unwrap();

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
        let _clock = crate::client::jobs::explicit_test_time();
        let fetch_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut fetcher = MockSubscriptionFetcher::new();
        let counter = std::sync::Arc::clone(&fetch_count);
        fetcher.expect_fetch().returning(move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(SubscriptionFetchError::mock())
        });
        let dir = tempdir().unwrap();
        {
            let client = ProfilesClient::new(
                crate::state::mutation::MutationCoordinator::isolated(),
                temp_profiles_path(&dir),
                std::sync::Arc::new(MockProfileFsPort::new()),
                std::sync::Arc::new(MockSubscriptionFetcher::new()),
                test_materialization_port(),
                tokio_util::sync::CancellationToken::new(),
                &tokio_util::task::TaskTracker::new(),
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
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            std::sync::Arc::new(MockProfileFsPort::new()),
            std::sync::Arc::new(fetcher),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        client.start_producers().unwrap();
        wait_for_source(&client, "r1", |source| {
            source.origin == SourceOrigin::ScheduledRefresh
        })
        .await;
        crate::client::jobs::settled(&client.jobs()).await;
        assert_eq!(fetch_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn external_mirror_change_syncs_copy_and_bumps_updated_at() {
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        let target_path = target_dir.path().join("external.yaml");
        std::fs::write(&target_path, "proxies: []\n").unwrap();
        let paths = crate::client::tests::test_paths(config_dir.path(), data_dir.path());
        let fs = std::sync::Arc::new(ProfileFileService::new(
            paths,
            std::sync::Arc::new(NoProxyPort),
            Arc::new(crate::client::tests::FixedDeviceInfoSource),
        ));
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
            fs.clone() as Arc<dyn ProfileFsPort>,
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            fs.clone() as Arc<dyn ProfileMaterializationPort>,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        client.start_producers().unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(external_item(
            "ext1",
            ExternalMode::Mirror,
            external_path(&target_path),
        ));
        client.replace(profiles).await.unwrap();

        // Production path: OS watcher casts ExternalFileChanged after the
        // external binding target changes. Poll with yields (no sleep).
        std::fs::write(&target_path, "proxies: []\n# touched\n").unwrap();

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
        let mirrored = config_dir.path().join("profiles").join("ext1.yaml");
        let content = std::fs::read_to_string(mirrored).unwrap();
        assert!(content.contains("# touched"));
    }

    #[tokio::test]
    async fn external_symlink_change_bumps_updated_at_without_copy() {
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        let target_path = target_dir.path().join("external.yaml");
        std::fs::write(&target_path, "proxies: []\n").unwrap();
        let paths = crate::client::tests::test_paths(config_dir.path(), data_dir.path());
        let fs = std::sync::Arc::new(ProfileFileService::new(
            paths,
            std::sync::Arc::new(NoProxyPort),
            Arc::new(crate::client::tests::FixedDeviceInfoSource),
        ));
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
            fs.clone() as Arc<dyn ProfileFsPort>,
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            fs.clone() as Arc<dyn ProfileMaterializationPort>,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        client.start_producers().unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(external_item(
            "ext1",
            ExternalMode::Symlink,
            external_path(&target_path),
        ));
        client.replace(profiles).await.unwrap();

        std::fs::write(&target_path, "proxies: []\n# touched\n").unwrap();
        wait_for_updated_at(&client, "ext1").await;
    }

    #[tokio::test]
    async fn external_background_commit_does_not_bypass_the_mutation_participant() {
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        let target_path = target_dir.path().join("external.yaml");
        std::fs::write(&target_path, "proxies: []\n").unwrap();
        let paths = crate::client::tests::test_paths(config_dir.path(), data_dir.path());
        let fs = std::sync::Arc::new(ProfileFileService::new(
            paths,
            std::sync::Arc::new(NoProxyPort),
            Arc::new(crate::client::tests::FixedDeviceInfoSource),
        ));
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
            fs.clone() as Arc<dyn ProfileFsPort>,
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            fs.clone() as Arc<dyn ProfileMaterializationPort>,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        client.start_producers().unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(external_item(
            "ext1",
            ExternalMode::Mirror,
            external_path(&target_path),
        ));
        profiles.set_current(Some(ProfileId("ext1".into())));
        client.replace(profiles).await.unwrap();

        std::fs::write(&target_path, "proxies: []\n# touched\n").unwrap();
        wait_for_updated_at(&client, "ext1").await;
    }

    #[tokio::test]
    async fn external_watcher_smoke_mirror_real_file_event() {
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        let target_path = target_dir.path().join("external.yaml");
        std::fs::write(&target_path, "proxies: []\n").unwrap();
        let paths = crate::client::tests::test_paths(config_dir.path(), data_dir.path());
        let fs = std::sync::Arc::new(ProfileFileService::new(
            paths,
            std::sync::Arc::new(NoProxyPort),
            Arc::new(crate::client::tests::FixedDeviceInfoSource),
        ));
        let profiles_path =
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            profiles_path,
            fs.clone() as Arc<dyn ProfileFsPort>,
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            fs.clone() as Arc<dyn ProfileMaterializationPort>,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        client.start_producers().unwrap();
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
        let content = std::fs::read_to_string(config_dir.path().join("profiles").join("ext1.yaml"))
            .expect("real external watcher event should mirror the changed file");
        assert!(content.contains("mode: rule"));
    }

    /// An operation id on the receipt is the source having classified the
    /// write as reaching the runtime and taken the Runtime into it.
    #[tokio::test]
    async fn set_current_commits_and_reaches_the_runtime_even_when_reselected() {
        let (client, dir) = seeded_client().await;
        let report = client
            .set_current(Some(ProfileId("cfg1".into())))
            .await
            .expect("activate cfg1");
        assert!(report.receipt.operation_id.is_some());
        assert_eq!(report.snapshot.current, Some(ProfileId("cfg1".into())));
        assert_eq!(report.snapshot.revision(), 2);

        let report = client
            .set_current(Some(ProfileId("cfg1".into())))
            .await
            .unwrap();
        assert!(
            report.receipt.operation_id.is_some(),
            "re-selecting the current profile still asks for the runtime"
        );
        assert_eq!(report.snapshot.revision(), 3);

        drop(client);
        let (client, _dir2) = {
            let path = temp_profiles_path(&dir);
            let client = ProfilesClient::new(
                crate::state::mutation::MutationCoordinator::isolated(),
                path,
                std::sync::Arc::new(MockProfileFsPort::new()),
                std::sync::Arc::new(MockSubscriptionFetcher::new()),
                test_materialization_port(),
                tokio_util::sync::CancellationToken::new(),
                &tokio_util::task::TaskTracker::new(),
            )
            .await
            .unwrap();
            (client, dir)
        };
        assert_eq!(client.snapshot().current, Some(ProfileId("cfg1".into())));
    }

    #[tokio::test]
    async fn set_current_if_none_only_activates_when_empty() {
        let (client, _dir) = seeded_client().await;

        // Empty current -> activates.
        let report = client
            .set_current_if_none(ProfileId("cfg1".into()))
            .await
            .expect("call ok")
            .expect("activated when current was empty");
        assert_eq!(report.snapshot.current, Some(ProfileId("cfg1".into())));

        // A current already exists -> does NOT activate (returns None) and
        // leaves the existing selection untouched.
        let skipped = client
            .set_current_if_none(ProfileId("cfg2".into()))
            .await
            .expect("call ok");
        assert!(skipped.is_none(), "must not overwrite an existing current");
        assert_eq!(client.snapshot().current, Some(ProfileId("cfg1".into())));
    }

    #[tokio::test]
    async fn set_current_rejects_missing_and_transform_targets() {
        let (client, _dir) = seeded_client().await;
        let err = client
            .set_current(Some(ProfileId("ghost".into())))
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed { .. }));
        let err = client
            .set_current(Some(ProfileId("ovl1".into())))
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed { .. }));
        assert!(client.snapshot().current.is_none());
    }

    #[tokio::test]
    async fn set_global_transforms_validates_kind_and_reports_change() {
        let (client, _dir) = seeded_client().await;
        client
            .set_global_transforms(vec![ProfileId("ovl1".into())])
            .await
            .expect("set transforms");

        let err = client
            .set_global_transforms(vec![ProfileId("cfg1".into())])
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ValidationFailed { .. }));
    }

    #[tokio::test]
    async fn set_valid_fields_commits_and_always_reaches_the_runtime() {
        let (client, _dir) = seeded_client().await;
        let report = client
            .set_valid_fields(vec!["dns".into(), "tun".into()])
            .await
            .expect("set valid fields");
        assert!(
            report.receipt.operation_id.is_some(),
            "whitelist change must trigger rebuild"
        );
        assert_eq!(
            report.snapshot.valid,
            vec!["dns".to_string(), "tun".to_string()]
        );
    }

    #[tokio::test]
    async fn add_generates_uid_canonical_path_and_writes_initial_file() {
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let paths = crate::client::tests::test_paths(config_dir.path(), data_dir.path());
        let fs = Arc::new(ProfileFileService::new(
            paths,
            Arc::new(NoProxyPort),
            Arc::new(crate::client::tests::FixedDeviceInfoSource),
        ));
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
            fs.clone() as Arc<dyn ProfileFsPort>,
            Arc::new(MockSubscriptionFetcher::new()),
            fs.clone() as Arc<dyn ProfileMaterializationPort>,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();

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

        assert!(report.degradations.is_empty());
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
        assert_eq!(
            std::fs::read_to_string(config_dir.path().join("profiles").join(file)).unwrap(),
            "proxies: []\n"
        );
    }

    #[tokio::test]
    async fn add_promotion_failure_rolls_back_before_reporting_created() {
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            failing_state_promote_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();

        let err = client
            .add(
                NewProfileRequest {
                    metadata: file_config_item("placeholder").metadata,
                    definition: file_config_item("placeholder").definition,
                },
                Some("proxies: []\n".to_string()),
            )
            .await
            .expect_err("promote failure must fail Add");
        assert!(matches!(
            err,
            ProfilesError::Commit {
                source: CommitAborted::WriteConfig { .. },
                ..
            }
        ));
        assert!(client.snapshot().items.is_empty());
    }

    /// Promotion is before the source CAS: losing the config directory while
    /// promoting must not make a forward source snapshot visible.
    #[tokio::test]
    async fn add_promote_failure_never_publishes_source_when_config_directory_disappears() {
        let root = tempdir().unwrap();
        let live = root.path().join("live");
        std::fs::create_dir(&live).unwrap();
        let profiles_path =
            Utf8PathBuf::from_path_buf(live.join("profiles.yaml")).expect("utf-8 temp path");
        let dead = root.path().join("dead");
        let durable_forward = dead.join("profiles.yaml");

        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("state".into())));
        let live_for_promote = live.clone();
        let dead_for_promote = dead.clone();
        materialization.expect_promote().returning(move |_| {
            // Simulate losing the config directory before the source CAS.
            std::fs::rename(&live_for_promote, &dead_for_promote)
                .expect("move profiles parent aside before source CAS");
            Err(ProfileFileError::mock("disk full"))
        });
        let compensate_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        {
            let compensate_calls = Arc::clone(&compensate_calls);
            materialization.expect_compensate().returning(move |_| {
                compensate_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            });
        }

        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            profiles_path,
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            Arc::new(materialization),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();

        let error = client
            .add(add_placeholder_request(), Some("proxies: []\n".into()))
            .await
            .expect_err("promotion failure must reject before the source CAS");
        assert!(matches!(
            error,
            ProfilesError::Commit {
                source: CommitAborted::WriteConfig { .. },
                ..
            }
        ));
        assert!(client.snapshot().items.is_empty());
        assert!(
            !durable_forward.exists(),
            "no source was persisted before promotion"
        );
        assert_eq!(
            compensate_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[tokio::test]
    async fn add_prepare_failure_has_zero_commit() {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        let promote_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let compensate_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Err(ProfileFileError::mock("staging refused")));
        {
            let promote_calls = Arc::clone(&promote_calls);
            materialization.expect_promote().returning(move |_| {
                promote_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            });
        }
        {
            let compensate_calls = Arc::clone(&compensate_calls);
            materialization.expect_compensate().returning(move |_| {
                compensate_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            });
        }

        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            Arc::new(materialization),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();

        let err = client
            .add(add_placeholder_request(), Some("proxies: []\n".into()))
            .await
            .expect_err("prepare failure must fail Add");
        assert!(
            matches!(
                err,
                ProfilesError::Materialization {
                    operation: MaterializationOperation::PrepareFileFirst,
                    ..
                }
            ),
            "prepare-phase failure expected, got: {err:?}"
        );
        assert!(client.snapshot().items.is_empty());
        assert_eq!(promote_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(
            compensate_calls.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }

    #[tokio::test]
    async fn add_promote_and_compensate_failure_reports_compound_error() {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("state".into())));
        materialization
            .expect_promote()
            .returning(|_| Err(ProfileFileError::mock("disk full")));
        let compensate_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        {
            let compensate_calls = Arc::clone(&compensate_calls);
            materialization.expect_compensate().returning(move |_| {
                compensate_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(ProfileFileError::mock("cannot restore"))
            });
        }

        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            Arc::new(materialization),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();

        let err = client
            .add(add_placeholder_request(), Some("proxies: []\n".into()))
            .await
            .expect_err("compound materialization failure");
        let ProfilesError::Commit {
            source:
                CommitAborted::RecoverAfterWriteFailure {
                    source:
                        ReplaceIfVersionError::ResourceRecovery {
                            cause,
                            recovery_error,
                        },
                    ..
                },
            ..
        } = &err
        else {
            panic!("compound failure expected, got: {err:?}");
        };
        assert!(
            format!("{cause:#}").contains("disk full")
                && format!("{recovery_error:#}").contains("cannot restore"),
            "compound error must carry promotion and compensate failures: {err:?}"
        );
        assert_eq!(
            compensate_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert!(client.snapshot().items.is_empty());
    }

    #[tokio::test]
    async fn add_complete_failure_returns_committed_degradation() {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("state".into())));
        materialization.expect_promote().returning(|_| Ok(()));
        materialization
            .expect_complete()
            .returning(|_| Err(ProfileFileError::mock("journal stuck")));

        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            Arc::new(materialization),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();

        let report = client
            .add(add_placeholder_request(), Some("proxies: []\n".into()))
            .await
            .expect("complete failure is committed-degraded");
        assert!(report.created.is_some());
        assert_eq!(report.snapshot.items.len(), 1);
        let created = report.created.clone().unwrap();
        assert!(
            client.snapshot().items.contains_key(&created),
            "complete degradation must leave the committed item durable"
        );
        assert_eq!(report.degradations.len(), 1);
        assert_eq!(
            report.degradations[0].phase,
            ProfileDegradationPhase::Reconcile
        );
        assert_eq!(
            report.degradations[0].code,
            ProfileDegradationCode::MaterializationDeferred
        );
        assert!(report.degradations[0].code.retryable());
    }

    #[tokio::test]
    async fn replace_definition_prepare_cleanup_failure_compensates_new_and_preserves_old() {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("new".into())));
        materialization
            .expect_prepare_cleanup()
            .returning(|_, _| Err(ProfileFileError::mock("cleanup journal refused")));
        let compensate_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        {
            let compensate_calls = Arc::clone(&compensate_calls);
            materialization.expect_compensate().returning(move |_| {
                compensate_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            });
        }

        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            Arc::new(materialization),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(file_config_item("cfg2"));
        client.replace(profiles).await.unwrap();

        let err = client
            .replace_definition(ProfileId("cfg2".into()), kind_switch_script_definition())
            .await
            .expect_err("prepare-cleanup failure must fail ReplaceDefinition");
        assert!(matches!(
            err,
            ProfilesError::Materialization {
                operation: MaterializationOperation::PrepareCleanup,
                ..
            }
        ));
        assert_eq!(
            compensate_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        let snapshot = client.snapshot();
        let item = &snapshot.items[&ProfileId("cfg2".into())];
        match &item.definition {
            ProfileDefinition::Config { config } => {
                assert_eq!(
                    config.source().unwrap().materialized().file.as_str(),
                    "cfg2.yaml"
                );
            }
            other => panic!("old definition must be preserved, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn replace_definition_promote_failure_cancels_cleanup_and_rolls_back() {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("new".into())));
        materialization
            .expect_prepare_cleanup()
            .returning(|_, _| Ok(PreparedCleanup::new("old".into())));
        materialization
            .expect_promote()
            .returning(|_| Err(ProfileFileError::mock("disk full")));
        let cancel_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let compensate_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        {
            let cancel_calls = Arc::clone(&cancel_calls);
            materialization.expect_cancel_cleanup().returning(move |_| {
                cancel_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            });
        }
        {
            let compensate_calls = Arc::clone(&compensate_calls);
            materialization.expect_compensate().returning(move |_| {
                compensate_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            });
        }

        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            Arc::new(materialization),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(file_config_item("cfg2"));
        client.replace(profiles).await.unwrap();

        let err = client
            .replace_definition(ProfileId("cfg2".into()), kind_switch_script_definition())
            .await
            .expect_err("promote failure must roll back ReplaceDefinition");
        assert!(matches!(
            err,
            ProfilesError::Commit {
                source: CommitAborted::WriteConfig { .. },
                ..
            }
        ));
        assert_eq!(cancel_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            compensate_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        let snapshot = client.snapshot();
        let item = &snapshot.items[&ProfileId("cfg2".into())];
        match &item.definition {
            ProfileDefinition::Config { config } => {
                assert_eq!(
                    config.source().unwrap().materialized().file.as_str(),
                    "cfg2.yaml"
                );
            }
            other => panic!("old definition must be restored, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn refresh_promote_failure_compensates_without_advancing_metadata() {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("file".into())));
        materialization
            .expect_promote()
            .returning(|_| Err(ProfileFileError::mock("disk full")));
        let compensate_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        {
            let compensate_calls = Arc::clone(&compensate_calls);
            materialization.expect_compensate().returning(move |_| {
                compensate_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            });
        }

        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(ok_fetch("proxies: []\n")),
            Arc::new(materialization),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(remote_config_item("r1"));
        client.replace(profiles).await.unwrap();

        let err = client
            .refresh(ProfileId("r1".into()), None)
            .await
            .expect_err("refresh promote failure");
        assert!(matches!(
            err,
            ProfilesError::Commit {
                source: CommitAborted::WriteConfig { .. },
                ..
            }
        ));
        assert_eq!(
            compensate_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        let snapshot = client.snapshot();
        let source = snapshot.items[&ProfileId("r1".into())]
            .definition
            .source()
            .unwrap();
        assert!(source.materialized().updated_at.is_none());
    }

    #[tokio::test]
    async fn refresh_complete_failure_commits_with_degradation() {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_file_first()
            .returning(|_, _, _| Ok(PreparedMaterialization::new("file".into())));
        materialization.expect_promote().returning(|_| Ok(()));
        materialization
            .expect_complete()
            .returning(|_| Err(ProfileFileError::mock("journal stuck")));

        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(ok_fetch("proxies: []\n")),
            Arc::new(materialization),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(remote_config_item("r1"));
        client.replace(profiles).await.unwrap();

        let report = client
            .refresh(ProfileId("r1".into()), None)
            .await
            .expect("complete failure is committed-degraded");
        assert_eq!(report.degradations.len(), 1);
        assert_eq!(
            report.degradations[0].phase,
            ProfileDegradationPhase::Reconcile
        );
        assert_eq!(
            report.degradations[0].code,
            ProfileDegradationCode::MaterializationDeferred
        );
        assert!(report.degradations[0].code.retryable());
        assert!(
            report.snapshot.items[&ProfileId("r1".into())]
                .definition
                .source()
                .unwrap()
                .materialized()
                .updated_at
                .is_some()
        );
    }

    #[tokio::test]
    async fn delete_prepare_failure_preserves_item() {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_cleanup()
            .returning(|_, _| Err(ProfileFileError::mock("cleanup journal refused")));

        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            Arc::new(materialization),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        client.replace(seeded_profiles()).await.unwrap();

        let err = client
            .delete(ProfileId("cfg2".into()))
            .await
            .expect_err("prepare-cleanup failure must fail Delete");
        assert!(matches!(
            err,
            ProfilesError::Materialization {
                operation: MaterializationOperation::PrepareCleanup,
                ..
            }
        ));
        assert!(
            client
                .snapshot()
                .items
                .contains_key(&ProfileId("cfg2".into()))
        );
    }

    #[tokio::test]
    async fn delete_activate_failure_commits_deletion_with_cleanup_degradation() {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_cleanup()
            .returning(|_, _| Ok(PreparedCleanup::new("cleanup".into())));
        materialization
            .expect_activate_cleanup()
            .returning(|_| Err(ProfileFileError::mock("activate refused")));
        let retry_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        {
            let retry_calls = Arc::clone(&retry_calls);
            materialization
                .expect_retry_cleanup()
                .returning(move |_, _| {
                    retry_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(CleanupOutcome::Removed)
                });
        }

        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            Arc::new(materialization),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        client.replace(seeded_profiles()).await.unwrap();

        let report = client
            .delete(ProfileId("cfg2".into()))
            .await
            .expect("activate failure is committed-degraded");
        assert!(
            report
                .snapshot
                .items
                .get(&ProfileId("cfg2".into()))
                .is_none()
        );
        assert_eq!(report.degradations.len(), 1);
        assert_eq!(
            report.degradations[0].phase,
            ProfileDegradationPhase::Cleanup
        );
        assert_eq!(
            report.degradations[0].code,
            ProfileDegradationCode::CleanupDeferred
        );
        assert_eq!(retry_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn caller_aborted_refresh_eventually_settles_and_does_not_leave_pending_stuck() {
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let release_fetch = Arc::new(tokio::sync::Notify::new());
        let holds_remaining = Arc::new(std::sync::atomic::AtomicUsize::new(1));
        struct FirstHoldFetcher {
            started: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
            release: Arc<tokio::sync::Notify>,
            holds_remaining: Arc<std::sync::atomic::AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl SubscriptionFetcher for FirstHoldFetcher {
            async fn fetch(
                &self,
                _url: &url::Url,
                _options: &RemoteProfileOptions,
            ) -> Result<FetchedSubscription, SubscriptionFetchError> {
                let should_hold = self
                    .holds_remaining
                    .fetch_update(
                        std::sync::atomic::Ordering::SeqCst,
                        std::sync::atomic::Ordering::SeqCst,
                        |n| n.checked_sub(1),
                    )
                    .map(|prev| prev == 1)
                    .unwrap_or(false);
                if should_hold {
                    if let Some(started) = self.started.lock().unwrap().take() {
                        let _ = started.send(());
                    }
                    self.release.notified().await;
                }
                Ok(FetchedSubscription {
                    content: "proxies: []\n".into(),
                    filename: None,
                    subscription: SubscriptionInfo {
                        upload: Some(1),
                        ..Default::default()
                    },
                    suggested_update_interval_minutes: None,
                })
            }
        }
        let fetcher = FirstHoldFetcher {
            started: std::sync::Mutex::new(Some(started)),
            release: Arc::clone(&release_fetch),
            holds_remaining,
        };
        let (client, _dir) =
            remote_seeded_client_with_fetcher(MockProfileFsPort::new(), Arc::new(fetcher)).await;

        let refresh_client = client.clone();
        let pending =
            tokio::spawn(async move { refresh_client.refresh(ProfileId("r1".into()), None).await });
        started_rx.await.unwrap();
        pending.abort();
        let join_err = pending.await.expect_err("aborted refresh join must fail");
        assert!(
            join_err.is_cancelled(),
            "caller-aborted refresh must cancel the waiting call future"
        );

        release_fetch.notify_waiters();

        // CommitRefreshed removes pending_refresh even when the reply port is gone.
        // Bound the poll so a stuck pending_refresh fails the test instead of hanging CI.
        let report = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                match client.refresh(ProfileId("r1".into()), None).await {
                    Ok(report) => break report,
                    Err(ProfilesError::RefreshInProgress { .. }) => {
                        tokio::task::yield_now().await;
                    }
                    Err(other) => {
                        panic!("subsequent refresh must settle cleanly, got {other:?}")
                    }
                }
            }
        })
        .await
        .expect("aborted refresh must clear pending_refresh promptly");
        assert!(
            report.snapshot.items[&ProfileId("r1".into())]
                .definition
                .source()
                .unwrap()
                .materialized()
                .updated_at
                .is_some()
        );
    }

    #[tokio::test]
    async fn add_success_leaves_no_materialization_staging_leftovers() {
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let paths = crate::client::tests::test_paths(config_dir.path(), data_dir.path());
        let fs = Arc::new(ProfileFileService::new(
            paths,
            Arc::new(NoProxyPort),
            Arc::new(crate::client::tests::FixedDeviceInfoSource),
        ));
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
            fs.clone() as Arc<dyn ProfileFsPort>,
            Arc::new(MockSubscriptionFetcher::new()),
            fs.clone() as Arc<dyn ProfileMaterializationPort>,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();

        let report = client
            .add(add_placeholder_request(), Some("proxies: []\n".into()))
            .await
            .expect("add success");
        assert!(report.degradations.is_empty());
        let created = report.created.expect("created uid");
        let file = report.snapshot.items[&created]
            .definition
            .source()
            .unwrap()
            .materialized()
            .file
            .as_str();
        assert_eq!(
            std::fs::read_to_string(config_dir.path().join("profiles").join(file)).unwrap(),
            "proxies: []\n"
        );
        assert_no_materialization_artifact_files(config_dir.path());
    }

    #[tokio::test]
    async fn refresh_persist_failure_restores_previous_materialized_bytes() {
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let paths = crate::client::tests::test_paths(config_dir.path(), data_dir.path());
        let fs = Arc::new(ProfileFileService::new(
            paths,
            Arc::new(NoProxyPort),
            Arc::new(crate::client::tests::FixedDeviceInfoSource),
        ));
        let profiles_path =
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            profiles_path.clone(),
            fs.clone() as Arc<dyn ProfileFsPort>,
            Arc::new(ok_fetch("proxies:\n  - name: new\n")),
            fs.clone() as Arc<dyn ProfileMaterializationPort>,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(remote_config_item("r1"));
        client.replace(profiles).await.unwrap();
        let path = ManagedProfilePath::new("r1.yaml").unwrap();
        fs.write_atomic(&path, "proxies:\n  - name: old\n").unwrap();

        std::fs::remove_file(&profiles_path).unwrap();
        std::fs::create_dir(&profiles_path).unwrap();
        assert!(matches!(
            client.refresh(ProfileId("r1".into()), None).await,
            Err(ProfilesError::Commit {
                source: CommitAborted::WriteConfig { .. },
                ..
            })
        ));
        assert_eq!(
            fs.read(&path).unwrap(),
            "proxies:\n  - name: old\n",
            "file-first compensation must restore old bytes after metadata CAS failure"
        );
    }

    #[tokio::test]
    async fn add_script_transform_uses_runtime_extension() {
        let (client, _dir) = test_client_with(MockProfileFsPort::new()).await;
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
        assert!(matches!(err, ProfilesError::ProfileNotFound { .. }));
    }

    #[tokio::test]
    async fn delete_unreferenced_managed_profile_removes_file() {
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let paths = crate::client::tests::test_paths(config_dir.path(), data_dir.path());
        let fs = Arc::new(ProfileFileService::new(
            paths,
            Arc::new(NoProxyPort),
            Arc::new(crate::client::tests::FixedDeviceInfoSource),
        ));
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
            fs.clone() as Arc<dyn ProfileFsPort>,
            Arc::new(MockSubscriptionFetcher::new()),
            fs.clone() as Arc<dyn ProfileMaterializationPort>,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(file_config_item("cfg2"));
        client.replace(profiles).await.unwrap();
        let path = ManagedProfilePath::new("cfg2.yaml").unwrap();
        fs.write_atomic(&path, "proxies: []\n").unwrap();

        let report = client
            .delete(ProfileId("cfg2".into()))
            .await
            .expect("delete cfg2");
        assert!(
            report
                .snapshot
                .items
                .get(&ProfileId("cfg2".into()))
                .is_none()
        );
        assert!(
            fs.read(&path).is_err(),
            "cleanup must remove the managed file"
        );
    }

    #[tokio::test]
    async fn delete_cleanup_failure_degrades_to_warning() {
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            failing_cleanup_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        client.replace(seeded_profiles()).await.unwrap();
        let report = client
            .delete(ProfileId("cfg2".into()))
            .await
            .expect("delete commits anyway");
        assert_eq!(report.degradations.len(), 1);
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
        assert!(matches!(err, ProfilesError::ReorderListSizeMismatch { .. }));

        let err = client
            .reorder(ReorderOp::ByList(vec![
                ProfileId("cfg1".into()),
                ProfileId("cfg1".into()),
                ProfileId("ovl1".into()),
            ]))
            .await
            .unwrap_err();
        assert!(matches!(err, ProfilesError::ReorderListDuplicate { .. }));
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
        assert!(matches!(err, ProfilesError::NotARemoteProfile { .. }));
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
        client
            .replace_definition(ProfileId("cfg1".into()), definition)
            .await
            .unwrap();

        let definition = seeded_profiles().items[&ProfileId("cfg2".into())]
            .definition
            .clone();
        client
            .replace_definition(ProfileId("cfg2".into()), definition)
            .await
            .unwrap();

        let err = client.delete(ProfileId("ovl1".into())).await.unwrap_err();
        match err {
            ProfilesError::ProfileInUse {
                referrers,
                current,
                global_transforms,
                ..
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
        assert!(matches!(err, ProfilesError::ValidationFailed { .. }));
        drop(client);

        let reopened = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            std::sync::Arc::new(MockProfileFsPort::new()),
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("reopen after failed mutation");
        let snapshot = reopened.snapshot();
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
                ..
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
                ..
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

        // External Symlink cleanup is delegated to ProfileMaterializationPort.
        let (client, _dir) = test_client_with(MockProfileFsPort::new()).await;
        let mut profiles = Profiles::default();
        profiles.append_item(external_item(
            "sym1",
            ExternalMode::Symlink,
            external_path(std::path::Path::new(outside_a)),
        ));
        client.replace(profiles).await.unwrap();
        client.delete(ProfileId("sym1".into())).await.unwrap();

        // External Mirror cleanup is delegated to ProfileMaterializationPort.
        let (client, _dir) = test_client_with(MockProfileFsPort::new()).await;
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
            "_nyanpasu:\n  document: profiles\n  schema_revision: 4\ncurrent: ghost\nitems:\n- uid: a\n  name: A\n  type: config\n  config:\n    type: file\n    source:\n      type: local\n      binding:\n        type: managed\n        file: a.yaml\n",
        )
        .unwrap();
        let result = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            path,
            std::sync::Arc::new(MockProfileFsPort::new()),
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
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
        assert!(matches!(err, ProfilesError::ValidationFailed { .. }));
        drop(client);

        let reopened = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            std::sync::Arc::new(MockProfileFsPort::new()),
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let snapshot = reopened.snapshot();
        let item = &snapshot.items[&ProfileId("cfg1".into())];
        match &item.definition {
            ProfileDefinition::Config { config } => assert!(config.transforms().is_empty()),
            other => panic!("cfg1 must stay a config, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn replace_definition_kind_switch_rewrites_path_and_cleans_orphan() {
        let config_dir = tempdir().unwrap();
        let data_dir = tempdir().unwrap();
        let paths = crate::client::tests::test_paths(config_dir.path(), data_dir.path());
        let fs = Arc::new(ProfileFileService::new(
            paths,
            Arc::new(NoProxyPort),
            Arc::new(crate::client::tests::FixedDeviceInfoSource),
        ));
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
            fs.clone() as Arc<dyn ProfileFsPort>,
            Arc::new(MockSubscriptionFetcher::new()),
            fs.clone() as Arc<dyn ProfileMaterializationPort>,
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(file_config_item("cfg2"));
        client.replace(profiles).await.unwrap();
        let old_path = ManagedProfilePath::new("cfg2.yaml").unwrap();
        fs.write_atomic(&old_path, "proxies: []\n").unwrap();

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
        assert!(
            fs.read(&old_path).is_err(),
            "old managed path must be cleaned up"
        );
        assert_eq!(
            fs.read(&ManagedProfilePath::new("cfg2.lua").unwrap())
                .unwrap(),
            ""
        );
    }

    fn import_metadata(name: &str, custom_name: bool) -> ProfileMetadata {
        ProfileMetadata {
            name: name.into(),
            desc: None,
            custom_name,
        }
    }

    async fn wait_until_empty(client: &ProfilesClient) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if client.snapshot().items.is_empty() {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "items did not stay empty after cancelled import"
            );
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn import_happy_path_commits_complete_remote_profile() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(1).returning(|_, _| {
            Ok(FetchedSubscription {
                content: "proxies: []\n".into(),
                filename: Some("Server Title".into()),
                subscription: SubscriptionInfo {
                    upload: Some(9),
                    download: Some(3),
                    total: Some(100),
                    expire: None,
                },
                suggested_update_interval_minutes: Some(180),
            })
        });
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(fetcher),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();

        let report = client
            .import(
                url::Url::parse("https://example.com/subs/my-sub.yaml").unwrap(),
                None,
                import_metadata("url-name", false),
                RemoteProfileOptions::default(),
                false,
            )
            .await
            .expect("import");
        let uid = report.created.expect("import must return created uid");
        let item = &report.snapshot.items[&uid];
        assert_eq!(item.metadata.name, "Server Title");
        assert!(!item.metadata.custom_name);
        let ProfileSource::Remote {
            url,
            option,
            subscription,
            materialized,
        } = item.definition.source().unwrap()
        else {
            panic!("expected remote");
        };
        assert_eq!(url.as_str(), "https://example.com/subs/my-sub.yaml");
        assert_eq!(option.update_interval_minutes, 180);
        assert_eq!(subscription.upload, Some(9));
        assert_eq!(subscription.download, Some(3));
        assert!(materialized.updated_at.is_some());
        assert_eq!(materialized.file.as_str(), format!("{uid}.yaml"));
    }

    #[tokio::test]
    async fn import_fetch_failure_leaves_zero_items_without_delete_compensation() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher
            .expect_fetch()
            .times(1)
            .returning(|_, _| Err(SubscriptionFetchError::mock()));
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        // No prepare/promote/cleanup expectations: failure must not touch journals.
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(fetcher),
            Arc::new(materialization),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();

        let err = client
            .import(
                url::Url::parse("https://example.com/subs/x.yaml").unwrap(),
                None,
                import_metadata("x", true),
                RemoteProfileOptions::default(),
                false,
            )
            .await
            .expect_err("fetch failure");
        assert!(matches!(err, ProfilesError::FetchSubscription { .. }));
        assert!(client.snapshot().items.is_empty());
    }

    #[tokio::test]
    async fn import_caller_abort_during_successful_fetch_commits_nothing() {
        let dir = tempdir().unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        struct AbortThenSucceedFetcher {
            started: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
            release: std::sync::Arc<tokio::sync::Notify>,
            calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl SubscriptionFetcher for AbortThenSucceedFetcher {
            async fn fetch(
                &self,
                _url: &url::Url,
                _options: &RemoteProfileOptions,
            ) -> Result<FetchedSubscription, SubscriptionFetchError> {
                let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if call == 1 {
                    if let Some(started) = self.started.lock().unwrap().take() {
                        let _ = started.send(());
                    }
                    self.release.notified().await;
                }
                Ok(FetchedSubscription {
                    content: "proxies: []\n".into(),
                    filename: None,
                    subscription: SubscriptionInfo::default(),
                    suggested_update_interval_minutes: None,
                })
            }
        }

        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(AbortThenSucceedFetcher {
                started: std::sync::Mutex::new(Some(started_tx)),
                release: std::sync::Arc::clone(&release),
                calls: std::sync::Arc::clone(&calls),
            }),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let client_for_task = client.clone();
        let handle = tokio::spawn(async move {
            client_for_task
                .import(
                    url::Url::parse("https://example.com/subs/x.yaml").unwrap(),
                    None,
                    import_metadata("x", true),
                    RemoteProfileOptions::default(),
                    false,
                )
                .await
        });
        started_rx.await.expect("fetch started");
        handle.abort();
        let _ = handle.await;
        release.notify_waiters();
        wait_until_empty(&client).await;
        // Pending import must be cleared: a subsequent import on the same actor proceeds.
        client
            .import(
                url::Url::parse("https://example.com/subs/y.yaml").unwrap(),
                None,
                import_metadata("y", true),
                RemoteProfileOptions::default(),
                false,
            )
            .await
            .expect("second import after abort");
        assert_eq!(client.snapshot().items.len(), 1);
        assert!(calls.load(std::sync::atomic::Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn import_caller_abort_during_fetch_failure_leaves_nothing() {
        let dir = tempdir().unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let release = std::sync::Arc::new(tokio::sync::Notify::new());

        struct FailingHoldingFetcher {
            started: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
            release: std::sync::Arc<tokio::sync::Notify>,
        }
        #[async_trait::async_trait]
        impl SubscriptionFetcher for FailingHoldingFetcher {
            async fn fetch(
                &self,
                _url: &url::Url,
                _options: &RemoteProfileOptions,
            ) -> Result<FetchedSubscription, SubscriptionFetchError> {
                if let Some(started) = self.started.lock().unwrap().take() {
                    let _ = started.send(());
                }
                self.release.notified().await;
                Err(SubscriptionFetchError::mock())
            }
        }

        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(FailingHoldingFetcher {
                started: std::sync::Mutex::new(Some(started_tx)),
                release: std::sync::Arc::clone(&release),
            }),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let client_for_task = client.clone();
        let handle = tokio::spawn(async move {
            client_for_task
                .import(
                    url::Url::parse("https://example.com/subs/x.yaml").unwrap(),
                    None,
                    import_metadata("x", true),
                    RemoteProfileOptions::default(),
                    false,
                )
                .await
        });
        started_rx.await.expect("fetch started");
        handle.abort();
        let _ = handle.await;
        release.notify_waiters();
        wait_until_empty(&client).await;
    }

    #[tokio::test]
    async fn import_client_drop_while_fetch_blocked_leaves_no_durable_item() {
        let dir = tempdir().unwrap();
        let path = temp_profiles_path(&dir);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let release = std::sync::Arc::new(tokio::sync::Notify::new());
        let fetcher = HoldingFetcher {
            started: std::sync::Mutex::new(Some(started_tx)),
            release: std::sync::Arc::clone(&release),
        };
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            path.clone(),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(fetcher),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let handle = tokio::spawn(async move {
            client
                .import(
                    url::Url::parse("https://example.com/subs/x.yaml").unwrap(),
                    None,
                    import_metadata("x", true),
                    RemoteProfileOptions::default(),
                    false,
                )
                .await
        });
        started_rx.await.expect("fetch started");
        // Dropping the client (via task abort of the only clone owner after spawn
        // moved it) stops the actor; any in-flight fetch must not materialize.
        handle.abort();
        let _ = handle.await;
        release.notify_waiters();
        // Allow the aborted fetch task to finish casting against a stopped actor.
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            path,
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        assert!(client.snapshot().items.is_empty());
    }

    #[tokio::test]
    async fn import_explicit_interval_and_pinned_name_remain_authoritative() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(1).returning(|_, _| {
            Ok(FetchedSubscription {
                content: "proxies: []\n".into(),
                filename: Some("Server Title".into()),
                subscription: SubscriptionInfo::default(),
                suggested_update_interval_minutes: Some(360),
            })
        });
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(fetcher),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let option = RemoteProfileOptions {
            update_interval_minutes: 45,
            ..RemoteProfileOptions::default()
        };
        let report = client
            .import(
                url::Url::parse("https://example.com/subs/x.yaml").unwrap(),
                None,
                import_metadata("Pinned Name", true),
                option,
                true,
            )
            .await
            .expect("import");
        let uid = report.created.unwrap();
        let item = &report.snapshot.items[&uid];
        assert_eq!(item.metadata.name, "Pinned Name");
        assert!(item.metadata.custom_name);
        let ProfileSource::Remote { option, .. } = item.definition.source().unwrap() else {
            unreachable!()
        };
        assert_eq!(option.update_interval_minutes, 45);
    }

    #[tokio::test]
    async fn import_suggested_interval_applies_only_when_not_explicit() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(1).returning(|_, _| {
            Ok(FetchedSubscription {
                content: "proxies: []\n".into(),
                filename: None,
                subscription: SubscriptionInfo::default(),
                suggested_update_interval_minutes: Some(240),
            })
        });
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(fetcher),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let report = client
            .import(
                url::Url::parse("https://example.com/subs/x.yaml").unwrap(),
                None,
                import_metadata("x", true),
                RemoteProfileOptions::default(),
                false,
            )
            .await
            .expect("import");
        let uid = report.created.unwrap();
        let ProfileSource::Remote { option, .. } =
            report.snapshot.items[&uid].definition.source().unwrap()
        else {
            unreachable!()
        };
        assert_eq!(option.update_interval_minutes, 240);
    }

    #[tokio::test]
    async fn import_rejects_zero_interval_before_fetch() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(0);
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(fetcher),
            test_materialization_port(),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .unwrap();
        let option = RemoteProfileOptions {
            update_interval_minutes: 0,
            ..RemoteProfileOptions::default()
        };
        let err = client
            .import(
                url::Url::parse("https://example.com/subs/x.yaml").unwrap(),
                None,
                import_metadata("x", true),
                option,
                true,
            )
            .await
            .expect_err("zero interval");
        assert!(matches!(err, ProfilesError::ValidationFailed { .. }));
        assert!(client.snapshot().items.is_empty());
    }

    // --- T10 §2 producer gating (P1–P5) and §3 source receipts (R1–R5) ---

    use crate::{
        client::convergence::ConvergenceHealth,
        state::profiles::{
            RefreshAttemptToken, RefreshOutcome,
            sources::{SourceOrigin, SourceOutcome, SourceStatus},
        },
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A round trip through the mailbox: every message cast before it has
    /// been handled once it returns, whatever the gate answers.
    async fn round_trip(client: &ProfilesClient) {
        let result = client.refresh(ProfileId("ghost".into()), None).await;
        assert!(
            !matches!(
                result,
                Err(ProfilesError::ProfilesActorStopped | ProfilesError::ProfilesReplyDropped)
            ),
            "profiles actor must survive: {result:?}"
        );
    }

    /// Fails a test whose call parks instead of answering.
    async fn bounded<T>(call: impl std::future::Future<Output = T>) -> T {
        tokio::time::timeout(std::time::Duration::from_secs(10), call)
            .await
            .expect("call parked instead of answering")
    }

    fn yaml(profiles: &Profiles) -> String {
        serde_yaml::to_string(profiles).unwrap()
    }

    fn cast(client: &ProfilesClient, message: ProfilesActorMessage) {
        client.inner.actor_ref.cast(message).unwrap();
    }

    fn scheduled_refresh(uid: &str) -> ProfilesActorMessage {
        ProfilesActorMessage::RefreshRemote {
            uid: ProfileId(uid.into()),
            patch: None,
            origin: RefreshOrigin::Scheduled,
            reply: None,
        }
    }

    /// A remote profile refreshed just now, so starting the producers does
    /// not catch it up.
    fn fresh_remote_item(uid: &str) -> nyanpasu_config::profile::ProfileItem {
        let mut item = remote_config_item(uid);
        if let Some(source) = item.definition.source_mut() {
            source.materialized_mut().updated_at = Some(time::OffsetDateTime::now_utc());
        }
        item
    }

    async fn client_with(
        dir: &TempDir,
        fetcher: Arc<dyn SubscriptionFetcher>,
        materialization: Arc<dyn ProfileMaterializationPort>,
        items: Vec<nyanpasu_config::profile::ProfileItem>,
    ) -> ProfilesClient {
        client_with_shutdown(
            dir,
            fetcher,
            materialization,
            items,
            &CancellationToken::new(),
            &TaskTracker::new(),
        )
        .await
    }

    /// The same client, drained once `shutdown` is cancelled.
    async fn client_with_shutdown(
        dir: &TempDir,
        fetcher: Arc<dyn SubscriptionFetcher>,
        materialization: Arc<dyn ProfileMaterializationPort>,
        items: Vec<nyanpasu_config::profile::ProfileItem>,
        shutdown: &CancellationToken,
        tasks: &TaskTracker,
    ) -> ProfilesClient {
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(dir),
            Arc::new(MockProfileFsPort::new()),
            fetcher,
            materialization,
            shutdown.clone(),
            tasks,
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        for item in items {
            profiles.append_item(item);
        }
        client.replace(profiles).await.unwrap();
        client
    }

    async fn wait_for_source(
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

    async fn reach(count: &mut tokio::sync::watch::Receiver<usize>, n: usize) {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            count.wait_for(|calls| *calls >= n),
        )
        .await
        .expect("call count not reached")
        .unwrap();
    }

    /// Parks every fetch until released. Reports each start, and counts the
    /// parked fetches that were dropped instead of finishing.
    struct ParkedFetcher {
        started: tokio::sync::mpsc::UnboundedSender<()>,
        release: Arc<tokio::sync::Notify>,
        dropped: Arc<AtomicUsize>,
    }

    struct Parked(Option<Arc<AtomicUsize>>);

    impl Drop for Parked {
        fn drop(&mut self) {
            if let Some(dropped) = self.0.take() {
                dropped.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    impl ParkedFetcher {
        fn new() -> (
            Arc<Self>,
            tokio::sync::mpsc::UnboundedReceiver<()>,
            Arc<tokio::sync::Notify>,
            Arc<AtomicUsize>,
        ) {
            let (started, started_rx) = tokio::sync::mpsc::unbounded_channel();
            let release = Arc::new(tokio::sync::Notify::new());
            let dropped = Arc::new(AtomicUsize::new(0));
            (
                Arc::new(Self {
                    started,
                    release: Arc::clone(&release),
                    dropped: Arc::clone(&dropped),
                }),
                started_rx,
                release,
                dropped,
            )
        }
    }

    #[async_trait::async_trait]
    impl SubscriptionFetcher for ParkedFetcher {
        async fn fetch(
            &self,
            _url: &url::Url,
            _options: &RemoteProfileOptions,
        ) -> Result<FetchedSubscription, SubscriptionFetchError> {
            let mut parked = Parked(Some(Arc::clone(&self.dropped)));
            let _ = self.started.send(());
            self.release.notified().await;
            parked.0 = None;
            Ok(FetchedSubscription {
                content: "proxies: []\n".into(),
                filename: None,
                subscription: SubscriptionInfo::default(),
                suggested_update_interval_minutes: None,
            })
        }
    }

    /// P1: nothing runs while held; StartProducers catches the overdue
    /// profile up exactly once and arms the scheduler and the journal ticker.
    #[tokio::test(start_paused = true)]
    async fn producers_stay_held_until_started_and_catch_up_once() {
        let _clock = crate::client::jobs::explicit_test_time();
        let (fetches, mut fetched) = tokio::sync::watch::channel(0usize);
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().returning(move |_, _| {
            fetches.send_modify(|calls| *calls += 1);
            Err(SubscriptionFetchError::mock())
        });
        let (reconciles, mut reconciled) = tokio::sync::watch::channel(0usize);
        let mut materialization = MockProfileMaterializationPort::new();
        materialization.expect_reconcile().returning(move |_| {
            reconciles.send_modify(|calls| *calls += 1);
            Ok(MaterializationReconcileReport::default())
        });
        let dir = tempdir().unwrap();
        // Never refreshed, so overdue from the start.
        let client = client_with(
            &dir,
            Arc::new(fetcher),
            Arc::new(materialization),
            vec![remote_config_item("r1")],
        )
        .await;

        tokio::time::advance(std::time::Duration::from_secs(6 * 60 * 60)).await;
        cast(&client, scheduled_refresh("r1"));
        cast(&client, ProfilesActorMessage::ReconcileMaterializations);
        round_trip(&client).await;
        assert_eq!(*fetched.borrow(), 0, "held: no catch-up, tick or refresh");
        assert_eq!(*reconciled.borrow(), 1, "held: only the startup reconcile");

        client.start_producers().unwrap();
        reach(&mut fetched, 1).await;
        client.start_producers().unwrap();
        round_trip(&client).await;
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(*fetched.borrow(), 1, "a second start catches up nothing");

        crate::client::jobs::settled(&client.jobs()).await;
        tokio::time::advance(std::time::Duration::from_secs(5 * 60)).await;
        reach(&mut reconciled, 2).await;
        crate::client::jobs::settled(&client.jobs()).await;
        tokio::time::advance(std::time::Duration::from_secs(115 * 60)).await;
        reach(&mut fetched, 2).await;
    }

    /// P1: an external file event is dropped while held and applied once the
    /// producers run.
    #[tokio::test]
    async fn external_file_events_wait_for_start() {
        let fixture = ExternalFixture::new(None).await;
        std::fs::write(&fixture.target, "proxies: []\n# edited\n").unwrap();
        let before = yaml(&fixture.client.snapshot());

        cast(
            &fixture.client,
            ProfilesActorMessage::ExternalFileChanged {
                uid: ProfileId("ext1".into()),
            },
        );
        round_trip(&fixture.client).await;
        assert_eq!(yaml(&fixture.client.snapshot()), before);
        assert!(!fixture.mirrored.exists());
        assert!(fixture.client.sources().entries.is_empty());

        fixture.client.start_producers().unwrap();
        cast(
            &fixture.client,
            ProfilesActorMessage::ExternalFileChanged {
                uid: ProfileId("ext1".into()),
            },
        );
        let status = wait_for_source(&fixture.client, "ext1", |status| {
            matches!(status.outcome, SourceOutcome::Committed { .. })
        })
        .await;
        assert_eq!(status.origin, SourceOrigin::ExternalFile);
        assert!(
            std::fs::read_to_string(&fixture.mirrored)
                .unwrap()
                .contains("# edited")
        );
    }

    /// P2: held producers still admit a manual refresh and an import.
    #[tokio::test]
    async fn held_producers_admit_manual_refresh_and_import() {
        let dir = tempdir().unwrap();
        let client = client_with(
            &dir,
            Arc::new(ok_fetch("proxies: []\n")),
            test_materialization_port(),
            vec![remote_config_item("r1")],
        )
        .await;

        client
            .refresh(ProfileId("r1".into()), None)
            .await
            .expect("manual refresh while held");
        let report = client
            .import(
                url::Url::parse("https://example.com/subs/held.yaml").unwrap(),
                None,
                import_metadata("held", true),
                RemoteProfileOptions::default(),
                false,
            )
            .await
            .expect("import while held");
        assert_eq!(report.snapshot.items.len(), 2);
    }

    /// P3: the shutdown aborts every pending download and waits for it, tells
    /// its caller the application is shutting down, and a completion that
    /// reaches the actor after the cancel commits nothing even though the
    /// definition is unchanged.
    #[tokio::test]
    async fn the_shutdown_aborts_pending_downloads_and_refuses_their_late_completions() {
        let (fetcher, mut started, _release, dropped) = ParkedFetcher::new();
        let dir = tempdir().unwrap();
        let (shutdown, tasks) = (CancellationToken::new(), TaskTracker::new());
        let client = client_with_shutdown(
            &dir,
            fetcher,
            test_materialization_port(),
            vec![fresh_remote_item("r1")],
            &shutdown,
            &tasks,
        )
        .await;
        client.start_producers().unwrap();

        let refresh_client = client.clone();
        let manual =
            tokio::spawn(async move { refresh_client.refresh(ProfileId("r1".into()), None).await });
        let import_client = client.clone();
        let import = tokio::spawn(async move {
            import_client
                .import(
                    url::Url::parse("https://example.com/subs/stopped.yaml").unwrap(),
                    None,
                    import_metadata("stopped", true),
                    RemoteProfileOptions::default(),
                    false,
                )
                .await
        });
        started.recv().await.unwrap();
        started.recv().await.unwrap();
        let before = yaml(&client.snapshot());
        let definition = client.snapshot().items[&ProfileId("r1".into())]
            .definition
            .clone();

        shutdown.cancel();
        // Queued before this test yields, so ahead of the drain: only the
        // entry check keeps it from committing.
        cast(
            &client,
            ProfilesActorMessage::CommitRefreshed {
                uid: ProfileId("r1".into()),
                token: RefreshAttemptToken::nth(1),
                url: url::Url::parse("https://example.com/sub").unwrap(),
                definition_fingerprint: serde_yaml::to_string(&definition).unwrap(),
                outcome: RefreshOutcome::Succeeded {
                    subscription: SubscriptionInfo::default(),
                    suggested_update_interval_minutes: None,
                    content: "proxies: []\n".into(),
                    filename: None,
                },
            },
        );
        tasks.close();
        bounded(tasks.wait()).await;

        assert_eq!(dropped.load(Ordering::SeqCst), 2, "both downloads aborted");
        assert!(matches!(
            bounded(manual).await.unwrap(),
            Err(ProfilesError::ShuttingDown)
        ));
        assert!(matches!(
            bounded(import).await.unwrap(),
            Err(ProfilesError::ShuttingDown)
        ));
        assert_eq!(yaml(&client.snapshot()), before);
        assert!(client.sources().entries.is_empty());
    }

    /// P4: once the shutdown began, manual work and writes are refused and
    /// background input is dropped; a start arms nothing.
    #[tokio::test(start_paused = true)]
    async fn after_the_shutdown_began_no_write_or_producer_runs() {
        let _clock = crate::client::jobs::explicit_test_time();
        let (fetches, fetched) = tokio::sync::watch::channel(0usize);
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().returning(move |_, _| {
            fetches.send_modify(|calls| *calls += 1);
            Err(SubscriptionFetchError::mock())
        });
        let dir = tempdir().unwrap();
        // Only an external event that got through would read the Mirror
        // target; the read is counted and refused.
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&reads);
        let mut fs = MockProfileFsPort::new();
        fs.expect_read_external().returning(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Err(ProfileFileError::mock(
                "a stopped actor reads no external file",
            ))
        });
        let (shutdown, tasks) = (CancellationToken::new(), TaskTracker::new());
        let client = ProfilesClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(&dir),
            Arc::new(fs),
            Arc::new(fetcher),
            test_materialization_port(),
            shutdown.clone(),
            &tasks,
        )
        .await
        .unwrap();
        let mut profiles = Profiles::default();
        profiles.append_item(remote_config_item("r1"));
        profiles.append_item(external_item(
            "ext1",
            ExternalMode::Mirror,
            external_path(&dir.path().join("external.yaml")),
        ));
        client.replace(profiles).await.unwrap();
        let before = yaml(&client.snapshot());

        shutdown.cancel();
        // All of it is queued before this test yields, so ahead of the drain:
        // the entry check is what refuses it.
        client.start_producers().unwrap();
        cast(&client, scheduled_refresh("r1"));
        cast(
            &client,
            ProfilesActorMessage::ExternalFileChanged {
                uid: ProfileId("ext1".into()),
            },
        );
        let (refresh, import, replace) = tokio::join!(
            client.refresh(ProfileId("r1".into()), None),
            client.import(
                url::Url::parse("https://example.com/subs/late.yaml").unwrap(),
                None,
                import_metadata("late", true),
                RemoteProfileOptions::default(),
                false,
            ),
            client.replace(Profiles::default()),
        );
        assert!(matches!(refresh, Err(ProfilesError::ShuttingDown)));
        assert!(matches!(import, Err(ProfilesError::ShuttingDown)));
        assert!(matches!(replace, Err(ProfilesError::ShuttingDown)));
        tasks.close();
        bounded(tasks.wait()).await;

        tokio::time::advance(std::time::Duration::from_secs(6 * 60 * 60)).await;
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            reads.load(Ordering::SeqCst),
            0,
            "stopped: the external event is dropped"
        );
        assert_eq!(
            *fetched.borrow(),
            0,
            "stopped: no catch-up, tick or refresh"
        );
        assert_eq!(
            yaml(&client.snapshot()),
            before,
            "no write after the cancel"
        );
        assert!(client.sources().entries.is_empty());
    }

    /// P5: a completion carrying another attempt's token commits nothing and
    /// leaves the pending attempt in place; the attempt's own completion
    /// still settles it.
    #[tokio::test]
    async fn refresh_completion_must_carry_the_pending_attempt_token() {
        let (fetcher, mut started, release, _dropped) = ParkedFetcher::new();
        let dir = tempdir().unwrap();
        let client = client_with(
            &dir,
            fetcher,
            test_materialization_port(),
            vec![fresh_remote_item("r1")],
        )
        .await;
        let refresh_client = client.clone();
        let pending =
            tokio::spawn(async move { refresh_client.refresh(ProfileId("r1".into()), None).await });
        started.recv().await.unwrap();
        let before = yaml(&client.snapshot());

        let definition = client.snapshot().items[&ProfileId("r1".into())]
            .definition
            .clone();
        cast(
            &client,
            ProfilesActorMessage::CommitRefreshed {
                uid: ProfileId("r1".into()),
                token: RefreshAttemptToken::nth(2),
                url: url::Url::parse("https://example.com/sub").unwrap(),
                definition_fingerprint: serde_yaml::to_string(&definition).unwrap(),
                outcome: RefreshOutcome::Succeeded {
                    subscription: SubscriptionInfo {
                        upload: Some(42),
                        ..Default::default()
                    },
                    suggested_update_interval_minutes: None,
                    content: "proxies: []\n".into(),
                    filename: None,
                },
            },
        );
        assert!(matches!(
            bounded(client.refresh(ProfileId("r1".into()), None)).await,
            Err(ProfilesError::RefreshInProgress { .. })
        ));
        assert_eq!(yaml(&client.snapshot()), before);
        assert!(client.sources().entries.is_empty());

        release.notify_waiters();
        let report = bounded(pending)
            .await
            .unwrap()
            .expect("its own completion commits");
        let ProfileSource::Remote { subscription, .. } = report.snapshot.items
            [&ProfileId("r1".into())]
            .definition
            .source()
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(subscription.upload, None, "the foreign payload was dropped");
    }

    /// R1: a committed manual refresh and a failed scheduled one each leave
    /// their receipt.
    #[tokio::test]
    async fn refresh_receipts_record_committed_and_failed_downloads() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut fetcher = MockSubscriptionFetcher::new();
        let counter = Arc::clone(&calls);
        fetcher.expect_fetch().returning(move |_, _| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(FetchedSubscription {
                    content: "proxies: []\n".into(),
                    filename: None,
                    subscription: SubscriptionInfo::default(),
                    suggested_update_interval_minutes: None,
                })
            } else {
                Err(SubscriptionFetchError::mock())
            }
        });
        let dir = tempdir().unwrap();
        let client = client_with(
            &dir,
            Arc::new(fetcher),
            test_materialization_port(),
            vec![fresh_remote_item("r1")],
        )
        .await;

        client.refresh(ProfileId("r1".into()), None).await.unwrap();
        let committed = client.sources().entries[0].clone();
        assert_eq!(committed.origin, SourceOrigin::ManualRefresh);
        // Nothing runs `r1`, so its refresh is a plain save with no operation.
        assert!(matches!(
            committed.outcome,
            SourceOutcome::Committed { operation_id: None }
        ));
        assert_eq!(committed.health, ConvergenceHealth::Healthy);

        client.start_producers().unwrap();
        cast(&client, scheduled_refresh("r1"));
        let failed = wait_for_source(&client, "r1", |status| {
            status.origin == SourceOrigin::ScheduledRefresh
        })
        .await;
        assert!(matches!(
            &failed.outcome,
            SourceOutcome::Failed { message } if message.contains("HTTP 500")
        ));
        assert_eq!(failed.health, ConvergenceHealth::Blocked);
        assert_eq!(client.sources().entries.len(), 1, "one row per profile");
    }

    /// R2: a download whose definition changed meanwhile ends as
    /// Superseded (V31), which is healthy.
    #[tokio::test]
    async fn a_download_fenced_out_by_a_changed_definition_is_superseded() {
        let (fetcher, mut started, release, _dropped) = ParkedFetcher::new();
        let dir = tempdir().unwrap();
        let client = client_with(
            &dir,
            fetcher,
            test_materialization_port(),
            vec![fresh_remote_item("r1")],
        )
        .await;
        let refresh_client = client.clone();
        let pending =
            tokio::spawn(async move { refresh_client.refresh(ProfileId("r1".into()), None).await });
        started.recv().await.unwrap();

        let mut patch = RemoteProfileOptions::new_empty_patch();
        patch.update_interval_minutes = Some(60);
        client
            .patch_remote_options(ProfileId("r1".into()), patch)
            .await
            .unwrap();
        release.notify_waiters();
        assert!(matches!(
            bounded(pending).await.unwrap(),
            Err(ProfilesError::ProfileChangedDuringRefresh { .. })
        ));
        let stale = client.sources().entries[0].clone();
        assert_eq!(stale.origin, SourceOrigin::ManualRefresh);
        assert!(matches!(
            stale.outcome,
            SourceOutcome::Superseded { reason } if reason.contains("changed")
        ));
        assert_eq!(stale.health, ConvergenceHealth::Healthy);
    }

    /// R2: so is a download that fails after its definition changed: the
    /// failure belongs to a definition that no longer exists.
    #[tokio::test]
    async fn a_failed_download_fenced_out_by_a_changed_definition_is_superseded() {
        /// Parks every fetch until released, then fails it.
        struct ParkThenFail {
            started: tokio::sync::mpsc::UnboundedSender<()>,
            release: Arc<tokio::sync::Notify>,
        }
        #[async_trait::async_trait]
        impl SubscriptionFetcher for ParkThenFail {
            async fn fetch(
                &self,
                _url: &url::Url,
                _options: &RemoteProfileOptions,
            ) -> Result<FetchedSubscription, SubscriptionFetchError> {
                let _ = self.started.send(());
                self.release.notified().await;
                Err(SubscriptionFetchError::mock())
            }
        }
        let (started, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let release = Arc::new(tokio::sync::Notify::new());
        let dir = tempdir().unwrap();
        let client = client_with(
            &dir,
            Arc::new(ParkThenFail {
                started,
                release: Arc::clone(&release),
            }),
            test_materialization_port(),
            vec![fresh_remote_item("r1")],
        )
        .await;
        let refresh_client = client.clone();
        let pending =
            tokio::spawn(async move { refresh_client.refresh(ProfileId("r1".into()), None).await });
        started_rx.recv().await.unwrap();

        let mut patch = RemoteProfileOptions::new_empty_patch();
        patch.update_interval_minutes = Some(60);
        client
            .patch_remote_options(ProfileId("r1".into()), patch)
            .await
            .unwrap();
        release.notify_waiters();
        assert!(matches!(
            bounded(pending).await.unwrap(),
            Err(ProfilesError::ProfileChangedDuringRefresh { .. })
        ));
        let stale = client.sources().entries[0].clone();
        assert_eq!(stale.origin, SourceOrigin::ManualRefresh);
        assert!(matches!(
            stale.outcome,
            SourceOutcome::Superseded { reason } if reason.contains("changed")
        ));
        assert_eq!(stale.health, ConvergenceHealth::Healthy);
    }

    /// R2 (ruling R22): a tick that lands on a refresh in flight writes no
    /// receipt, so the last Failed row stands until that refresh concludes.
    #[tokio::test]
    async fn a_tick_folded_into_a_refresh_in_flight_keeps_the_last_receipt() {
        /// Fails the first fetch and parks every later one until released.
        struct FailThenPark {
            calls: AtomicUsize,
            started: tokio::sync::mpsc::UnboundedSender<()>,
            release: Arc<tokio::sync::Notify>,
        }
        #[async_trait::async_trait]
        impl SubscriptionFetcher for FailThenPark {
            async fn fetch(
                &self,
                _url: &url::Url,
                _options: &RemoteProfileOptions,
            ) -> Result<FetchedSubscription, SubscriptionFetchError> {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(SubscriptionFetchError::mock());
                }
                let _ = self.started.send(());
                self.release.notified().await;
                Ok(FetchedSubscription {
                    content: "proxies: []\n".into(),
                    filename: None,
                    subscription: SubscriptionInfo::default(),
                    suggested_update_interval_minutes: None,
                })
            }
        }
        let (started, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let release = Arc::new(tokio::sync::Notify::new());
        let dir = tempdir().unwrap();
        let client = client_with(
            &dir,
            Arc::new(FailThenPark {
                calls: AtomicUsize::new(0),
                started,
                release: Arc::clone(&release),
            }),
            test_materialization_port(),
            vec![fresh_remote_item("r1")],
        )
        .await;
        client.start_producers().unwrap();

        cast(&client, scheduled_refresh("r1"));
        let failed = wait_for_source(&client, "r1", |status| {
            matches!(status.outcome, SourceOutcome::Failed { .. })
        })
        .await;
        let failed_seq = client.sources().event_seq;
        cast(&client, scheduled_refresh("r1"));
        started_rx.recv().await.unwrap();
        cast(&client, scheduled_refresh("r1"));
        round_trip(&client).await;
        let sources = client.sources();
        assert_eq!(
            sources.event_seq, failed_seq,
            "the folded tick published nothing"
        );
        assert_eq!(sources.entries, vec![failed]);

        release.notify_waiters();
        let committed = wait_for_source(&client, "r1", |status| {
            matches!(status.outcome, SourceOutcome::Committed { .. })
        })
        .await;
        assert_eq!(committed.origin, SourceOrigin::ScheduledRefresh);
    }

    /// R3: a download the source transaction refuses is Rejected, not
    /// Failed, and commits nothing.
    #[tokio::test]
    async fn refresh_refused_by_the_source_transaction_is_rejected() {
        let dir = tempdir().unwrap();
        let client = client_with(
            &dir,
            Arc::new(ok_fetch("proxies: []\n")),
            failing_state_promote_materialization_port(),
            vec![remote_config_item("r1")],
        )
        .await;
        let before = yaml(&client.snapshot());

        assert!(matches!(
            client.refresh(ProfileId("r1".into()), None).await,
            Err(ProfilesError::Commit {
                source: CommitAborted::WriteConfig { .. },
                ..
            })
        ));
        let status = client.sources().entries[0].clone();
        assert!(matches!(
            &status.outcome,
            SourceOutcome::Rejected { code, .. } if code == SourceOutcome::SUBSCRIPTION_REJECTED
        ));
        assert_eq!(status.health, ConvergenceHealth::Blocked);
        assert_eq!(yaml(&client.snapshot()), before);
    }

    /// A Mirror profile over a real file service. The watcher is live once
    /// the producers start, so a test writes the target only while the
    /// outcome of every event is the same.
    struct ExternalFixture {
        client: ProfilesClient,
        target: std::path::PathBuf,
        mirrored: std::path::PathBuf,
        _dirs: [TempDir; 3],
    }

    impl ExternalFixture {
        async fn new(materialization: Option<Arc<dyn ProfileMaterializationPort>>) -> Self {
            let config_dir = tempdir().unwrap();
            let data_dir = tempdir().unwrap();
            let target_dir = tempdir().unwrap();
            let target = target_dir.path().join("external.yaml");
            std::fs::write(&target, "proxies: []\n").unwrap();
            let fs = Arc::new(ProfileFileService::new(
                crate::client::tests::test_paths(config_dir.path(), data_dir.path()),
                Arc::new(NoProxyPort),
                Arc::new(crate::client::tests::FixedDeviceInfoSource),
            ));
            let client = ProfilesClient::new(
                crate::state::mutation::MutationCoordinator::isolated(),
                Utf8PathBuf::from_path_buf(config_dir.path().join("profiles.yaml")).unwrap(),
                fs.clone() as Arc<dyn ProfileFsPort>,
                Arc::new(MockSubscriptionFetcher::new()),
                materialization.unwrap_or(fs as Arc<dyn ProfileMaterializationPort>),
                tokio_util::sync::CancellationToken::new(),
                &tokio_util::task::TaskTracker::new(),
            )
            .await
            .unwrap();
            let mut profiles = Profiles::default();
            profiles.append_item(external_item(
                "ext1",
                ExternalMode::Mirror,
                external_path(&target),
            ));
            client.replace(profiles).await.unwrap();
            Self {
                client,
                target,
                mirrored: config_dir.path().join("profiles").join("ext1.yaml"),
                _dirs: [config_dir, data_dir, target_dir],
            }
        }
    }

    /// Counts the commits that staged content and refuses to promote them,
    /// so a test can tell content refused before the source transaction from
    /// content the transaction itself refused.
    fn refusing_materialization_port(
        staged: Arc<AtomicUsize>,
    ) -> Arc<dyn ProfileMaterializationPort> {
        let mut materialization = MockProfileMaterializationPort::new();
        materialization
            .expect_reconcile()
            .returning(|_| Ok(MaterializationReconcileReport::default()));
        materialization
            .expect_prepare_file_first()
            .returning(move |_, _, _| {
                staged.fetch_add(1, Ordering::SeqCst);
                Ok(PreparedMaterialization::new("file".into()))
            });
        materialization
            .expect_promote()
            .returning(|_| Err(ProfileFileError::mock("disk full")));
        materialization.expect_compensate().returning(|_| Ok(()));
        Arc::new(materialization)
    }

    fn rejected_external(status: &SourceStatus) -> bool {
        matches!(
            &status.outcome,
            SourceOutcome::Rejected { code, .. } if code == SourceOutcome::EXTERNAL_SOURCE_REJECTED
        )
    }

    /// R4 (V33): Mirror content that fails validation is Rejected before it
    /// reaches the source transaction; the external file keeps what the user
    /// wrote and nothing is applied.
    #[tokio::test]
    async fn invalid_external_content_is_rejected_and_left_untouched() {
        let staged = Arc::new(AtomicUsize::new(0));
        let fixture =
            ExternalFixture::new(Some(refusing_materialization_port(Arc::clone(&staged)))).await;
        let before = yaml(&fixture.client.snapshot());
        fixture.client.start_producers().unwrap();

        std::fs::write(&fixture.target, "mode: rule\n").unwrap();
        cast(
            &fixture.client,
            ProfilesActorMessage::ExternalFileChanged {
                uid: ProfileId("ext1".into()),
            },
        );
        let status = wait_for_source(&fixture.client, "ext1", rejected_external).await;
        assert!(matches!(
            &status.outcome,
            SourceOutcome::Rejected { message, .. } if message.contains("proxies")
        ));
        assert_eq!(status.health, ConvergenceHealth::Blocked);
        assert_eq!(
            staged.load(Ordering::SeqCst),
            0,
            "invalid content never reaches the source transaction"
        );
        assert_eq!(
            std::fs::read_to_string(&fixture.target).unwrap(),
            "mode: rule\n"
        );
        assert_eq!(yaml(&fixture.client.snapshot()), before);
    }

    /// R5 (V33): valid external content that the source transaction refuses
    /// is Rejected the same way, with the external file untouched.
    #[tokio::test]
    async fn external_content_refused_by_the_source_transaction_is_rejected() {
        let staged = Arc::new(AtomicUsize::new(0));
        let fixture =
            ExternalFixture::new(Some(refusing_materialization_port(Arc::clone(&staged)))).await;
        let before = yaml(&fixture.client.snapshot());
        fixture.client.start_producers().unwrap();

        std::fs::write(&fixture.target, "proxies: []\n# edited\n").unwrap();
        cast(
            &fixture.client,
            ProfilesActorMessage::ExternalFileChanged {
                uid: ProfileId("ext1".into()),
            },
        );
        let status = wait_for_source(&fixture.client, "ext1", rejected_external).await;
        assert_eq!(status.health, ConvergenceHealth::Blocked);
        assert!(
            staged.load(Ordering::SeqCst) >= 1,
            "valid content reaches the source transaction before it is refused"
        );
        assert_eq!(
            std::fs::read_to_string(&fixture.target).unwrap(),
            "proxies: []\n# edited\n"
        );
        assert_eq!(yaml(&fixture.client.snapshot()), before);
    }
    async fn sync_fixture(
        directory: &TempDir,
        fetcher: Arc<dyn SubscriptionFetcher>,
        capture: nyanpasu_jobs::LogCapture,
        shutdown: &CancellationToken,
        tasks: &TaskTracker,
    ) -> ProfilesClient {
        let jobs = crate::client::jobs::start(
            directory.path().join("sync-jobs.redb"),
            capture,
            shutdown.clone(),
            tasks,
        )
        .await
        .unwrap();
        ProfilesClient::new_with_jobs(
            crate::state::mutation::MutationCoordinator::isolated(),
            temp_profiles_path(directory),
            jobs,
            Arc::new(MockProfileFsPort::new()),
            fetcher,
            test_materialization_port(),
            shutdown.clone(),
            tasks,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn manual_sync_persists_commit_and_scoped_safe_logs() {
        use tracing::instrument::WithSubscriber;
        use tracing_subscriber::{Layer as _, layer::SubscriberExt};
        // Keep two dispatchers alive so tracing does not cache a callsite first
        // visited by a parallel test against that thread's NoSubscriber.
        let _parallel_dispatch =
            tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
        let capture = crate::client::jobs::capture();
        let subscriber = tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::sink)
                    .with_filter(tracing_subscriber::filter::LevelFilter::WARN),
            )
            .with(capture.layer());
        async {
            let dir = tempdir().unwrap();
            let (shutdown, tasks) = (CancellationToken::new(), TaskTracker::new());
            let client = sync_fixture(
                &dir,
                Arc::new(ok_fetch("proxies: []\n")),
                capture,
                &shutdown,
                &tasks,
            )
            .await;
            let mut profiles = Profiles::default();
            let mut item = remote_config_item("r1");
            if let Some(ProfileSource::Remote { url, .. }) = item.definition.source_mut() {
                *url = url::Url::parse("https://example.com/sub?token=private-token").unwrap();
            }
            profiles.append_item(item);
            client.replace(profiles).await.unwrap();
            let uid = ProfileId("r1".into());
            let report = client.sync(uid.clone(), None).await.unwrap();
            assert!(
                report.snapshot.items[&uid]
                    .definition
                    .source()
                    .unwrap()
                    .materialized()
                    .updated_at
                    .is_some()
            );
            let runs = client
                .jobs()
                .runs(crate::state::profiles::jobs::sync_key(&uid), None, 10)
                .await
                .unwrap();
            assert_eq!(runs.items.len(), 1);
            let run = &runs.items[0];
            assert_eq!(run.trigger, nyanpasu_jobs::Trigger::Manual);
            assert_eq!(
                run.completion().unwrap().outcome,
                nyanpasu_jobs::Outcome::Succeeded
            );
            let logs = client.jobs().logs(run.id, 0, 100).await.unwrap();
            let stages: Vec<_> = logs
                .items
                .iter()
                .filter_map(|entry| {
                    entry
                        .fields
                        .get("stage")
                        .map(|value| serde_json::from_str::<String>(value).unwrap())
                })
                .collect();
            assert_eq!(stages, ["started", "download", "validated", "committed"]);
            let serialized = serde_json::to_string(&logs.items).unwrap();
            assert!(!serialized.contains("private-token"));
            assert!(!serialized.contains("example.com"));
            shutdown.cancel();
            tasks.close();
            tasks.wait().await;
        }
        .with_subscriber(subscriber)
        .await;
    }

    #[tokio::test]
    async fn failed_sync_retains_ten_runs_and_recovers_history_after_restart() {
        let dir = tempdir().unwrap();
        let (shutdown, tasks) = (CancellationToken::new(), TaskTracker::new());
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher
            .expect_fetch()
            .returning(|_, _| Err(SubscriptionFetchError::SubscriptionHttpStatus { status: 403 }));
        let client = sync_fixture(
            &dir,
            Arc::new(fetcher),
            crate::client::jobs::capture(),
            &shutdown,
            &tasks,
        )
        .await;
        let mut profiles = Profiles::default();
        profiles.append_item(remote_config_item("r1"));
        client.replace(profiles).await.unwrap();
        let uid = ProfileId("r1".into());
        for _ in 0..12 {
            assert!(matches!(
                client.sync(uid.clone(), None).await,
                Err(crate::client::ClientError::Profiles(
                    ProfilesError::FetchSubscription { .. }
                ))
            ));
        }
        crate::client::jobs::settled(&client.jobs()).await;
        let key = crate::state::profiles::jobs::sync_key(&uid);
        let first = client.jobs().runs(key.clone(), None, 4).await.unwrap();
        let second = client
            .jobs()
            .runs(key.clone(), first.next, 10)
            .await
            .unwrap();
        assert_eq!(first.items.len(), 4);
        assert_eq!(second.items.len(), 6);
        assert!(first.items.iter().chain(&second.items).all(|run| matches!(&run.completion().unwrap().outcome, nyanpasu_jobs::Outcome::Failed(error) if error.code == "subscription_http_status")));
        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
        let (shutdown, tasks) = (CancellationToken::new(), TaskTracker::new());
        let reloaded = sync_fixture(
            &dir,
            Arc::new(MockSubscriptionFetcher::new()),
            crate::client::jobs::capture(),
            &shutdown,
            &tasks,
        )
        .await;
        assert_eq!(
            reloaded
                .jobs()
                .runs(key, None, 20)
                .await
                .unwrap()
                .items
                .len(),
            10
        );
        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
    }

    #[tokio::test]
    async fn dropping_sync_waiter_keeps_the_download_owned_and_rejects_overlap() {
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let release = Arc::new(tokio::sync::Notify::new());
        let fetcher = HoldingFetcher {
            started: std::sync::Mutex::new(Some(started)),
            release: release.clone(),
        };
        let (client, _dir) =
            remote_seeded_client_with_fetcher(MockProfileFsPort::new(), Arc::new(fetcher)).await;
        let uid = ProfileId("r1".into());
        let caller = tokio::spawn({
            let client = client.clone();
            let uid = uid.clone();
            async move { client.sync(uid, None).await }
        });
        started_rx.await.unwrap();
        caller.abort();
        let _ = caller.await;
        assert!(matches!(
            client.sync(uid.clone(), None).await,
            Err(crate::client::ClientError::Jobs(nyanpasu_jobs::Error::Busy))
        ));
        release.notify_waiters();
        wait_for_source(&client, "r1", |source| {
            matches!(source.outcome, SourceOutcome::Committed { .. })
        })
        .await;
        crate::client::jobs::settled(&client.jobs()).await;
        let runs = client
            .jobs()
            .runs(crate::state::profiles::jobs::sync_key(&uid), None, 10)
            .await
            .unwrap();
        assert_eq!(runs.items.len(), 1);
        assert_eq!(
            runs.items[0].completion().unwrap().outcome,
            nyanpasu_jobs::Outcome::Succeeded
        );
    }
}
