//! Migration and backup integration with core's real storage adapters.

use std::{fs, sync::Arc};

use nyanpasu_contracts::bundle_metadata::TEST_BUNDLE_METADATA;
use nyanpasu_migration::{
    Runner,
    backup::{self, BackupKind, BackupRequest, StorageSource},
    store::{MigrationStore, STORE_FILE_NAME},
    test_support::{EXPECTED_CONFIG, EXPECTED_PROFILES, LEGACY_CONFIG, LEGACY_PROFILES},
};
use nyanpasu_paths::PathResolver;
use semver::Version;

use crate::storage::{RedbHotkeyStoreOpener, Storage, WebStorage as _};

#[test]
fn a_live_storage_is_exported_as_a_consistent_copy() {
    let dir = tempfile::tempdir().unwrap();
    let paths = PathResolver::with_base_dirs(
        dir.path().join("config").try_into().unwrap(),
        dir.path().join("data").try_into().unwrap(),
    );
    fs::create_dir_all(paths.app_config_dir()).unwrap();
    fs::write(paths.profiles_path(), "profiles").unwrap();
    fs::create_dir_all(paths.app_data_dir()).unwrap();
    let storage = Storage::try_new(paths.storage_path().as_std_path()).unwrap();
    storage.set_item("a", &1).unwrap();
    storage.set_item("b", &"two").unwrap();

    let backup = backup::create_backup(&BackupRequest {
        paths: &paths,
        bundle_metadata: TEST_BUNDLE_METADATA,
        storage: StorageSource::Live(&storage),
        kind: BackupKind::Manual,
        now: time::OffsetDateTime::from_unix_timestamp(1_790_000_000).unwrap(),
    })
    .unwrap();

    let exported = Storage::try_new(&backup.path.join("data").join("storage.db")).unwrap();
    assert_eq!(exported.get_all().unwrap(), storage.get_all().unwrap());
    assert_eq!(exported.get_all().unwrap().len(), 2);
    storage.set_item("c", &3).unwrap();
    assert_eq!(storage.get_all().unwrap().len(), 3);
    assert_eq!(exported.get_all().unwrap().len(), 2);
}

fn assert_yaml_shape_eq(path: std::path::PathBuf, expected: &str) {
    let actual_raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let actual: yaml_serde::Value = yaml_serde::from_str(&actual_raw).unwrap();
    let expected: yaml_serde::Value = yaml_serde::from_str(expected).unwrap();
    assert_eq!(
        actual,
        expected,
        "migrated YAML shape mismatch for {}",
        path.display()
    );
}

