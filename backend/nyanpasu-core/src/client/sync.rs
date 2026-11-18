//! Configuration sync as the facade exposes it.
//!
//! The owner lives in [`crate::sync`]; this module supplies the production
//! adapters that read and write through the existing configuration owners and
//! the files the resolver names, and the facade methods RPC calls.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, sync::PortableDocument,
};
use nyanpasu_paths::{LogicalPath, PathResolver};
use nyanpasu_runtime_values::sync::{ConflictChoice, SyncConflict, SyncStatus};
use nyanpasu_state_engine::Version;
use snafu::{ResultExt as _, Snafu};

use crate::{
    client::{
        application::ApplicationClient, clash_config::ClashConfigClient, profiles::ProfilesClient,
    },
    state::application::ApplicationSnapshot,
    sync::{
        self, SyncFailure,
        client::SyncClient,
        engine::CycleReport,
        ports::{
            ApplyError, ApplyOutcome, CommittedDocument, CommittedProfiles, PortableStatePort,
            SnapshotSummary, SyncFilePort, SyncPorts, SyncSnapshotPort, SyncStatusSink,
        },
        settings::{FileSyncSettingsStore, SyncSettingsStore},
        state::{RedbSyncStateStore, SyncStateStore},
    },
};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("failed to start the sync owner"))]
    Start { source: sync::client::Error },
    #[snafu(display("failed to read the sync status"))]
    Status { source: sync::client::Error },
    #[snafu(display("failed to read the sync settings"))]
    Settings { source: sync::client::Error },
    #[snafu(display("failed to read the open conflicts"))]
    Conflicts { source: sync::client::Error },
}

/// The adapters a shell injects to enable sync.
pub struct SyncAdapters {
    pub remote: Arc<dyn nyanpasu_contracts::sync::RemoteStoreFactory>,
    pub secrets: Arc<dyn nyanpasu_contracts::sync::SecretStore>,
}

/// Publishes status to whoever subscribes, such as the RPC event bridge.
#[derive(Debug, Clone)]
pub struct SyncStatusHub {
    sender: tokio::sync::watch::Sender<SyncStatus>,
}

impl Default for SyncStatusHub {
    fn default() -> Self {
        let (sender, _) = tokio::sync::watch::channel(SyncStatus::default());
        Self { sender }
    }
}

impl SyncStatusHub {
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<SyncStatus> {
        self.sender.subscribe()
    }

    #[must_use]
    pub fn current(&self) -> SyncStatus {
        self.sender.borrow().clone()
    }
}

impl SyncStatusSink for SyncStatusHub {
    fn publish(&self, status: &SyncStatus) {
        self.sender.send_replace(status.clone());
    }
}

/// Reads and writes the three documents through their owners.
pub struct OwnerStatePort {
    pub(crate) application: ApplicationClient,
    pub(crate) clash: ClashConfigClient,
    pub(crate) profiles: ProfilesClient,
    pub(crate) paths: PathResolver,
}

#[async_trait::async_trait]
impl PortableStatePort for OwnerStatePort {
    async fn application(&self) -> Result<CommittedDocument<NyanpasuAppConfig>, ApplyError> {
        let snapshot: ApplicationSnapshot = self.application.snapshot();
        Ok(CommittedDocument {
            state: snapshot.state,
            version: snapshot.version,
        })
    }

    async fn clash(&self) -> Result<CommittedDocument<ClashConfig>, ApplyError> {
        let snapshot = self.clash.snapshot();
        Ok(CommittedDocument {
            state: snapshot.state,
            version: snapshot.version,
        })
    }

    /// Reads the catalog, its version and its portable files under the content
    /// read guard, so a file promoted by an in-flight write cannot be paired
    /// with the catalog that preceded it.
    async fn profiles(&self) -> Result<CommittedProfiles, ApplyError> {
        let reader = self.profiles.content_reader();
        let paths = self.paths.clone();
        let capture = reader
            .committed(move |versioned| async move {
                let state = versioned.state.clone();
                let version = versioned.version;
                let document =
                    std::fs::read(paths.profiles_path().as_std_path()).unwrap_or_default();
                let mut files = std::collections::BTreeMap::new();
                for file in nyanpasu_config::sync::portable_profile_files(&state) {
                    let path = paths.app_profiles_dir().join(&file);
                    if let Ok(bytes) = std::fs::read(path.as_std_path()) {
                        files.insert(file, bytes);
                    }
                }
                (state, version, files, document)
            })
            .await;
        let (state, version, files, document) = capture;
        Ok(CommittedProfiles {
            state,
            version,
            files,
            document,
        })
    }

