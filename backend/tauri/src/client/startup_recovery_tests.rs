//! Recovery regressions in bootstrap order: pre-Tauri migration, setup
//! migration, and typed client construction, all over temporary app paths.

use std::{
    fs,
    path::{Path, PathBuf},
};

use nyanpasu_paths::PathResolver;
use redb::ReadableDatabase as _;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{new_typed_config_clients, tests::test_paths};
use crate::core::{
    migration::Runner,
    storage::{Storage, WebStorage},
};

pub(super) fn evidence(path: &Path) -> Vec<PathBuf> {
    let prefix = format!("{}.corrupt-", path.file_name().unwrap().to_string_lossy());
    fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&prefix)
        })
        .collect()
}

fn fixture() -> (tempfile::TempDir, PathResolver) {
    let temp = tempfile::tempdir().unwrap();
    let paths = test_paths(temp.path().join("config"), temp.path().join("data"));
    paths.create_base_dirs().unwrap();
    (temp, paths)
}

pub(super) fn migrate(paths: &PathResolver) {
    Runner::with_paths(paths.clone(), false)
        .unwrap()
        .run_pending()
        .unwrap();
}

#[tokio::test]
async fn session_state_recovers_in_each_migration_and_client_startup() {
    // Corruption can be present at launch, or arrive between the two migration
    // executions and client construction. Each boundary must recover itself.
    for corrupt_at in 0..3 {
        let (_temp, paths) = fixture();
        migrate(&paths);
        let path = paths.session_state_path();
        let bad = b"window_state: [invalid";
        for boundary in 0..3 {
            if boundary == corrupt_at {
                fs::write(&path, bad).unwrap();
            }
            if boundary < 2 {
                migrate(&paths);
                if boundary >= corrupt_at {
                    assert_eq!(
                        evidence(path.as_std_path()).len(),
                        1,
                        "migration must recover before clients"
                    );
                    let _: nyanpasu_config::state::PersistentState =
                        serde_yaml::from_slice(&fs::read(&path).unwrap()).unwrap();
                }
            }
        }
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let (_application, state, _clash) = new_typed_config_clients(
            crate::state::mutation::MutationCoordinator::isolated(),
            crate::bundle::Channel::Stable,
            paths.clone(),
            &shutdown,
            &tasks,
        )
        .await
        .unwrap();
        assert!(state.snapshot().state.window_state.is_empty());
        state
            .save_main_window(nyanpasu_config::state::window::WindowState {
                width: 800,
                height: 600,
                x: 1,
                y: 2,
                maximized: false,
                fullscreen: false,
            })
            .await
            .unwrap();
        let saved: nyanpasu_config::state::PersistentState =
            serde_yaml::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.window_state.len(), 1);
        let preserved = evidence(path.as_std_path());
        assert_eq!(preserved.len(), 1);
        assert_eq!(fs::read(&preserved[0]).unwrap(), bad);
        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
    }
}

#[test]
fn session_evidence_is_preserved_before_partial_config_rejection() {
    let (_temp, paths) = fixture();
    let path = paths.session_state_path();
    fs::write(&path, b"window_state: [invalid").unwrap();
    let error = Runner::with_paths(paths.clone(), false).unwrap_err();
    assert!(matches!(
        error,
        crate::core::migration::runner::Error::DetectBaseline { .. }
    ));
    assert_eq!(evidence(path.as_std_path()).len(), 1);
    let _: nyanpasu_config::state::PersistentState =
        serde_yaml::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(
        !paths.application_config_path().exists(),
        "recovery must not invent user configuration"
    );
}

