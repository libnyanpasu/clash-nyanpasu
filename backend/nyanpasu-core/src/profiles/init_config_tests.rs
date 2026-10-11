use super::init_config;
use crate::{format::Format, migration::modules::profiles::ProfilesFormat};
use nyanpasu_paths::PathResolver;
use std::fs;

fn paths(root: &std::path::Path) -> PathResolver {
    PathResolver::with_base_dirs(
        root.join("config").try_into().unwrap(),
        root.join("data").try_into().unwrap(),
    )
}

#[test]
fn init_config_creates_exact_default_bytes_and_payload() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    fs::create_dir(paths.app_config_dir()).unwrap();
    init_config(&paths).unwrap();
    let bytes = fs::read(paths.profiles_path()).unwrap();
    assert_eq!(bytes, b"# Clash Nyanpasu\n_nyanpasu:\n  document: profiles\n  schema_revision: 4\nvalid:\n- dns\n- unified-delay\n- tcp-concurrent\nitems: []\n");
    let profiles: nyanpasu_config::profile::Profiles = ProfilesFormat::default()
        .deserialize(bytes.as_slice())
        .unwrap();
    assert_eq!(profiles.revision(), 0);
    assert!(profiles.current.is_none());
    assert!(profiles.global_transforms.is_empty());
    assert_eq!(profiles.valid, ["dns", "unified-delay", "tcp-concurrent"]);
    assert!(profiles.items.is_empty());
    assert_eq!(fs::read_dir(paths.app_config_dir()).unwrap().count(), 1);
    assert!(!paths.app_data_dir().exists());
    init_config(&paths).unwrap();
    assert_eq!(fs::read(paths.profiles_path()).unwrap(), bytes);
}

#[test]
fn init_config_skips_invalid_existing_bytes_and_directory() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    fs::create_dir(paths.app_config_dir()).unwrap();
    let invalid = b"\xffnot stamped yaml: [";
    fs::write(paths.profiles_path(), invalid).unwrap();
    init_config(&paths).unwrap();
    assert_eq!(fs::read(paths.profiles_path()).unwrap(), invalid);
    fs::remove_file(paths.profiles_path()).unwrap();
    fs::create_dir(paths.profiles_path()).unwrap();
    init_config(&paths).unwrap();
    assert!(paths.profiles_path().is_dir());
    assert_eq!(fs::read_dir(paths.profiles_path()).unwrap().count(), 0);
}

fn assert_write_error(paths: &PathResolver) {
    let error = init_config(paths).unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("failed to save file \"{}\"", paths.profiles_path())
    );
    assert!(error.downcast_ref::<std::io::Error>().is_some());
    assert!(!paths.profiles_path().exists());
}

#[test]
fn init_config_missing_parent_fails_without_creating_it() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    assert_write_error(&paths);
    assert!(!paths.app_config_dir().exists());
}

#[test]
fn init_config_regular_file_parent_preserves_error_context() {
    let root = tempfile::tempdir().unwrap();
    let paths = paths(root.path());
    fs::write(paths.app_config_dir(), b"parent").unwrap();
    assert_write_error(&paths);
    assert_eq!(fs::read(paths.app_config_dir()).unwrap(), b"parent");
}