    async fn apply_application(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<ApplyOutcome, ApplyError> {
        let current = self.application.snapshot();
        if current.version != expected {
            return Ok(ApplyOutcome::VersionConflict);
        }
        let next =
            nyanpasu_config::sync::apply_application(&current.state, merged).map_err(|error| {
                ApplyError::Rejected {
                    item: "app/application".to_owned(),
                    reason: error.to_string(),
                }
            })?;
        Ok(match self.application.replace(next).await {
            Ok(_) => ApplyOutcome::Committed { deferred: false },
            Err(error) => outcome_from_config(&error),
        })
    }

    async fn apply_clash(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<ApplyOutcome, ApplyError> {
        let current = self.clash.snapshot();
        if current.version != expected {
            return Ok(ApplyOutcome::VersionConflict);
        }
        let next = nyanpasu_config::sync::apply_clash(&current.state, merged).map_err(|error| {
            ApplyError::Rejected {
                item: "app/clash".to_owned(),
                reason: error.to_string(),
            }
        })?;
        Ok(match self.clash.replace(next).await {
            Ok(_) => ApplyOutcome::Committed { deferred: false },
            Err(error) => outcome_from_config(&error),
        })
    }

    async fn apply_profiles(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<ApplyOutcome, ApplyError> {
        if self.profiles.version() != expected {
            return Ok(ApplyOutcome::VersionConflict);
        }
        let current = self.profiles.snapshot();
        let next = nyanpasu_config::sync::apply_profiles(&current, merged).map_err(|error| {
            ApplyError::Rejected {
                item: "app/profiles".to_owned(),
                reason: error.to_string(),
            }
        })?;
        Ok(match self.profiles.replace(next).await {
            Ok(report) => {
                // A committed catalog whose runtime application is deferred still
                // saved the source, so the baseline may advance; the deferral is
                // reported as a degradation, not as a failed commit.
                let deferred = report.runtime_degradations.iter().any(|degradation| {
                    matches!(
                        degradation.reason,
                        nyanpasu_runtime_values::runtime::DegradationReason::RuntimeDeferred { .. }
                    )
                });
                ApplyOutcome::Committed { deferred }
            }
            Err(error) => outcome_from_profiles(&error),
        })
    }

    async fn active_profile(&self) -> Option<String> {
        self.profiles
            .snapshot()
            .current
            .as_ref()
            .map(|id| id.0.clone())
    }
}

/// Reads and writes profile files and tray icons at the resolver's paths.
pub struct ResolverFilePort {
    paths: PathResolver,
}

impl ResolverFilePort {
    #[must_use]
    pub fn new(paths: PathResolver) -> Self {
        Self { paths }
    }

    fn physical(&self, logical: &LogicalPath) -> Result<PathBuf, ApplyError> {
        let mut segments = logical.segments();
        let root = match segments.next() {
            Some("profiles") => self.paths.app_profiles_dir(),
            Some("icons") => self.paths.app_config_dir().join("icons"),
            _ => {
                return Err(ApplyError::File {
                    path: logical.to_string(),
                    reason: "not a file item".to_owned(),
                });
            }
        };
        let mut path = root.into_std_path_buf();
        for segment in segments {
            path.push(segment);
        }
        Ok(path)
    }
}

impl SyncFilePort for ResolverFilePort {
    fn read_text(&self, logical: &LogicalPath) -> Result<Option<Vec<u8>>, ApplyError> {
        read_bytes(&self.physical(logical)?)
    }

    fn write_text(&self, logical: &LogicalPath, bytes: &[u8]) -> Result<(), ApplyError> {
        write_bytes(&self.physical(logical)?, bytes)
    }

    fn remove_text(&self, logical: &LogicalPath) -> Result<(), ApplyError> {
        remove_file(&self.physical(logical)?)
    }

    fn read_binary(&self, logical: &LogicalPath) -> Result<Option<Vec<u8>>, ApplyError> {
        read_bytes(&self.physical(logical)?)
    }

    fn write_binary(&self, logical: &LogicalPath, bytes: &[u8]) -> Result<(), ApplyError> {
        write_bytes(&self.physical(logical)?, bytes)
    }

    fn remove_binary(&self, logical: &LogicalPath) -> Result<(), ApplyError> {
        remove_file(&self.physical(logical)?)
    }

    fn read_document(&self, logical: &LogicalPath) -> Result<Option<Vec<u8>>, ApplyError> {
        let path = match logical.as_str() {
            "app/application" => self.paths.application_config_path(),
            "app/clash" => self.paths.clash_config_path(),
            "app/profiles" => self.paths.profiles_path(),
            other => {
                return Err(ApplyError::File {
                    path: other.to_owned(),
                    reason: "not a configuration document".to_owned(),
                });
            }
        };
        read_bytes(path.as_std_path())
    }
}

/// A committed-versus-aborted outcome from an application or Clash owner.
fn outcome_from_config(error: &crate::state::config_error::ConfigError) -> ApplyOutcome {
    use crate::state::config_error::ConfigError;
    match error {
        ConfigError::VersionConflict { .. } => ApplyOutcome::VersionConflict,
        ConfigError::Commit { source, .. } => outcome_from_aborted(source),
        other => ApplyOutcome::Unavailable {
            reason: other.to_string(),
        },
    }
}

/// A committed-versus-aborted outcome from the profiles owner.
fn outcome_from_profiles(error: &nyanpasu_runtime_values::profiles::ProfilesError) -> ApplyOutcome {
    use nyanpasu_runtime_values::profiles::ProfilesError;
    match error {
        ProfilesError::VersionConflict { .. } => ApplyOutcome::VersionConflict,
        ProfilesError::Commit { source, .. } => outcome_from_aborted(source),
        other => ApplyOutcome::Unavailable {
            reason: other.to_string(),
        },
    }
}

/// A source that refused a candidate is not retried unchanged; one that could
/// not be checked is transient.
fn outcome_from_aborted(
    aborted: &nyanpasu_runtime_values::mutation::CommitAborted,
) -> ApplyOutcome {
    use nyanpasu_runtime_values::mutation::CommitAborted;
    match aborted {
        CommitAborted::RuntimeRefused { .. } => ApplyOutcome::Refused {
            reason: aborted.to_string(),
        },
        CommitAborted::RuntimeFailed { .. }
        | CommitAborted::WriteConfig { .. }
        | CommitAborted::RecoverAfterWriteFailure { .. } => ApplyOutcome::Unavailable {
            reason: aborted.to_string(),
        },
    }
}

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, ApplyError> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ApplyError::File {
            path: path.display().to_string(),
            reason: error.to_string(),
        }),
    }
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), ApplyError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| ApplyError::File {
            path: path.display().to_string(),
            reason: error.to_string(),
        })?;
    }
    std::fs::write(path, bytes).map_err(|error| ApplyError::File {
        path: path.display().to_string(),
        reason: error.to_string(),
    })
}