#[test]
fn database_recovers_in_both_migrations_and_storage_construction() {
    for corrupt_at in 0..3 {
        for version in [None, Some(0), Some(2), Some(255)] {
            let (_temp, paths) = fixture();
            if corrupt_at != 0 {
                migrate(&paths);
            }
            let path = paths.storage_path();
            let bad = if let Some(version) = version {
                drop(Storage::try_new(path.as_std_path()).unwrap());
                let mut bytes = fs::read(&path).unwrap();
                // redb's two commit slots begin at 64 and 192. Version is
                // their first byte and is checked before their checksums.
                if version == 0 {
                    bytes.truncate(20);
                } else {
                    bytes[64] = version;
                    bytes[192] = version;
                }
                bytes
            } else {
                b"not a database".to_vec()
            };
            for boundary in 0..3 {
                if boundary == corrupt_at {
                    if boundary == 1 {
                        let state_path = paths
                            .app_config_dir()
                            .join(crate::core::migration::store::STORE_FILE_NAME);
                        let mut store = crate::core::migration::store::MigrationStore::load(
                            state_path.as_std_path(),
                        )
                        .unwrap();
                        store.modules.get_mut("storage").unwrap().applied_revision = 1;
                        store.tasks.retain(|_, task| task.module != "storage");
                        store.flush_atomic(state_path.as_std_path()).unwrap();
                    }
                    fs::write(&path, &bad).unwrap();
                }
                if boundary < 2 {
                    migrate(&paths);
                    if boundary >= corrupt_at {
                        assert_eq!(
                            evidence(path.as_std_path()).len(),
                            1,
                            "migration must recover before storage construction"
                        );
                    }
                }
            }
            let storage = Storage::try_new(path.as_std_path()).unwrap();
            storage.set_item("web:usable", &42).unwrap();
            assert_eq!(
                storage.get_item::<i32>("web:usable").unwrap().value,
                Some(42)
            );
            let preserved = evidence(path.as_std_path());
            assert_eq!(preserved.len(), 1);
            assert_eq!(fs::read(&preserved[0]).unwrap(), bad);
        }
    }
}

#[test]
fn legacy_database_directory_is_preserved_and_rebuilt() {
    let (_temp, paths) = fixture();
    let path = paths.storage_path();
    fs::create_dir(&path).unwrap();
    fs::write(path.join("legacy.sst"), b"legacy rocksdb data").unwrap();
    migrate(&paths);
    migrate(&paths);
    let storage = Storage::try_new(path.as_std_path()).unwrap();
    storage.set_item("usable", &true).unwrap();
    assert_eq!(
        storage.get_item::<bool>("usable").unwrap().value,
        Some(true)
    );
    let preserved = evidence(path.as_std_path());
    assert_eq!(preserved.len(), 1);
    assert_eq!(
        fs::read(preserved[0].join("legacy.sst")).unwrap(),
        b"legacy rocksdb data"
    );
}

#[test]
fn incompatible_kv_table_is_preserved_during_migration() {
    let (_temp, paths) = fixture();
    let path = paths.storage_path();
    {
        let db = redb::Database::create(&path).unwrap();
        let write = db.begin_write().unwrap();
        write
            .open_table(redb::TableDefinition::<u64, u64>::new("clash-nyanpasu"))
            .unwrap()
            .insert(7, 99)
            .unwrap();
        write.commit().unwrap();
    }
    migrate(&paths);
    migrate(&paths);
    let preserved = evidence(path.as_std_path());
    assert_eq!(preserved.len(), 1);
    // Opening a valid redb file can update internal headers. Assert that the
    // evidence still carries the original incompatible table and its value.
    let old = redb::Database::open(&preserved[0]).unwrap();
    let read = old.begin_read().unwrap();
    let table = read
        .open_table(redb::TableDefinition::<u64, u64>::new("clash-nyanpasu"))
        .unwrap();
    assert_eq!(table.get(7).unwrap().unwrap().value(), 99);
    let storage = Storage::try_new(path.as_std_path()).unwrap();
    storage.set_item("usable", &42).unwrap();
    assert_eq!(storage.get_item::<i32>("usable").unwrap().value, Some(42));
}

