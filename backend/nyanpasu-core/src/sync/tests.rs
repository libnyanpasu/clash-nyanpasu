//! Two simulated devices over one in-memory vault.
//!
//! These exercise the real engine, planner, merge and commit format against
//! the contracts fake, so a protocol regression fails here without a network.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
    sync::PortableDocument,
};
use nyanpasu_contracts::sync::{
    ProviderConfig, ProviderKind, ProviderSecrets, RemoteError, RemoteStore, RemoteStoreFactory,
    SyncSettings,
    test_support::{ConditionalCreate, MemoryRemoteStore},
};
use nyanpasu_paths::{LogicalPath, PathResolver};
use nyanpasu_runtime_values::sync::{SyncBlockedReason, SyncOutcome, SyncPhase, SyncStatus};
use nyanpasu_state_engine::Version;

use super::{
    engine::{CycleInput, run_cycle},
    ports::{
        ApplyError, ApplyOutcome, CommittedDocument, CommittedProfiles, PortableStatePort,
        SyncFilePort, SyncPorts, SyncSnapshotPort, SyncStatusSink,
    },
    state::{LoadedState, RedbSyncStateStore, StateError, SyncMeta, SyncStateStore},
};

#[derive(Default)]
struct DeviceDocuments {
    application: NyanpasuAppConfig,
    clash: ClashConfig,
    profiles: Profiles,
    version: u64,
}

struct FakePortable {
    documents: Mutex<DeviceDocuments>,
    /// Scripted apply outcomes, consumed one per apply; a refusal test queues
    /// one per call it expects.
    script: Mutex<std::collections::VecDeque<ApplyOutcome>>,
    /// The bytes the guarded capture returns. It is deliberately separate from
    /// the file port so a test can make a later direct read answer differently
    /// and prove the cycle uses the captured bytes.
    captured: Mutex<BTreeMap<LogicalPath, Vec<u8>>>,
}

impl FakePortable {
    fn new() -> Self {
        Self {
            documents: Mutex::new(DeviceDocuments::default()),
            captured: Mutex::new(BTreeMap::new()),
            script: Mutex::new(std::collections::VecDeque::new()),
        }
    }

    fn capture_file(&self, logical: LogicalPath, bytes: Vec<u8>) {
        self.captured.lock().unwrap().insert(logical, bytes);
    }

    /// Queues the outcome the next apply reports. A scripted rejection or
    /// conflict returns without committing; a scripted deferred commit really
    /// commits and only changes how the result is reported.
    fn script(&self, outcome: ApplyOutcome) {
        self.script.lock().unwrap().push_back(outcome);
    }

    fn take_script(&self) -> Option<ApplyOutcome> {
        self.script.lock().unwrap().pop_front()
    }
}

fn next_version(state: &mut DeviceDocuments) -> Version {
    state.version += 1;
    Version::new(state.version)
}

#[async_trait::async_trait]
impl PortableStatePort for FakePortable {
    async fn application(&self) -> Result<CommittedDocument<NyanpasuAppConfig>, ApplyError> {
        let state = self.documents.lock().unwrap();
        Ok(CommittedDocument {
            state: state.application.clone(),
            version: Version::new(state.version),
        })
    }

    async fn clash(&self) -> Result<CommittedDocument<ClashConfig>, ApplyError> {
        let state = self.documents.lock().unwrap();
        Ok(CommittedDocument {
            state: state.clash.clone(),
            version: Version::new(state.version),
        })
    }

    async fn profiles(&self) -> Result<CommittedProfiles, ApplyError> {
        let state = self.documents.lock().unwrap();
        let mut files = std::collections::BTreeMap::new();
        for (logical, bytes) in self.captured.lock().unwrap().iter() {
            if let Some(name) = logical.as_str().strip_prefix("profiles/") {
                files.insert(name.to_owned(), bytes.clone());
            }
        }
        Ok(CommittedProfiles {
            state: state.profiles.clone(),
            version: Version::new(state.version),
            files,
            document: Vec::new(),
        })
    }

