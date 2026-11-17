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
use nyanpasu_runtime_values::sync::{SyncOutcome, SyncPhase, SyncStatus};
use nyanpasu_state_engine::Version;

use super::{
    engine::{CycleInput, run_cycle},
    ports::{
        ApplyError, CommittedDocument, PortableStatePort, SyncFilePort, SyncPorts,
        SyncSnapshotPort, SyncStatusSink,
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
}

impl FakePortable {
    fn new() -> Self {
        Self {
            documents: Mutex::new(DeviceDocuments::default()),
        }
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

    async fn profiles(&self) -> Result<CommittedDocument<Profiles>, ApplyError> {
        let state = self.documents.lock().unwrap();
        Ok(CommittedDocument {
            state: state.profiles.clone(),
            version: Version::new(state.version),
        })
    }

    async fn apply_application(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<(), ApplyError> {
        let mut state = self.documents.lock().unwrap();
        if Version::new(state.version) != expected {
            return Err(ApplyError::VersionConflict {
                item: "app/application".to_owned(),
            });
        }
        state.application = nyanpasu_config::sync::apply_application(&state.application, merged)
            .map_err(|error| ApplyError::Rejected {
                item: "app/application".to_owned(),
                reason: error.to_string(),
            })?;
        next_version(&mut state);
        Ok(())
    }

    async fn apply_clash(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<(), ApplyError> {
        let mut state = self.documents.lock().unwrap();
        if Version::new(state.version) != expected {
            return Err(ApplyError::VersionConflict {
                item: "app/clash".to_owned(),
            });
        }
        state.clash =
            nyanpasu_config::sync::apply_clash(&state.clash, merged).map_err(|error| {
                ApplyError::Rejected {
                    item: "app/clash".to_owned(),
                    reason: error.to_string(),
                }
            })?;
        next_version(&mut state);
        Ok(())
    }

    async fn apply_profiles(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<(), ApplyError> {
        let mut state = self.documents.lock().unwrap();
        if Version::new(state.version) != expected {
            return Err(ApplyError::VersionConflict {
                item: "app/profiles".to_owned(),
            });
        }
        state.profiles =
            nyanpasu_config::sync::apply_profiles(&state.profiles, merged).map_err(|error| {
                ApplyError::Rejected {
                    item: "app/profiles".to_owned(),
                    reason: error.to_string(),
                }
            })?;
        next_version(&mut state);
        Ok(())
    }

    async fn active_profile(&self) -> Option<String> {
        None
    }
}

#[derive(Default)]
struct FakeFiles {
    text: Mutex<BTreeMap<LogicalPath, Vec<u8>>>,
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
}

struct NoSnapshots;

impl SyncSnapshotPort for NoSnapshots {
    fn capture(&self, _items: &[LogicalPath]) -> Result<String, ApplyError> {
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
    state: Arc<RedbSyncStateStore>,
    ports: SyncPorts,
    settings: SyncSettings,
    paths: PathResolver,
    _directory: tempfile::TempDir,
}

impl Device {
    fn new(store: &MemoryRemoteStore, name: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let state =
            Arc::new(RedbSyncStateStore::open(&directory.path().join("sync/state.redb")).unwrap());
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
            files: Arc::new(crate::client::sync::ResolverFilePort::new(paths.clone())),
            snapshots: Arc::new(NoSnapshots),
            status: Arc::new(NoStatus),
            paths: paths.clone(),
            shutdown: tokio_util::sync::CancellationToken::new(),
        };
        Self {
            portable,
            state,
            ports,
            paths,
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
        })
        .await
        .unwrap()
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

    /// Writes a profile file where the resolver names it, as an owner would.
    fn put_file(&self, name: &str, bytes: &[u8]) {
        let directory = self.paths.app_profiles_dir();
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(name), bytes).unwrap();
    }

    fn read_file(&self, name: &str) -> Option<Vec<u8>> {
        std::fs::read(self.paths.app_profiles_dir().join(name)).ok()
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
    })
    .await;
    assert!(result.is_err(), "a broken baseline stops the cycle");
}