fn remove_file(path: &Path) -> Result<(), ApplyError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ApplyError::File {
            path: path.display().to_string(),
            reason: error.to_string(),
        }),
    }
}

/// Pre-image snapshots under `<data>/sync/snapshots/<id>/`.
pub struct FsSnapshotPort {
    root: PathBuf,
    paths: PathResolver,
}

impl FsSnapshotPort {
    #[must_use]
    pub fn new(paths: PathResolver) -> Self {
        Self {
            root: paths
                .app_data_dir()
                .join("sync/snapshots")
                .into_std_path_buf(),
            paths,
        }
    }

    fn directory(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// The physical path of a logical item, for the snapshot copy.
    fn physical(&self, logical: &LogicalPath) -> Option<PathBuf> {
        let mut segments = logical.segments();
        let root = match segments.next() {
            Some("profiles") => self.paths.app_profiles_dir(),
            Some("icons") => self.paths.app_config_dir().join("icons"),
            Some("app") => self.paths.app_config_dir().to_owned(),
            _ => return None,
        };
        let mut path = root.into_std_path_buf();
        for segment in segments {
            path.push(segment);
        }
        if path.extension().is_none() {
            path.set_extension("yaml");
        }
        Some(path)
    }
}

impl SyncSnapshotPort for FsSnapshotPort {
    fn capture(
        &self,
        pre_images: &std::collections::BTreeMap<LogicalPath, Vec<u8>>,
    ) -> Result<String, ApplyError> {
        let id = format!(
            "cycle-{}",
            nyanpasu_config::sync::ObjectId::of(&nanoid::nanoid!().into_bytes()).as_str()[..12]
                .to_owned()
        );
        let directory = self.directory(&id);
        std::fs::create_dir_all(&directory).map_err(|error| ApplyError::File {
            path: directory.display().to_string(),
            reason: error.to_string(),
        })?;
        let mut captured = Vec::new();
        for (logical, bytes) in pre_images {
            let name = logical.as_str().replace('/', "__");
            std::fs::write(directory.join(&name), bytes).map_err(|error| ApplyError::File {
                path: logical.to_string(),
                reason: error.to_string(),
            })?;
            captured.push(logical.as_str().to_owned());
        }
        let manifest = serde_json::json!({
            "id": id,
            "created_at": sync::clock::now_iso8601(),
            "items": captured,
        });
        std::fs::write(
            directory.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap_or_default(),
        )
        .map_err(|error| ApplyError::File {
            path: directory.display().to_string(),
            reason: error.to_string(),
        })?;
        Ok(id)
    }