    async fn apply_application(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<ApplyOutcome, ApplyError> {
        let scripted = self.take_script();
        if let Some(
            outcome @ (ApplyOutcome::Refused { .. }
            | ApplyOutcome::Unavailable { .. }
            | ApplyOutcome::VersionConflict),
        ) = &scripted
        {
            return Ok(outcome.clone());
        }
        let mut state = self.documents.lock().unwrap();
        if Version::new(state.version) != expected {
            return Ok(ApplyOutcome::VersionConflict);
        }
        state.application = nyanpasu_config::sync::apply_application(&state.application, merged)
            .map_err(|error| ApplyError::Rejected {
                item: "app/application".to_owned(),
                reason: error.to_string(),
            })?;
        next_version(&mut state);
        Ok(scripted.unwrap_or(ApplyOutcome::Committed { deferred: false }))
    }

    async fn apply_clash(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<ApplyOutcome, ApplyError> {
        let scripted = self.take_script();
        if let Some(
            outcome @ (ApplyOutcome::Refused { .. }
            | ApplyOutcome::Unavailable { .. }
            | ApplyOutcome::VersionConflict),
        ) = &scripted
        {
            return Ok(outcome.clone());
        }
        let mut state = self.documents.lock().unwrap();
        if Version::new(state.version) != expected {
            return Ok(ApplyOutcome::VersionConflict);
        }
        state.clash =
            nyanpasu_config::sync::apply_clash(&state.clash, merged).map_err(|error| {
                ApplyError::Rejected {
                    item: "app/clash".to_owned(),
                    reason: error.to_string(),
                }
            })?;
        next_version(&mut state);
        Ok(scripted.unwrap_or(ApplyOutcome::Committed { deferred: false }))
    }

    async fn apply_profiles(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<ApplyOutcome, ApplyError> {
        let scripted = self.take_script();
        if let Some(
            outcome @ (ApplyOutcome::Refused { .. }
            | ApplyOutcome::Unavailable { .. }
            | ApplyOutcome::VersionConflict),
        ) = &scripted
        {
            return Ok(outcome.clone());
        }
        let mut state = self.documents.lock().unwrap();
        if Version::new(state.version) != expected {
            return Ok(ApplyOutcome::VersionConflict);
        }
        state.profiles =
            nyanpasu_config::sync::apply_profiles(&state.profiles, merged).map_err(|error| {
                ApplyError::Rejected {
                    item: "app/profiles".to_owned(),
                    reason: error.to_string(),
                }
            })?;
        next_version(&mut state);
        Ok(scripted.unwrap_or(ApplyOutcome::Committed { deferred: false }))
    }

    async fn active_profile(&self) -> Option<String> {
        None
    }
}

#[derive(Default)]
struct FakeFiles {
    text: Mutex<BTreeMap<LogicalPath, Vec<u8>>>,
}

impl FakeFiles {
    fn put(&self, path: &str, bytes: &[u8]) {
        self.text
            .lock()
            .unwrap()
            .insert(LogicalPath::parse(path).unwrap(), bytes.to_vec());
    }

    fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.text
            .lock()
            .unwrap()
            .get(&LogicalPath::parse(path).unwrap())
            .cloned()
    }
}

impl SyncFilePort for FakeFiles {
    fn read_text(&self, logical: &LogicalPath) -> Result<Option<Vec<u8>>, ApplyError> {
        Ok(self.text.lock().unwrap().get(logical).cloned())
    }

    fn write_text(&self, logical: &LogicalPath, bytes: &[u8]) -> Result<(), ApplyError> {
        self.text
            .lock()
            .unwrap()
            .insert(logical.clone(), bytes.to_vec());
        Ok(())
    }

    fn remove_text(&self, logical: &LogicalPath) -> Result<(), ApplyError> {
        self.text.lock().unwrap().remove(logical);
        Ok(())
    }

    fn read_binary(&self, logical: &LogicalPath) -> Result<Option<Vec<u8>>, ApplyError> {
        self.read_text(logical)
    }

    fn write_binary(&self, logical: &LogicalPath, bytes: &[u8]) -> Result<(), ApplyError> {
        self.write_text(logical, bytes)
    }

    fn remove_binary(&self, logical: &LogicalPath) -> Result<(), ApplyError> {
        self.remove_text(logical)
    }

    fn read_document(&self, logical: &LogicalPath) -> Result<Option<Vec<u8>>, ApplyError> {
        Ok(self.text.lock().unwrap().get(logical).cloned())
    }
}

struct NoSnapshots;

impl SyncSnapshotPort for NoSnapshots {
    fn capture(&self, _items: &BTreeMap<LogicalPath, Vec<u8>>) -> Result<String, ApplyError> {
        Ok("test".to_owned())
    }

    fn restore(&self, _snapshot_id: &str) -> Result<(), ApplyError> {
        Ok(())
    }
}

#[derive(Default)]
struct NoStatus;

impl SyncStatusSink for NoStatus {
    fn publish(&self, _status: &SyncStatus) {}
}

struct FixedFactory(Arc<MemoryRemoteStore>);

impl RemoteStoreFactory for FixedFactory {
    fn kinds(&self) -> Vec<ProviderKind> {
        vec![ProviderKind::Fs]
    }

    fn open(
        &self,
        _config: &ProviderConfig,
        _secrets: &ProviderSecrets,
    ) -> Result<Arc<dyn RemoteStore>, RemoteError> {
        Ok(self.0.clone())
    }
}

struct NoSettings;

impl super::settings::SyncSettingsStore for NoSettings {
    fn load(&self) -> Result<SyncSettings, super::settings::SettingsError> {
        Ok(SyncSettings::default())
    }

    fn save(&self, _settings: &SyncSettings) -> Result<(), super::settings::SettingsError> {
        Ok(())
    }
}

struct NoSecrets;

impl nyanpasu_contracts::sync::SecretStore for NoSecrets {
    fn load(&self) -> Result<Option<ProviderSecrets>, nyanpasu_contracts::sync::SecretStoreError> {
        Ok(None)
    }

    fn save(
        &self,
        _secrets: &ProviderSecrets,
    ) -> Result<(), nyanpasu_contracts::sync::SecretStoreError> {
        Ok(())
    }

    fn clear(&self) -> Result<(), nyanpasu_contracts::sync::SecretStoreError> {
        Ok(())
    }
}

/// One simulated device: its owners, files, baseline and settings.
struct Device {
    portable: Arc<FakePortable>,
    files: Arc<FakeFiles>,
    state: Arc<RedbSyncStateStore>,
    ports: SyncPorts,
    settings: SyncSettings,
    _directory: tempfile::TempDir,
}

impl Device {
    fn new(store: &MemoryRemoteStore, name: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let state =
            Arc::new(RedbSyncStateStore::open(&directory.path().join("sync/state.redb")).unwrap());
        let files = Arc::new(FakeFiles::default());
        let portable = Arc::new(FakePortable::new());
        let paths = PathResolver::with_base_dirs(
            directory.path().join("config").try_into().unwrap(),
            directory.path().join("data").try_into().unwrap(),
        );
        let ports = SyncPorts {
            state: state.clone(),
            settings: Arc::new(NoSettings),
            secrets: Arc::new(NoSecrets),
            remote: Arc::new(FixedFactory(Arc::new(store.connect()))),
            portable: portable.clone(),
            files: files.clone(),
            snapshots: Arc::new(NoSnapshots),
            status: Arc::new(NoStatus),
            paths: paths.clone(),
            shutdown: tokio_util::sync::CancellationToken::new(),
        };
        Self {
            portable,
            files,
            state,
            ports,
            settings: SyncSettings {
                enabled: true,
                device_name: name.to_owned(),
                ..Default::default()
            },
            _directory: directory,
        }
    }

    async fn sync(&self, store: &MemoryRemoteStore) -> super::engine::CycleReport {
        let meta = self.state.load().unwrap().meta;
        let mut status = SyncStatus::default();
        run_cycle(CycleInput {
            ports: &self.ports,
            store: Arc::new(store.connect()),
            meta: &meta,
            settings: &self.settings,
            status: &mut status,
            retry_blocked: false,
        })
        .await
        .unwrap()
    }

    fn put_file(&self, name: &str, bytes: &[u8]) {
        let logical = LogicalPath::parse(&format!("profiles/{name}")).unwrap();
        // Both the guarded capture and the file port see a locally written file.
        self.portable.capture_file(logical.clone(), bytes.to_vec());
        self.files.put(&format!("profiles/{name}"), bytes);
    }

    fn read_file(&self, name: &str) -> Option<Vec<u8>> {
        self.files.get(&format!("profiles/{name}"))
    }

    /// Registers a managed local profile, as the profiles owner would.
    fn add_local_profile(&self, file: &str) {
        let mut state = self.portable.documents.lock().unwrap();
        let uid =
            nyanpasu_config::profile::ProfileId(format!("p{}", file.trim_end_matches(".yaml")));
        let item = nyanpasu_config::profile::ProfileItem {
            uid: uid.clone(),
            metadata: nyanpasu_config::profile::ProfileMetadata {
                name: file.to_owned(),
                desc: None,
                custom_name: true,
            },
            definition: nyanpasu_config::profile::ProfileDefinition::Config {
                config: nyanpasu_config::profile::ConfigDefinition::File(
                    nyanpasu_config::profile::FileConfig {
                        source: nyanpasu_config::profile::ProfileSource::Local {
                            binding: nyanpasu_config::profile::LocalBinding::Managed {
                                materialized: nyanpasu_config::profile::MaterializedFile {
                                    file: nyanpasu_config::profile::ManagedProfilePath::new(file)
                                        .unwrap(),
                                    updated_at: None,
                                },
                            },
                        },
                        transforms: Vec::new(),
                    },
                ),
            },
        };
        state.profiles.items.insert(uid, item);
        state.version += 1;
    }

    fn set_theme(&self, theme: nyanpasu_config::application::ThemeMode) {
        let mut state = self.portable.documents.lock().unwrap();
        state.application.theme_mode = theme;
        state.version += 1;
    }

    fn theme(&self) -> nyanpasu_config::application::ThemeMode {
        self.portable
            .documents
            .lock()
            .unwrap()
            .application
            .theme_mode
    }

    fn set_language(&self, language: nyanpasu_config::application::I18nLanguage) {
        let mut state = self.portable.documents.lock().unwrap();
        state.application.language = language;
        state.version += 1;
    }

    fn language(&self) -> nyanpasu_config::application::I18nLanguage {
        self.portable.documents.lock().unwrap().application.language
    }
}

fn store() -> MemoryRemoteStore {
    MemoryRemoteStore::new(ConditionalCreate::Honored)
}

/// Runs one cycle for the device with an explicit retry flag.
async fn sync_with(
    device: &Device,
    vault: &MemoryRemoteStore,
    retry_blocked: bool,
) -> super::engine::CycleReport {
    let meta = device.state.load().unwrap().meta;
    let mut status = SyncStatus::default();
    run_cycle(CycleInput {
        ports: &device.ports,
        store: Arc::new(vault.connect()),
        meta: &meta,
        settings: &device.settings,
        status: &mut status,
        retry_blocked,
    })
    .await
    .unwrap()
}

/// A refused pull keeps the baseline and is not retried until the user asks.
#[tokio::test]
async fn a_refused_apply_is_blocked_and_not_retried_unchanged() {
    let vault = store();
    // The device establishes its baseline first, then the remote moves, so the
    // device's next cycle plans a pull.
    let device = Device::new(&vault, "device");
    device.sync(&vault).await;
    let publisher = Device::new(&vault, "publisher");
    publisher.sync(&vault).await;
    publisher.set_theme(nyanpasu_config::application::ThemeMode::Dark);
    publisher.sync(&vault).await;

    device.portable.script(ApplyOutcome::Refused {
        reason: "the core rejected the candidate".to_owned(),
    });
    let report = sync_with(&device, &vault, false).await;
    assert!(
        report
            .blocked
            .iter()
            .any(|item| matches!(item.reason, SyncBlockedReason::RuntimeRefused { .. })),
        "{:?}",
        report.blocked
    );
    let loaded = device.state.load().unwrap();
    assert!(
        loaded
            .blocked
            .iter()
            .any(|record| record.logical.as_str() == "app/application"),
        "{:?}",
        loaded.blocked
    );
    // The refused document kept its old base.
    let baseline = loaded
        .baseline_documents
        .get(&LogicalPath::parse("app/application").unwrap())
        .expect("a baseline document");
    assert_eq!(
        baseline.payload["theme_mode"],
        serde_json::json!("system"),
        "the refused unit keeps its base"
    );

    // A scheduled cycle does not retry the unchanged candidate.
    let scheduled = sync_with(&device, &vault, false).await;
    assert!(
        scheduled
            .blocked
            .iter()
            .any(|item| matches!(item.reason, SyncBlockedReason::RuntimeRefused { .. })),
        "{:?}",
        scheduled.blocked
    );
    assert_eq!(
        device.theme(),
        nyanpasu_config::application::ThemeMode::System,
        "the refusal is still in force"
    );

    // An explicit retry applies it.
    let retried = sync_with(&device, &vault, true).await;
    assert!(!retried.replan);
    assert_eq!(
        device.theme(),
        nyanpasu_config::application::ThemeMode::Dark,
        "an explicit retry applies the refused candidate"
    );
    assert!(
        device.state.load().unwrap().blocked.is_empty(),
        "the record is cleared once the candidate applies"
    );
}

/// A transient check failure keeps no record and is retried next cycle.
#[tokio::test]
async fn a_transient_failure_is_retried_and_keeps_no_record() {
    let vault = store();
    let device = Device::new(&vault, "device");
    device.sync(&vault).await;
    let publisher = Device::new(&vault, "publisher");
    publisher.sync(&vault).await;
    publisher.set_theme(nyanpasu_config::application::ThemeMode::Dark);
    publisher.sync(&vault).await;

    device.portable.script(ApplyOutcome::Unavailable {
        reason: "the candidate could not be checked".to_owned(),
    });
    let report = sync_with(&device, &vault, false).await;
    assert!(
        report
            .blocked
            .iter()
            .any(|item| matches!(item.reason, SyncBlockedReason::RuntimeUnavailable { .. })),
        "{:?}",
        report.blocked
    );
    assert!(
        device.state.load().unwrap().blocked.is_empty(),
        "a transient failure is not recorded as a refusal"
    );

    // The next scheduled cycle retries and applies.
    let retried = sync_with(&device, &vault, false).await;
    assert_eq!(
        device.theme(),
        nyanpasu_config::application::ThemeMode::Dark,
        "{retried:?}"
    );
}

/// A committed mutation whose runtime application is deferred still advances
/// the baseline, because the source was saved.
#[tokio::test]
async fn a_deferred_commit_advances_the_baseline() {
    let vault = store();
    let device = Device::new(&vault, "device");
    device.sync(&vault).await;
    let publisher = Device::new(&vault, "publisher");
    publisher.sync(&vault).await;
    publisher.set_theme(nyanpasu_config::application::ThemeMode::Dark);
    publisher.sync(&vault).await;

    // The owner commits the source but reports a deferral; the fake still
    // records the committed document because a deferral is not a failed write.
    device
        .portable
        .script(ApplyOutcome::Committed { deferred: true });
    let report = sync_with(&device, &vault, false).await;
    assert!(report.blocked.is_empty(), "{:?}", report.blocked);
    let loaded = device.state.load().unwrap();
    assert_eq!(
        loaded
            .baseline_documents
            .get(&LogicalPath::parse("app/application").unwrap())
            .expect("a baseline document")
            .payload["theme_mode"],
        serde_json::json!("dark"),
        "a deferred commit still advances the baseline"
    );
    assert_eq!(
        device.theme(),
        nyanpasu_config::application::ThemeMode::Dark,
        "the deferred commit saved the source"
    );
}

/// An owner that moved during the cycle asks for a replan rather than a
/// refusal, and the next cycle converges.
#[tokio::test]
async fn a_stale_version_asks_for_a_replan() {
    let vault = store();
    let device = Device::new(&vault, "device");
    device.sync(&vault).await;
    let publisher = Device::new(&vault, "publisher");
    publisher.sync(&vault).await;
    publisher.set_theme(nyanpasu_config::application::ThemeMode::Dark);
    publisher.sync(&vault).await;

    device.portable.script(ApplyOutcome::VersionConflict);
    let report = sync_with(&device, &vault, false).await;
    assert!(report.replan, "a stale version asks for a replan");
    assert!(report.blocked.is_empty());

    // A fresh cycle sees the current version and applies.
    let converged = sync_with(&device, &vault, false).await;
    assert!(!converged.replan);
    assert_eq!(
        device.theme(),
        nyanpasu_config::application::ThemeMode::Dark,
        "{converged:?}"
    );
}

/// Records the pre-images a cycle hands to the snapshot port.
#[derive(Default)]
struct RecordingSnapshots {
    captured: Mutex<Option<BTreeMap<LogicalPath, Vec<u8>>>>,
}

impl SyncSnapshotPort for RecordingSnapshots {
    fn capture(&self, pre_images: &BTreeMap<LogicalPath, Vec<u8>>) -> Result<String, ApplyError> {
        *self.captured.lock().unwrap() = Some(pre_images.clone());
        Ok("cycle-test".to_owned())
    }

    fn restore(&self, _snapshot_id: &str) -> Result<(), ApplyError> {
        Ok(())
    }
}

/// A device whose file port answers with content that differs from the guarded
/// capture, standing in for a write that lands after the guard is released.
struct MismatchedDevice {
    portable: Arc<FakePortable>,
    files: Arc<FakeFiles>,
    snapshots: Arc<RecordingSnapshots>,
    state: Arc<RedbSyncStateStore>,
    ports: SyncPorts,
    settings: SyncSettings,
    _directory: tempfile::TempDir,
}

impl MismatchedDevice {
    fn new(store: &MemoryRemoteStore) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let state =
            Arc::new(RedbSyncStateStore::open(&directory.path().join("sync/state.redb")).unwrap());
        let files = Arc::new(FakeFiles::default());
        let portable = Arc::new(FakePortable::new());
        let snapshots = Arc::new(RecordingSnapshots::default());
        let paths = PathResolver::with_base_dirs(
            directory.path().join("config").try_into().unwrap(),
            directory.path().join("data").try_into().unwrap(),
        );
        let ports = SyncPorts {
            state: state.clone(),
            settings: Arc::new(NoSettings),
            secrets: Arc::new(NoSecrets),
            remote: Arc::new(FixedFactory(Arc::new(store.connect()))),
            portable: portable.clone(),
            files: files.clone(),
            snapshots: snapshots.clone(),
            status: Arc::new(NoStatus),
            paths,
            shutdown: tokio_util::sync::CancellationToken::new(),
        };
        Self {
            portable,
            files,
            snapshots,
            state,
            ports,
            settings: SyncSettings {
                enabled: true,
                device_name: "mismatched".to_owned(),
                ..Default::default()
            },
            _directory: directory,
        }
    }

    fn add_local_profile(&self, file: &str) {
        let mut state = self.portable.documents.lock().unwrap();
        let uid =
            nyanpasu_config::profile::ProfileId(format!("p{}", file.trim_end_matches(".yaml")));
        state.profiles.items.insert(
            uid.clone(),
            nyanpasu_config::profile::ProfileItem {
                uid,
                metadata: nyanpasu_config::profile::ProfileMetadata {
                    name: file.to_owned(),
                    desc: None,
                    custom_name: true,
                },
                definition: nyanpasu_config::profile::ProfileDefinition::Config {
                    config: nyanpasu_config::profile::ConfigDefinition::File(
                        nyanpasu_config::profile::FileConfig {
                            source: nyanpasu_config::profile::ProfileSource::Local {
                                binding: nyanpasu_config::profile::LocalBinding::Managed {
                                    materialized: nyanpasu_config::profile::MaterializedFile {
                                        file: nyanpasu_config::profile::ManagedProfilePath::new(
                                            file,
                                        )
                                        .unwrap(),
                                        updated_at: None,
                                    },
                                },
                            },
                            transforms: Vec::new(),
                        },
                    ),
                },
            },
        );
        state.version += 1;
    }

    async fn sync(&self, store: &MemoryRemoteStore) -> super::engine::CycleReport {
        let meta = self.state.load().unwrap().meta;
        let mut status = SyncStatus::default();
        run_cycle(CycleInput {
            ports: &self.ports,
            store: Arc::new(store.connect()),
            meta: &meta,
            settings: &self.settings,
            status: &mut status,
            retry_blocked: false,
        })
        .await
        .unwrap()
    }
}

/// The bytes a cycle uploads and snapshots are the ones the guarded capture
/// returned, even if the file changed after the guard was released.
#[tokio::test]
async fn a_cycle_uses_the_guarded_capture_not_a_second_read() {
    let vault = store();
    let device = MismatchedDevice::new(&vault);
    device.add_local_profile("local.yaml");
    // The guarded capture holds these bytes; a later direct read would answer
    // with the different ones in the file port.
    let captured = b"committed: 1\n".to_vec();
    device.portable.capture_file(
        LogicalPath::parse("profiles/local.yaml").unwrap(),
        captured.clone(),
    );
    device.files.put("profiles/local.yaml", b"uncommitted: 2\n");

    device.sync(&vault).await;

    let committed_hash = nyanpasu_config::sync::ObjectId::of(&captured);
    assert!(
        vault
            .keys()
            .iter()
            .any(|key| key.ends_with(committed_hash.as_str())),
        "the captured bytes must be uploaded: {:?}",
        vault.keys()
    );
}

/// A pull's pre-image is the bytes the guarded capture returned, not a later
/// read of the live file.
#[tokio::test]
async fn a_pull_snapshots_the_guarded_capture_not_a_second_read() {
    let vault = store();
    // The remote already holds a file the pulling device will overwrite.
    let publisher = Device::new(&vault, "publisher");
    publisher.add_local_profile("local.yaml");
    publisher.put_file("local.yaml", b"remote: 1\n");
    publisher.sync(&vault).await;

    // The second device holds the file with `committed`, while its file port
    // answers `rewritten`, standing in for a write after the guard.
    let device = MismatchedDevice::new(&vault);
    device.add_local_profile("local.yaml");
    let committed = b"committed: 1\n".to_vec();
    device.portable.capture_file(
        LogicalPath::parse("profiles/local.yaml").unwrap(),
        committed.clone(),
    );
    device.files.put("profiles/local.yaml", b"rewritten: 2\n");

    device.sync(&vault).await;

    let snapshot = device.snapshots.captured.lock().unwrap().clone().unwrap();
    assert_eq!(
        snapshot.get(&LogicalPath::parse("profiles/local.yaml").unwrap()),
        Some(&committed),
        "a pre-image must be the captured pair, not a later read"
    );
}

#[tokio::test]
async fn a_backend_honouring_create_if_absent_gets_linear_history() {
    let vault = MemoryRemoteStore::new(ConditionalCreate::Honored);
    let first = Device::new(&vault, "first");
    first.sync(&vault).await;
    let format = nyanpasu_core_sync_format(&vault).await;
    assert_eq!(format, super::commit::CommitNaming::Exclusive);
    // Exclusive naming writes one commit per generation, with no id suffix.
    assert!(
        vault
            .keys()
            .iter()
            .any(|key| key == "commits/000000000001.json"),
        "{:?}",
        vault.keys()
    );
}

#[tokio::test]
async fn a_backend_ignoring_the_condition_gets_content_named_commits() {
    // The fake advertises create-if-absent but overwrites: the self-test must
    // trust the probe, not the flag.
    let vault = MemoryRemoteStore::new(ConditionalCreate::Ignored);
    let first = Device::new(&vault, "first");
    first.sync(&vault).await;
    let format = nyanpasu_core_sync_format(&vault).await;
    assert_eq!(format, super::commit::CommitNaming::Content);
    assert!(
        vault
            .keys()
            .iter()
            .any(|key| key.starts_with("commits/000000000001-")),
        "{:?}",
        vault.keys()
    );
}

#[tokio::test]
async fn a_webdav_like_backend_falls_back_to_content_naming() {
    let vault = MemoryRemoteStore::new(ConditionalCreate::Unsupported);
    let first = Device::new(&vault, "first");
    first.sync(&vault).await;
    let format = nyanpasu_core_sync_format(&vault).await;
    assert_eq!(format, super::commit::CommitNaming::Content);
}

/// Reads the vault format the way a device would.
async fn nyanpasu_core_sync_format(vault: &MemoryRemoteStore) -> super::commit::CommitNaming {
    let bytes = RemoteStore::get(vault, "format.json").await.unwrap();
    super::commit::VaultFormat::parse(&bytes)
        .unwrap()
        .commit_naming
}

#[tokio::test]
async fn a_second_device_pulls_the_first_devices_changes() {
    let vault = store();
    let first = Device::new(&vault, "first");
    let second = Device::new(&vault, "second");

    first.set_theme(nyanpasu_config::application::ThemeMode::Dark);
    let report = first.sync(&vault).await;
    assert_eq!(report.outcome, Some(SyncOutcome::Synced));
    assert_eq!(report.pushed, 3, "the three documents are pushed");

    let report = second.sync(&vault).await;
    assert_eq!(report.outcome, Some(SyncOutcome::Synced));
    // Only the application document differs; the other two are equal on both
    // sides and join the baseline without a merge.
    assert_eq!(report.merged, 1, "{report:?}");
    assert_eq!(report.pulled, 0, "{report:?}");
    assert_eq!(
        second.theme(),
        nyanpasu_config::application::ThemeMode::Dark
    );
}

#[tokio::test]
async fn non_overlapping_edits_merge_without_a_conflict() {
    let vault = store();
    let first = Device::new(&vault, "first");
    let second = Device::new(&vault, "second");
    first.sync(&vault).await;
    second.sync(&vault).await;

    first.set_theme(nyanpasu_config::application::ThemeMode::Dark);
    second.set_language(nyanpasu_config::application::I18nLanguage::Japanese);
    first.sync(&vault).await;
    let report = second.sync(&vault).await;

    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
    assert_eq!(
        second.theme(),
        nyanpasu_config::application::ThemeMode::Dark
    );
    assert_eq!(
        second.language(),
        nyanpasu_config::application::I18nLanguage::Japanese
    );
    first.sync(&vault).await;
    assert_eq!(
        first.language(),
        nyanpasu_config::application::I18nLanguage::Japanese,
        "the merged field reaches the first device"
    );
}

#[tokio::test]
async fn a_profile_file_travels_between_devices() {
    let vault = store();
    let first = Device::new(&vault, "first");
    let second = Device::new(&vault, "second");
    // The catalog must name the file for enumeration to select it.
    first.add_local_profile("local.yaml");
    first.put_file("local.yaml", b"proxies: []\n");
    first.sync(&vault).await;
    let report = second.sync(&vault).await;
    assert_eq!(
        second.read_file("local.yaml"),
        Some(b"proxies: []\n".to_vec()),
        "{report:?}"
    );
}

#[tokio::test]
async fn an_unchanged_vault_reports_up_to_date() {
    let vault = store();
    let device = Device::new(&vault, "only");
    device.sync(&vault).await;
    let report = device.sync(&vault).await;
    assert_eq!(report.outcome, Some(SyncOutcome::UpToDate));
}

#[tokio::test]
async fn the_baseline_is_persisted_for_the_next_cycle() {
    let vault = store();
    let device = Device::new(&vault, "only");
    device.sync(&vault).await;
    let loaded: LoadedState = device.state.load().unwrap();
    assert!(loaded.meta.baseline_commit.is_some());
    assert_eq!(loaded.baseline.len(), 3);
    assert!(
        loaded
            .baseline_documents
            .contains_key(&LogicalPath::parse("app/application").unwrap())
    );
}

#[tokio::test]
async fn a_broken_state_store_stops_the_cycle() {
    struct Broken;
    impl SyncStateStore for Broken {
        fn load(&self) -> Result<LoadedState, StateError> {
            Err(StateError::MalformedPath {
                value: "../escape".to_owned(),
            })
        }
        fn save_baseline(
            &self,
            _meta: &SyncMeta,
            _baseline: &BTreeMap<LogicalPath, super::state::BaselineItem>,
            _documents: &BTreeMap<LogicalPath, PortableDocument>,
        ) -> Result<(), StateError> {
            Ok(())
        }
        fn save_conflicts(
            &self,
            _conflicts: &[nyanpasu_runtime_values::sync::SyncConflict],
        ) -> Result<(), StateError> {
            Ok(())
        }
        fn save_blocked(&self, _blocked: &[super::state::BlockedRecord]) -> Result<(), StateError> {
            Ok(())
        }
    }
    let vault = store();
    let directory = tempfile::tempdir().unwrap();
    let ports = SyncPorts {
        state: Arc::new(Broken),
        settings: Arc::new(NoSettings),
        secrets: Arc::new(NoSecrets),
        remote: Arc::new(FixedFactory(Arc::new(vault.connect()))),
        portable: Arc::new(FakePortable::new()),
        files: Arc::new(FakeFiles::default()),
        snapshots: Arc::new(NoSnapshots),
        status: Arc::new(NoStatus),
        paths: PathResolver::with_base_dirs(
            directory.path().join("config").try_into().unwrap(),
            directory.path().join("data").try_into().unwrap(),
        ),
        shutdown: tokio_util::sync::CancellationToken::new(),
    };
    let mut status = SyncStatus {
        phase: SyncPhase::Idle,
        ..SyncStatus::default()
    };
    let result = run_cycle(CycleInput {
        ports: &ports,
        store: Arc::new(vault.connect()),
        meta: &SyncMeta::default(),
        settings: &SyncSettings::default(),
        status: &mut status,
        retry_blocked: false,
    })
    .await;
    assert!(result.is_err(), "a broken baseline stops the cycle");
}