/// Pins the supported upgrade boundary (1.6.1 -> 2.0): a real 1.6.1 config
/// pair must end up in the exact 2.0 shape every migration step produces.
/// Compares parsed structure (order-independent) rather than raw bytes so the
/// test tracks the data contract, not serializer key ordering.
#[test]
fn real_1_6_1_fixture_migrates_to_2_0_shape() {
    let temp = tempfile::tempdir().unwrap();
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();

    let config_path = config_dir.join("nyanpasu-config.yaml");
    let profiles_path = config_dir.join("profiles.yaml");
    std::fs::write(&config_path, LEGACY_CONFIG).unwrap();
    std::fs::write(&profiles_path, LEGACY_PROFILES).unwrap();

    let paths = PathResolver::with_base_dirs(
        config_dir.clone().try_into().unwrap(),
        data_dir.clone().try_into().unwrap(),
    );
    let mut runner = Runner::with_target(
        Version::parse("2.0.0").unwrap(),
        paths,
        TEST_BUNDLE_METADATA,
        false,
        Arc::new(RedbHotkeyStoreOpener),
    )
    .unwrap();
    runner.run_pending().unwrap();

    assert_yaml_shape_eq(config_path, EXPECTED_CONFIG);

    // The script migration re-prefixes profiles.yaml; the header is part of
    // the on-disk contract, so assert it verbatim before the comment-agnostic
    // shape check.
    let profiles_raw = std::fs::read_to_string(&profiles_path).unwrap();
    assert!(
        profiles_raw.starts_with("# Profiles Config for Clash Nyanpasu\n\n"),
        "profiles.yaml lost its generated header prefix"
    );
    assert_yaml_shape_eq(profiles_path, EXPECTED_PROFILES);

    // Each module advanced to the head of its step list.
    let store = MigrationStore::load(&config_dir.join(STORE_FILE_NAME)).unwrap();
    assert_eq!(store.module_state("profiles").applied_revision, 4);
    assert_eq!(store.module_state("app_config").applied_revision, 4);
    assert_eq!(store.module_state("storage").applied_revision, 2);
    assert_eq!(store.module_state("typed_config").applied_revision, 2);
    assert_eq!(
        store.app.last_succeeded,
        Some(Version::parse("2.0.0").unwrap())
    );

    // Hotkeys left the legacy config file and ended up in the typed
    // application config, with nothing stranded in KV storage.
    let storage = Storage::try_new(&data_dir.join(nyanpasu_paths::STORAGE_DB)).unwrap();
    let hotkeys: Option<Vec<String>> = storage.get_item("hotkeys").unwrap().value;
    assert_eq!(hotkeys, None);

    let application: nyanpasu_config::application::NyanpasuAppConfig = yaml_serde::from_str(
        &std::fs::read_to_string(config_dir.join("application.yaml")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        application.hotkeys,
        vec![
            "clash_mode_rule,Control+Q".to_string(),
            "toggle_system_proxy,Control+Shift+P".to_string(),
        ],
        "the typed application config is the single authority for hotkeys"
    );
    let _: nyanpasu_config::state::PersistentState = yaml_serde::from_str(
        &std::fs::read_to_string(config_dir.join("session-state.yaml")).unwrap(),
    )
    .unwrap();
    let _: nyanpasu_config::clash::config::ClashConfig = yaml_serde::from_str(
        &std::fs::read_to_string(config_dir.join("clash-config.yaml")).unwrap(),
    )
    .unwrap();
}

/// `typed_config` splits the legacy file while `hotkeys` is still in it, so
/// both it and `storage` end up with an opinion about the list. The
/// key-value store is the one the hotkey editor wrote to, so the typed
/// config must end the upgrade holding exactly what `hotkeys_to_kv` parked
/// there.
#[test]
fn full_chain_keeps_the_key_value_hotkeys() {
    let temp = tempfile::tempdir().unwrap();
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();

    let legacy_raw = LEGACY_CONFIG;
    std::fs::write(config_dir.join("nyanpasu-config.yaml"), legacy_raw).unwrap();
    std::fs::write(config_dir.join("profiles.yaml"), LEGACY_PROFILES).unwrap();

    // Read back out of the fixture rather than spelled out here, so the
    // assertion tracks whatever the legacy file actually carries.
    let legacy: yaml_serde::Mapping = yaml_serde::from_str(legacy_raw).unwrap();
    let expected: Vec<String> = legacy
        .get(yaml_serde::Value::String("hotkeys".into()))
        .and_then(|value| value.as_sequence())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect()
        })
        .expect("the 1.6.1 fixture carries hotkeys");
    assert!(!expected.is_empty());

    let paths = PathResolver::with_base_dirs(
        config_dir.clone().try_into().unwrap(),
        data_dir.clone().try_into().unwrap(),
    );
    let mut runner = Runner::with_target(
        Version::parse("2.0.0").unwrap(),
        paths,
        TEST_BUNDLE_METADATA,
        false,
        Arc::new(RedbHotkeyStoreOpener),
    )
    .unwrap();
    runner.run_pending().unwrap();

    let application: nyanpasu_config::application::NyanpasuAppConfig = yaml_serde::from_str(
        &std::fs::read_to_string(config_dir.join("application.yaml")).unwrap(),
    )
    .unwrap();
    assert_eq!(application.hotkeys, expected);
}