    fn restore(&self, snapshot_id: &str) -> Result<(), ApplyError> {
        let directory = self.directory(snapshot_id);
        let manifest =
            std::fs::read(directory.join("manifest.json")).map_err(|error| ApplyError::File {
                path: directory.display().to_string(),
                reason: error.to_string(),
            })?;
        let manifest: serde_json::Value =
            serde_json::from_slice(&manifest).map_err(|error| ApplyError::File {
                path: directory.display().to_string(),
                reason: error.to_string(),
            })?;
        for item in manifest["items"].as_array().into_iter().flatten() {
            let Some(logical) = item
                .as_str()
                .and_then(|value| LogicalPath::parse(value).ok())
            else {
                continue;
            };
            let Some(target) = self.physical(&logical) else {
                continue;
            };
            let name = logical.as_str().replace('/', "__");
            let source = directory.join(name);
            let bytes = std::fs::read(&source).map_err(|error| ApplyError::File {
                path: source.display().to_string(),
                reason: error.to_string(),
            })?;
            write_bytes(&target, &bytes)?;
        }
        Ok(())
    }
}

/// Lists the snapshots under `<data>/sync/snapshots`.
pub fn list_snapshots(paths: &PathResolver) -> Vec<SnapshotSummary> {
    let root = paths.app_data_dir().join("sync/snapshots");
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut summaries: Vec<SnapshotSummary> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let manifest = std::fs::read(entry.path().join("manifest.json")).ok()?;
            let value: serde_json::Value = serde_json::from_slice(&manifest).ok()?;
            Some(SnapshotSummary {
                id: value["id"].as_str()?.to_owned(),
                created_at: value["created_at"].as_str()?.to_owned(),
                items: value["items"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.as_str().and_then(|v| LogicalPath::parse(v).ok()))
                    .collect(),
            })
        })
        .collect();
    summaries.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    summaries
}

/// Builds the ports for one client from its owners and injected adapters.
/// How long the portable documents must be quiet before a change triggers a
/// cycle, so dragging a slider produces one sync rather than dozens.
const QUIET_PERIOD: std::time::Duration = std::time::Duration::from_secs(5);

pub(crate) struct SyncSetup<'a> {
    pub paths: &'a PathResolver,
    pub jobs: nyanpasu_jobs::JobsClient,
    pub dirty: crate::sync::SyncDirty,
    pub application: ApplicationClient,
    pub clash: ClashConfigClient,
    pub profiles: ProfilesClient,
    pub adapters: Option<SyncAdapters>,
    pub hub: Arc<SyncStatusHub>,
    pub shutdown: tokio_util::sync::CancellationToken,
}

