use nyanpasu_contracts::auth::{SecretStore, SecretStoreError};

use super::FsSecretStore;

fn store_in(dir: &tempfile::TempDir, max_bytes: usize) -> FsSecretStore {
    FsSecretStore::new(
        dir.path().join("remote-access/auth-secrets.json"),
        max_bytes,
    )
}

#[tokio::test]
async fn load_of_a_missing_record_is_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(store_in(&dir, 1024).load().await.unwrap().is_none());
}

#[tokio::test]
async fn save_then_load_round_trips_and_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir, 1024);

    store.save(b"first").await.unwrap();
    store.save(b"second").await.unwrap();

    assert_eq!(store.load().await.unwrap().unwrap().as_slice(), b"second");
}

#[cfg(unix)]
#[tokio::test]
async fn directory_is_0700_and_file_is_0600() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir, 1024);
    store.save(b"secret").await.unwrap();
    // A second save replaces the file; the replacement must stay private.
    store.save(b"secret-2").await.unwrap();

    let secrets_dir = dir.path().join("remote-access");
    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&secrets_dir), 0o700);
    assert_eq!(mode(&secrets_dir.join("auth-secrets.json")), 0o600);
}

#[tokio::test]
async fn oversized_records_are_rejected_on_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir, 8);

    assert!(matches!(
        store.save(&[0u8; 9]).await,
        Err(SecretStoreError::TooLarge { max_bytes: 8 })
    ));

    store.save(&[1u8; 8]).await.unwrap();
    assert_eq!(store.load().await.unwrap().unwrap().len(), 8);

    // A record that grew past the bound outside this adapter is refused, not truncated.
    let path = dir.path().join("remote-access/auth-secrets.json");
    std::fs::write(&path, [2u8; 9]).unwrap();
    assert!(matches!(
        store.load().await,
        Err(SecretStoreError::TooLarge { max_bytes: 8 })
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_and_non_regular_targets_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir, 1024);
    let secrets_dir = dir.path().join("remote-access");
    let path = secrets_dir.join("auth-secrets.json");
    std::fs::create_dir_all(&secrets_dir).unwrap();

    let outside = dir.path().join("outside");
    std::fs::write(&outside, b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(matches!(
        store.load().await,
        Err(SecretStoreError::NotRegularFile)
    ));
    assert!(matches!(
        store.save(b"x").await,
        Err(SecretStoreError::NotRegularFile)
    ));
    assert_eq!(std::fs::read(&outside).unwrap(), b"outside");

    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(matches!(
        store.save(b"x").await,
        Err(SecretStoreError::NotRegularFile)
    ));
}