#[test]
fn pending_legacy_hotkey_migration_rebuilds_database_at_its_write_open() {
    let (_temp, paths) = fixture();
    fs::write(
        paths.nyanpasu_config_path(),
        "hotkeys:\n  - toggle_tun_mode,Control+T\n",
    )
    .unwrap();
    let path = paths.storage_path();
    fs::write(&path, b"not a database").unwrap();
    // Legacy hotkeys make baseline detection stop before opening storage.
    // The migration's write open must apply exactly the same recovery policy.
    migrate(&paths);
    migrate(&paths);
    let preserved = evidence(path.as_std_path());
    assert_eq!(preserved.len(), 1);
    assert_eq!(fs::read(&preserved[0]).unwrap(), b"not a database");
    let application: nyanpasu_config::application::NyanpasuAppConfig =
        serde_yaml::from_slice(&fs::read(paths.application_config_path()).unwrap()).unwrap();
    assert_eq!(
        application.hotkeys,
        vec!["toggle_tun_mode,Control+T".to_owned()]
    );
    let storage = Storage::try_new(path.as_std_path()).unwrap();
    storage.set_item("usable", &42).unwrap();
    assert_eq!(storage.get_item::<i32>("usable").unwrap().value, Some(42));
}

#[cfg(windows)]
#[test]
fn failed_session_evidence_rename_is_typed_and_keeps_original() {
    use std::os::windows::fs::OpenOptionsExt as _;
    let (_temp, paths) = fixture();
    let path = paths.session_state_path();
    let original = b"window_state: [invalid";
    fs::write(&path, original).unwrap();
    // Permit reads, but hold the node against rename/delete. Parsing still
    // reaches the real preservation operation, which must fail without reset.
    const FILE_SHARE_READ: u32 = 1;
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&path)
        .unwrap();
    let result = crate::core::session_state::read(path.as_std_path());
    drop(held);
    assert!(
        matches!(result, Err(crate::core::session_state::Error::PreserveState {
        source: crate::core::corrupt::Error::PreserveEvidence { source, .. }
    }) if matches!(source.raw_os_error(), Some(32 | 33)))
    );
    assert!(evidence(path.as_std_path()).is_empty());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[test]
fn locked_database_is_returned_without_renaming_or_resetting() {
    let (_temp, paths) = fixture();
    let path = paths.storage_path();
    let storage = Storage::try_new(path.as_std_path()).unwrap();
    storage.set_item("kept", &42).unwrap();
    let error = match Storage::try_new(path.as_std_path()) {
        Ok(_) => panic!("a locked database must fail"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        crate::core::storage::StorageOperationError::OpenDatabase {
            source: redb::DatabaseError::DatabaseAlreadyOpen,
            ..
        }
    ));
    assert!(evidence(path.as_std_path()).is_empty());
    assert_eq!(storage.get_item::<i32>("kept").unwrap().value, Some(42));
}

#[cfg(windows)]
#[test]
fn read_only_database_permission_failure_preserves_original() {
    let (_temp, paths) = fixture();
    let path = paths.storage_path();
    drop(Storage::try_new(path.as_std_path()).unwrap());
    let original = fs::read(&path).unwrap();
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    let result = Storage::try_new(path.as_std_path());
    fs::set_permissions(&path, original_permissions).unwrap();
    assert!(
        matches!(result, Err(crate::core::storage::StorageOperationError::OpenDatabase {
        source: redb::DatabaseError::Storage(redb::StorageError::Io(ref error)), ..
    }) if error.kind() == std::io::ErrorKind::PermissionDenied)
    );
    assert!(evidence(path.as_std_path()).is_empty());
    assert_eq!(fs::read(path).unwrap(), original);
}