impl SyncSetup<'_> {
    pub(crate) async fn spawn(
        self,
        tasks: &tokio_util::task::TaskTracker,
    ) -> Result<SyncClient, Error> {
        let SyncSetup {
            paths,
            jobs,
            dirty,
            application,
            clash,
            profiles,
            adapters,
            hub,
            shutdown,
        } = self;
        let settings_path = paths.app_config_dir().join("local/sync.yaml");
        let settings_store: Arc<dyn SyncSettingsStore> = Arc::new(FileSyncSettingsStore::new(
            settings_path.into_std_path_buf(),
        ));
        let initial = settings_store.load().unwrap_or_else(|error| {
            tracing::warn!("sync settings are unreadable; sync stays disabled: {error}");
            nyanpasu_contracts::sync::SyncSettings::default()
        });
        let state_path = paths.app_data_dir().join("sync/state.redb");
        let state_store: Arc<dyn SyncStateStore> =
            match RedbSyncStateStore::open(&state_path.into_std_path_buf()) {
                Ok(store) => Arc::new(store),
                Err(error) => {
                    tracing::warn!("sync state is unreadable; the device rejoins as new: {error}");
                    Arc::new(UnavailableStateStore)
                }
            };
        let (settings_tx, settings_rx) = tokio::sync::watch::channel(initial.clone());
        let initial_settings = initial.clone();
        let settings_watch = settings_rx.clone();
        let (remote, secrets): (
            Option<Arc<dyn nyanpasu_contracts::sync::RemoteStoreFactory>>,
            Arc<dyn nyanpasu_contracts::sync::SecretStore>,
        ) = match adapters {
            Some(adapters) => (Some(adapters.remote), adapters.secrets),
            None => (
                None,
                Arc::new(MissingSecrets) as Arc<dyn nyanpasu_contracts::sync::SecretStore>,
            ),
        };
        let ports = SyncPorts {
            state: state_store,
            settings: settings_store,
            secrets,
            remote: remote.clone().unwrap_or_else(|| {
                Arc::new(NoRemoteFactory) as Arc<dyn nyanpasu_contracts::sync::RemoteStoreFactory>
            }),
            portable: Arc::new(OwnerStatePort {
                application,
                clash,
                profiles,
                paths: paths.clone(),
            }),
            files: Arc::new(ResolverFilePort::new(paths.clone())),
            snapshots: Arc::new(FsSnapshotPort::new(paths.clone())),
            status: hub,
            paths: paths.clone(),
            shutdown: shutdown.clone(),
        };
        let client = SyncClient::spawn(
            ports,
            initial.clone(),
            remote,
            settings_tx,
            shutdown.clone(),
            tasks,
        )
        .await
        .context(StartSnafu)?;
        // A burst of local edits produces one cycle: wait for quiet, then run.
        {
            let (dirty, sync_client, mut settings, token) = (
                dirty.clone(),
                client.clone(),
                settings_watch,
                shutdown.clone(),
            );
            tasks.spawn(async move {
                let mut rx = dirty.subscribe();
                loop {
                    tokio::select! {
                        () = token.cancelled() => break,
                        event = rx.recv() => {
                            if event.is_err() {
                                break;
                            }
                            // Coalesce: keep resetting the quiet window while
                            // hints keep arriving.
                            loop {
                                tokio::select! {
                                    () = token.cancelled() => return,
                                    () = tokio::time::sleep(QUIET_PERIOD) => break,
                                    event = rx.recv() => {
                                        if event.is_err() {
                                            return;
                                        }
                                    }
                                }
                            }
                            let current = settings.borrow_and_update().clone();
                            if current.enabled
                                && current.sync_on_change
                                && let Err(failure) = sync_client.sync_scheduled().await
                            {
                                tracing::debug!("a change-triggered sync did not run: {failure}");
                            }
                        }
                    }
                }
            });
        }
        // The interval cycle is a job, so its run history and logs appear in
        // the existing jobs UI and a scheduled cycle never overlaps a manual
        // one. The facade owns registration; the actor only publishes the
        // committed settings it must follow.
        let jobs = sync::SyncJobs::new(jobs.clone());
        let (registered, mut changes, sync_client, token) =
            (jobs.clone(), settings_rx, client.clone(), shutdown.clone());
        let initial_jobs = jobs.clone();
        let initial_settings = initial_settings.clone();
        let initial_client = client.clone();
        tasks.spawn(async move {
            if let Err(error) = initial_jobs.reconcile(&initial_settings, initial_client).await {
                tracing::warn!("the sync schedule could not be registered: {error}");
            }
            loop {
                tokio::select! {
                    () = token.cancelled() => break,
                    changed = changes.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        let settings = changes.borrow_and_update().clone();
                        if let Err(error) = registered.reconcile(&settings, sync_client.clone()).await {
                            tracing::warn!("the sync schedule could not be updated: {error}");
                        }
                    }
                }
            }
        });
        Ok(client)
    }
}

struct UnavailableStateStore;

impl SyncStateStore for UnavailableStateStore {
    fn load(&self) -> Result<sync::state::LoadedState, sync::state::StateError> {
        Ok(sync::state::LoadedState::default())
    }

    fn save_baseline(
        &self,
        _meta: &sync::state::SyncMeta,
        _baseline: &BTreeMap<LogicalPath, sync::state::BaselineItem>,
        _documents: &BTreeMap<LogicalPath, PortableDocument>,
    ) -> Result<(), sync::state::StateError> {
        Ok(())
    }

    fn save_conflicts(&self, _conflicts: &[SyncConflict]) -> Result<(), sync::state::StateError> {
        Ok(())
    }

    fn save_blocked(
        &self,
        _blocked: &[sync::state::BlockedRecord],
    ) -> Result<(), sync::state::StateError> {
        Ok(())
    }
}

struct MissingSecrets;

impl nyanpasu_contracts::sync::SecretStore for MissingSecrets {
    fn load(
        &self,
    ) -> Result<
        Option<nyanpasu_contracts::sync::ProviderSecrets>,
        nyanpasu_contracts::sync::SecretStoreError,
    > {
        Ok(None)
    }

    fn save(
        &self,
        _secrets: &nyanpasu_contracts::sync::ProviderSecrets,
    ) -> Result<(), nyanpasu_contracts::sync::SecretStoreError> {
        Err(nyanpasu_contracts::sync::SecretStoreError::Io {
            reason: "this shell has no sync credential store".to_owned(),
        })
    }

    fn clear(&self) -> Result<(), nyanpasu_contracts::sync::SecretStoreError> {
        Ok(())
    }
}

struct NoRemoteFactory;

impl nyanpasu_contracts::sync::RemoteStoreFactory for NoRemoteFactory {
    fn kinds(&self) -> Vec<nyanpasu_contracts::sync::ProviderKind> {
        Vec::new()
    }

    fn open(
        &self,
        _config: &nyanpasu_contracts::sync::ProviderConfig,
        _secrets: &nyanpasu_contracts::sync::ProviderSecrets,
    ) -> Result<Arc<dyn nyanpasu_contracts::sync::RemoteStore>, nyanpasu_contracts::sync::RemoteError>
    {
        Err(nyanpasu_contracts::sync::RemoteError::Unsupported {
            operation: "configuration sync in this shell",
        })
    }
}

/// Facade operations; each is a thin call to the typed client.
impl super::NyanpasuClient {
    fn sync(&self) -> &SyncClient {
        &self.inner.sync
    }

    /// The current full sync status.
    ///
    /// # Errors
    ///
    /// [`Error::Status`] when the owner stopped.
    pub async fn sync_status(&self) -> Result<SyncStatus, Error> {
        self.sync().status().await.context(StatusSnafu)
    }

    /// A receiver that publishes every new status, for the RPC event bridge.
    #[must_use]
    pub fn subscribe_sync_status(&self) -> tokio::sync::watch::Receiver<SyncStatus> {
        self.inner.sync_status.subscribe()
    }

    /// Settings, whether credentials exist, and the compiled provider kinds.
    ///
    /// # Errors
    ///
    /// [`Error::Settings`] when the owner stopped.
    pub async fn sync_settings(&self) -> Result<sync::SettingsReply, Error> {
        self.sync().settings().await.context(SettingsSnafu)
    }

    /// Open conflicts with both values.
    ///
    /// # Errors
    ///
    /// [`Error::Conflicts`] when the owner stopped.
    pub async fn list_sync_conflicts(&self) -> Result<Vec<SyncConflict>, Error> {
        self.sync().conflicts().await.context(ConflictsSnafu)
    }

    pub fn list_sync_snapshots(&self) -> Vec<SnapshotSummary> {
        list_snapshots(&self.inner.paths)
    }

    pub async fn test_sync_connection(
        &self,
        settings: nyanpasu_contracts::sync::SyncSettings,
        secrets: Option<nyanpasu_contracts::sync::ProviderSecrets>,
    ) -> Result<(), SyncFailure> {
        self.sync().test_connection(settings, secrets).await
    }

    pub async fn configure_sync(
        &self,
        settings: nyanpasu_contracts::sync::SyncSettings,
        secrets: Option<nyanpasu_contracts::sync::ProviderSecrets>,
    ) -> Result<(), SyncFailure> {
        self.sync().configure(settings, secrets).await
    }

    pub async fn enable_sync(&self) -> Result<CycleReport, SyncFailure> {
        self.sync().enable().await
    }

    pub async fn disable_sync(&self) -> Result<(), SyncFailure> {
        self.sync().disable().await
    }

    pub async fn sync_now(&self) -> Result<CycleReport, SyncFailure> {
        self.sync().sync_now().await
    }

    pub async fn resolve_sync_conflict(
        &self,
        conflict_id: String,
        choice: ConflictChoice,
    ) -> Result<CycleReport, SyncFailure> {
        self.sync().resolve_conflict(conflict_id, choice).await
    }

    pub async fn restore_sync_snapshot(&self, snapshot_id: String) -> Result<(), SyncFailure> {
        self.sync().restore_snapshot(snapshot_id).await
    }
}