#[cfg(windows)]
#[tokio::test]
async fn session_acl_permission_failure_is_returned_by_migration_and_client() {
    use std::process::Command;
    let (_temp, paths) = fixture();
    migrate(&paths);
    let path = paths.session_state_path();
    let original = b"window_state: [invalid";
    fs::write(&path, original).unwrap();
    let account = Command::new("whoami").output().unwrap();
    let account = String::from_utf8(account.stdout).unwrap().trim().to_owned();
    let deny = Command::new("icacls")
        .arg(&path)
        .args(["/deny", &format!("{account}:(R)")])
        .output()
        .unwrap();
    if !deny.status.success() {
        eprintln!("SKIPPED session ACL fixture: icacls could not deny read access");
        return;
    }
    struct RestoreAcl {
        path: PathBuf,
        account: String,
    }
    impl Drop for RestoreAcl {
        fn drop(&mut self) {
            let _ = Command::new("icacls")
                .arg(&self.path)
                .args(["/remove:d", &self.account])
                .output();
        }
    }
    let restore = RestoreAcl {
        path: path.clone().into_std_path_buf(),
        account,
    };
    match fs::read(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {}
        _ => {
            eprintln!("SKIPPED session ACL fixture: platform did not enforce denied reads");
            return;
        }
    }
    let migration =
        Runner::with_paths(paths.clone(), false).and_then(|mut runner| runner.run_pending());
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client =
        super::session_state::SessionStateClient::new(path.clone(), shutdown.child_token(), &tasks)
            .await;
    drop(restore);
    let error = migration.unwrap_err();
    assert!(matches!(
        error,
        crate::core::migration::runner::Error::InspectModule { .. }
    ));
    let denied = match &error {
        crate::core::migration::runner::Error::InspectModule { source, .. } => {
            match source.as_ref() {
                crate::core::migration::OperationError::TypedConfig { source } => {
                    match source.as_ref() {
                        crate::core::migration::modules::typed_config::Error::InspectInput {
                            source:
                                crate::core::migration::MigrationCheckError::ReadSessionState {
                                    source,
                                    ..
                                },
                        } => {
                            matches!(source.as_ref(), crate::core::session_state::Error::ReadState { source, .. }
                    if source.kind() == std::io::ErrorKind::PermissionDenied)
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        }
        _ => false,
    };
    assert!(
        denied,
        "migration must retain the typed session permission failure: {error:?}"
    );
    assert!(
        matches!(client, Err(super::session_state::Error::LoadState {
        source: crate::core::session_state::Error::ReadState { source, .. }
    }) if source.kind() == std::io::ErrorKind::PermissionDenied)
    );
    assert!(evidence(path.as_std_path()).is_empty());
    assert_eq!(fs::read(path).unwrap(), original);
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

async fn start_jobs(path: &Path) -> Result<(), crate::client::jobs::Error> {
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let started = crate::client::jobs::start(
        path.to_owned(),
        crate::client::jobs::capture(),
        shutdown.clone(),
        &tasks,
    )
    .await
    .map(drop);
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
    started
}

#[tokio::test]
async fn unusable_jobs_journals_are_preserved_and_rebuilt() {
    const RUNS: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("jobs_runs_v1");
    let temp = tempfile::tempdir().unwrap();
    let foreign = temp.path().join("foreign").join("jobs.redb");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    fs::write(&foreign, vec![b'x'; 4096]).unwrap();
    // Opens cleanly; the record fails to decode while the owner recovers.
    let undecodable = temp.path().join("undecodable").join("jobs.redb");
    fs::create_dir_all(undecodable.parent().unwrap()).unwrap();
    {
        let db = redb::Database::create(&undecodable).unwrap();
        let tx = db.begin_write().unwrap();
        tx.open_table(RUNS)
            .unwrap()
            .insert("run", b"not json".as_slice())
            .unwrap();
        tx.commit().unwrap();
    }
    let directory = temp.path().join("directory").join("jobs.redb");
    fs::create_dir_all(&directory).unwrap();

    for path in [&foreign, &undecodable, &directory] {
        start_jobs(path).await.unwrap();

        assert_eq!(evidence(path).len(), 1, "{}", path.display());
        let db = redb::Database::open(path).unwrap();
        let tx = db.begin_read().unwrap();
        assert!(
            tx.open_table(RUNS).unwrap().get("run").unwrap().is_none(),
            "the rebuilt journal starts empty: {}",
            path.display()
        );
    }
    assert_eq!(fs::read(&evidence(&foreign)[0]).unwrap(), vec![b'x'; 4096]);
    assert!(evidence(&directory)[0].is_dir());
}

#[tokio::test]
async fn a_held_jobs_journal_is_reported_without_rebuilding() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("jobs.redb");
    let _held = redb::Database::create(&path).unwrap();

    let error = start_jobs(&path).await.unwrap_err();

    assert!(
        matches!(&error, crate::client::jobs::Error::OpenStore { .. })
            && nyanpasu_utils::redb_ext::find_failure(&error)
                == Some(nyanpasu_utils::redb_ext::RedbFailure::AlreadyOpen),
        "{error:?}"
    );
    assert!(evidence(&path).is_empty());
}
