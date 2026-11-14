//! Unit tests for OS remote-auth clocks, entropy, protected file store, and AEAD cipher.

use std::{fs, sync::Arc};

use nyanpasu_contracts::auth::{AuthClock, EntropySource, SecretCipher, SecretCipherError};

use super::{
    clock_entropy::{OsAuthClock, OsEntropySource},
    keyring_backend::{KeyringBackendError, map_keyring_error},
    secret_cipher::{KeyringSecretCipher, instance_key_account},
    secret_store::{
        FsSecretStore, WindowsSecurityAssessment, classify_ancestor_ace, classify_windows_security,
        collect_ancestor_dirs, owner_ace_grants_rw, win_acl_bits, windows_private_sddl,
    },
    test_support::InMemoryCredentialBackend,
};

#[test]
fn os_clock_and_entropy_produce_monotonic_time_and_distinct_bytes() {
    let clock = OsAuthClock::new();
    let t1 = clock.now_unix_ms();
    let m1 = clock.monotonic_ms();
    let t2 = clock.now_unix_ms();
    let m2 = clock.monotonic_ms();

    assert!(t1 > 1_700_000_000_000);
    assert!(t2 >= t1);
    assert!(m2 >= m1);

    let entropy = OsEntropySource;
    let mut a = [0u8; 32];
    let mut b = [0u8; 32];
    entropy.fill_bytes(&mut a).unwrap();
    entropy.fill_bytes(&mut b).unwrap();
    assert_ne!(a, [0u8; 32]);
    assert_ne!(a, b);
}

#[tokio::test]
async fn cipher_xchacha20poly1305_roundtrip_fresh_nonces_aad_and_tamper() {
    let backend = Arc::new(InMemoryCredentialBackend::new());
    let entropy: Arc<dyn EntropySource> = Arc::new(OsEntropySource);
    let cipher = KeyringSecretCipher::with_default_service(
        "remote-auth-test-instance-1",
        backend.clone(),
        entropy.clone(),
    );

    assert_eq!(backend.call_counts(), (0, 0), "construction must be lazy");

    let aad = b"clash-nyanpasu:remote-auth-secrets:v1";
    let plaintext = br#"{"tokens":["secret-token-value"],"tls":"pem-key"}"#;

    let sealed_a = cipher.seal(plaintext, aad).await.unwrap();
    let sealed_b = cipher.seal(plaintext, aad).await.unwrap();
    assert_ne!(
        sealed_a, sealed_b,
        "every seal must use a fresh 24-byte nonce"
    );
    assert!(!String::from_utf8_lossy(&sealed_a).contains("secret-token-value"));

    let opened = cipher.open(&sealed_a, aad).await.unwrap();
    assert_eq!(opened.expose_bytes(), plaintext);

    // Wrong AAD fails authentication.
    let err_aad = cipher.open(&sealed_a, b"wrong-aad").await.unwrap_err();
    assert!(matches!(err_aad, SecretCipherError::Crypto { .. }));

    // Tampered version header fails.
    let mut bad_magic = sealed_a.clone();
    bad_magic[0] ^= 0xff;
    assert!(matches!(
        cipher.open(&bad_magic, aad).await,
        Err(SecretCipherError::Crypto { .. })
    ));

    // Tampered authentication tag / body fails without leaking key or plaintext.
    let mut bad_tag = sealed_a.clone();
    let last = bad_tag.len() - 1;
    bad_tag[last] ^= 0x01;
    let err_tag = cipher.open(&bad_tag, aad).await.unwrap_err();
    assert!(matches!(err_tag, SecretCipherError::Crypto { .. }));
    assert!(!err_tag.to_string().contains("secret-token-value"));

    // Truncated envelope fails.
    assert!(matches!(
        cipher.open(&sealed_a[..10], aad).await,
        Err(SecretCipherError::Crypto { .. })
    ));

    // New cipher instance with same backend & identity (simulating restart) decrypts cleanly.
    let restarted =
        KeyringSecretCipher::with_default_service("remote-auth-test-instance-1", backend, entropy);
    let reopened = restarted.open(&sealed_b, aad).await.unwrap();
    assert_eq!(reopened.expose_bytes(), plaintext);
}

#[tokio::test]
async fn cipher_never_regenerates_or_overwrites_key_on_missing_or_corrupt_key_failure() {
    let backend = Arc::new(InMemoryCredentialBackend::new());
    let entropy: Arc<dyn EntropySource> = Arc::new(OsEntropySource);
    let account = "remote-auth-test-no-regen";
    let cipher = KeyringSecretCipher::with_default_service(account, backend.clone(), entropy);

    let aad = b"clash-nyanpasu:remote-auth-secrets:v1";
    let sealed = cipher.seal(b"secret-payload", aad).await.unwrap();
    let (_, initial_sets) = backend.call_counts();
    assert_eq!(initial_sets, 1);

    // Missing key on open MUST fail closed and NEVER generate/store a replacement key.
    assert!(backend.remove_entry("clash-nyanpasu.remote-access", account));
    let before_open = backend.call_counts();
    let err = cipher.open(&sealed, aad).await.unwrap_err();
    assert!(matches!(err, SecretCipherError::Unavailable { .. }));
    assert_eq!(backend.call_counts(), (before_open.0 + 1, before_open.1));
    assert!(backend.stored_identities().is_empty());

    // Corrupt key entry MUST fail closed on both open AND seal without overwriting!
    backend.insert_raw_entry("clash-nyanpasu.remote-access", account, "corrupt-not-hex");
    let counts = backend.call_counts();
    assert!(matches!(
        cipher.open(&sealed, aad).await,
        Err(SecretCipherError::Unavailable { .. })
    ));
    assert!(matches!(
        cipher.seal(b"new-secret", aad).await,
        Err(SecretCipherError::Unavailable { .. })
    ));
    assert_eq!(backend.call_counts(), (counts.0 + 2, counts.1));

    // Transient backend read error MUST fail closed without writing.
    backend.set_forced_read_error(Some(KeyringBackendError::Unavailable {
        reason: "simulated daemon down",
    }));
    let counts = backend.call_counts();
    assert!(matches!(
        cipher.seal(b"new-secret", aad).await,
        Err(SecretCipherError::Unavailable { .. })
    ));
    assert_eq!(backend.call_counts(), (counts.0 + 1, counts.1));
}

#[test]
fn distinct_instances_derive_isolated_stable_key_accounts() {
    let dir = tempfile::tempdir().unwrap();
    let inst_a = dir
        .path()
        .join("instance-a/remote-access/auth-secrets.json");
    let inst_b = dir
        .path()
        .join("instance-b/remote-access/auth-secrets.json");

    fs::create_dir_all(dir.path().join("instance-a/other")).unwrap();
    let inst_a_equiv = dir
        .path()
        .join("instance-a/other/../remote-access/auth-secrets.json");

    let acct_a = instance_key_account("test", &inst_a);
    let acct_b = instance_key_account("test", &inst_b);
    let acct_a_equiv = instance_key_account("test", &inst_a_equiv);

    assert_ne!(acct_a, acct_b);
    assert_eq!(acct_a, acct_a_equiv);
}

#[test]
fn cipher_key_account_stable_before_and_after_first_create_with_aliases() {
    let root = tempfile::tempdir().unwrap();
    let canonical_path = root.path().join("data/remote-access/auth-secrets.json");
    let alias_path = root
        .path()
        .join("some/../data/remote-access/auth-secrets.json");

    // First-run before `some/`, `data/`, or `remote-access/` exist anywhere on disk.
    let acct_before_canonical = instance_key_account("remote-access", &canonical_path);
    let acct_before_alias = instance_key_account("remote-access", &alias_path);
    assert_eq!(acct_before_canonical, acct_before_alias);

    // After first creation of the real directories & file (still without creating `some/`).
    fs::create_dir_all(canonical_path.parent().unwrap()).unwrap();
    fs::write(&canonical_path, b"{}").unwrap();
    let acct_after_canonical = instance_key_account("remote-access", &canonical_path);
    let acct_after_alias = instance_key_account("remote-access", &alias_path);

    assert_eq!(acct_before_canonical, acct_after_canonical);
    assert_eq!(acct_before_alias, acct_after_alias);
}

#[cfg(unix)]
#[test]
fn cipher_key_account_resolves_symlink_before_parent_dir_and_preserves_identity() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let target_parent = root.path().join("target_a");
    let target_nested = target_parent.join("nested");
    fs::create_dir_all(&target_nested).unwrap();

    let link_path = root.path().join("link");
    symlink(&target_nested, &link_path).unwrap();

    // `link/../data/...` resolves through `link` (`target_a/nested/..` = `target_a`), NOT `root/data`.
    let via_symlink_alias = link_path.join("../data/remote-access/auth-secrets.json");
    let direct_target_a = target_parent.join("data/remote-access/auth-secrets.json");
    let unrelated_root = root.path().join("data/remote-access/auth-secrets.json");

    let acct_via_symlink_before = instance_key_account("remote-access", &via_symlink_alias);
    let acct_target_a = instance_key_account("remote-access", &direct_target_a);
    let acct_unrelated = instance_key_account("remote-access", &unrelated_root);

    assert_eq!(acct_via_symlink_before, acct_target_a);
    assert_ne!(acct_via_symlink_before, acct_unrelated);

    fs::create_dir_all(direct_target_a.parent().unwrap()).unwrap();
    fs::write(&direct_target_a, b"{}").unwrap();
    let acct_via_symlink_after = instance_key_account("remote-access", &via_symlink_alias);
    assert_eq!(acct_via_symlink_before, acct_via_symlink_after);
}

#[test]
fn windows_security_classification_seam_handles_unsupported_acl_and_rejects_integrity_violations() {
    assert!(
        !classify_windows_security(WindowsSecurityAssessment::UnsupportedAclMedium).unwrap(),
        "non-ACL media (e.g. FAT32/exFAT) must map to Unprotected so encrypted fallback succeeds"
    );
    assert!(
        classify_windows_security(WindowsSecurityAssessment::IntegrityViolation(
            "reparse point rejected"
        ))
        .is_err(),
        "reparse/hardlink integrity violations must still fail closed as Io errors"
    );
    assert!(
        classify_windows_security(WindowsSecurityAssessment::Descriptor {
            owner_is_current_user: true,
            dacl_present: true,
            dacl_protected: true,
            dacl_is_null: false,
            ace_count: 1,
            has_inherited_ace: false,
            has_unsupported_ace: false,
            has_non_owner_allow_ace: false,
            has_effective_owner_rw_allow: true,
        })
        .unwrap()
    );
    assert!(
        !classify_windows_security(WindowsSecurityAssessment::Descriptor {
            owner_is_current_user: true,
            dacl_present: true,
            dacl_protected: true,
            dacl_is_null: false,
            ace_count: 2,
            has_inherited_ace: false,
            has_unsupported_ace: false,
            has_non_owner_allow_ace: true,
            has_effective_owner_rw_allow: true,
        })
        .unwrap()
    );
    assert!(
        classify_windows_security(WindowsSecurityAssessment::Descriptor {
            owner_is_current_user: false,
            dacl_present: true,
            dacl_protected: true,
            dacl_is_null: false,
            ace_count: 1,
            has_inherited_ace: false,
            has_unsupported_ace: false,
            has_non_owner_allow_ace: false,
            has_effective_owner_rw_allow: true,
        })
        .is_err()
    );
}

#[test]
fn windows_ancestor_rights_and_inherit_only_ace_classification() {
    use win_acl_bits::{
        ACCESS_ALLOWED_ACE_TYPE, DELETE, FILE_ALL_ACCESS, FILE_APPEND_DATA, FILE_DELETE_CHILD,
        FILE_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
        FILE_READ_EA, FILE_WRITE_DATA, GENERIC_ALL, GENERIC_EXECUTE, GENERIC_READ, GENERIC_WRITE,
        INHERIT_ONLY_ACE_FLAG, READ_CONTROL, SYNCHRONIZE, WRITE_DAC, WRITE_OWNER,
    };

    // Normal read/list/traverse/read-control/synchronize ACE for Users/Everyone on ancestor MUST NOT fail.
    let users_rx_mask = FILE_GENERIC_READ
        | FILE_EXECUTE
        | READ_CONTROL
        | SYNCHRONIZE
        | FILE_READ_DATA
        | FILE_READ_EA
        | FILE_READ_ATTRIBUTES;
    assert_eq!(
        classify_ancestor_ace(ACCESS_ALLOWED_ACE_TYPE, 0, users_rx_mask, false),
        Ok(false)
    );
    assert_eq!(
        classify_ancestor_ace(
            ACCESS_ALLOWED_ACE_TYPE,
            0,
            GENERIC_READ | GENERIC_EXECUTE,
            false
        ),
        Ok(false)
    );

    // INHERIT_ONLY_ACE (e.g., default CREATOR OWNER GA on root/Users) grants nothing on current ancestor.
    assert_eq!(
        classify_ancestor_ace(
            ACCESS_ALLOWED_ACE_TYPE,
            INHERIT_ONLY_ACE_FLAG,
            GENERIC_ALL | FILE_ALL_ACCESS,
            false
        ),
        Ok(false)
    );

    // Explicit dangerous write/delete/ACL bits on untrusted SID MUST classify unsafe.
    for dangerous_mask in [
        WRITE_DAC,
        WRITE_OWNER,
        DELETE,
        FILE_DELETE_CHILD,
        FILE_WRITE_DATA,
        FILE_APPEND_DATA,
        GENERIC_WRITE,
        GENERIC_ALL,
        FILE_ALL_ACCESS,
    ] {
        assert_eq!(
            classify_ancestor_ace(ACCESS_ALLOWED_ACE_TYPE, 0, dangerous_mask, false),
            Ok(true),
            "mask {dangerous_mask:#x} must be treated as unsafe ancestor right"
        );
        // Same rights for SYSTEM / Administrators / current user stay allowed.
        assert_eq!(
            classify_ancestor_ace(ACCESS_ALLOWED_ACE_TYPE, 0, dangerous_mask, true),
            Ok(false)
        );
    }

    // Non-allow ACEs (e.g., deny=0x01 or object/callback=0x09) fail closed.
    assert!(classify_ancestor_ace(0x01, 0, GENERIC_READ, false).is_err());
    assert!(classify_ancestor_ace(0x09, 0, GENERIC_READ, false).is_err());

    // Owner R/W check must not treat READ_CONTROL alone as R/W via `FILE_ALL_ACCESS` bitmask overlap.
    assert!(!owner_ace_grants_rw(0, READ_CONTROL | SYNCHRONIZE));
    assert!(!owner_ace_grants_rw(INHERIT_ONLY_ACE_FLAG, GENERIC_ALL));
    assert!(owner_ace_grants_rw(0, GENERIC_ALL));
    assert!(owner_ace_grants_rw(0, FILE_ALL_ACCESS));
    assert!(owner_ace_grants_rw(
        0,
        FILE_GENERIC_READ | FILE_GENERIC_WRITE
    ));
}

#[test]
fn windows_root_ancestor_enumeration_and_sddl_explicit_owner_invariants() {
    use std::path::{Path, PathBuf};

    assert_eq!(
        collect_ancestor_dirs(Path::new("/remote-access")).unwrap(),
        vec![PathBuf::from("/")]
    );
    assert_eq!(
        collect_ancestor_dirs(Path::new("/a/b/remote-access")).unwrap(),
        vec![
            PathBuf::from("/"),
            PathBuf::from("/a"),
            PathBuf::from("/a/b"),
        ]
    );

    #[cfg(windows)]
    {
        assert_eq!(
            collect_ancestor_dirs(Path::new(r"C:\remote-access")).unwrap(),
            vec![PathBuf::from(r"C:\")]
        );
        assert_eq!(
            collect_ancestor_dirs(Path::new(r"C:\Users\alice\remote-access")).unwrap(),
            vec![
                PathBuf::from(r"C:\"),
                PathBuf::from(r"C:\Users"),
                PathBuf::from(r"C:\Users\alice"),
            ]
        );
        assert_eq!(
            collect_ancestor_dirs(Path::new(r"\\server\share\remote-access")).unwrap(),
            vec![PathBuf::from(r"\\server\share\")]
        );
        assert_eq!(
            collect_ancestor_dirs(Path::new(r"C:remote-access")).unwrap(),
            Vec::<PathBuf>::new()
        );
    }

    assert_eq!(
        windows_private_sddl("S-1-5-21-1-2-3-1001", true),
        "O:S-1-5-21-1-2-3-1001D:P(A;OICI;GA;;;S-1-5-21-1-2-3-1001)"
    );
    assert_eq!(
        windows_private_sddl("S-1-5-21-1-2-3-1001", false),
        "O:S-1-5-21-1-2-3-1001D:P(A;;GA;;;S-1-5-21-1-2-3-1001)"
    );
}

#[test]
fn keyring_error_mapping_redacts_raw_secret_bytes() {
    let sentinel = b"RAW_KEYRING_SECRET_SENTINEL_12345";
    let errors = [
        keyring_core::Error::BadEncoding(sentinel.to_vec()),
        keyring_core::Error::BadDataFormat(sentinel.to_vec(), "parse error".to_owned().into()),
        keyring_core::Error::NoEntry,
    ];
    let sentinel_str = std::str::from_utf8(sentinel).unwrap();
    for err in errors {
        let mapped = map_keyring_error(err);
        let debug = format!("{mapped:?}");
        let display = format!("{mapped}");
        assert!(!debug.contains(sentinel_str));
        assert!(!display.contains(sentinel_str));
    }
}

#[cfg(unix)]
mod unix_tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use nyanpasu_contracts::auth::{SecretStore, SecretStoreError, StorageProtection};

    use super::FsSecretStore;

    fn set_mode(path: &std::path::Path, mode: u32) {
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(mode);
        fs::set_permissions(path, perms).unwrap();
    }

    #[tokio::test]
    async fn unix_protected_plaintext_roundtrip_creates_0700_dir_and_0600_file_lazily() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();
        set_mode(&data_dir, 0o700);

        let secrets_path = data_dir.join("remote-access/auth-secrets.json");
        let store = FsSecretStore::new(secrets_path.clone(), 4096);

        assert_eq!(
            store.protection().await.unwrap(),
            StorageProtection::Protected
        );
        assert!(store.load().await.unwrap().is_none());
        assert!(
            !secrets_path.parent().unwrap().exists(),
            "read-only probe/load must not pre-create directory"
        );

        let payload = br#"{"plain_v1":{"tokens":["abc"]}}"#;
        store.save_protected_plaintext(payload).await.unwrap();

        let dir_mode = fs::metadata(secrets_path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&secrets_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);

        let loaded = store.load().await.unwrap().unwrap();
        assert_eq!(loaded.protection, StorageProtection::Protected);
        assert_eq!(loaded.bytes.expose_bytes(), payload);

        let updated = br#"{"plain_v1":{"tokens":["def"]}}"#;
        store.save_protected_plaintext(updated).await.unwrap();
        assert_eq!(
            store.load().await.unwrap().unwrap().bytes.expose_bytes(),
            updated
        );
    }

    #[tokio::test]
    async fn unix_relaxed_dir_is_never_silently_chmoded_and_refuses_plaintext() {
        let root = tempfile::tempdir().unwrap();
        let private_dir = root.path().join("data/remote-access");
        fs::create_dir_all(&private_dir).unwrap();
        set_mode(&private_dir, 0o755);

        let secrets_path = private_dir.join("auth-secrets.json");
        let store = FsSecretStore::new(secrets_path.clone(), 4096);

        assert_eq!(
            store.protection().await.unwrap(),
            StorageProtection::Unprotected
        );
        let err = store
            .save_protected_plaintext(b"MUST_NOT_WRITE_PLAINTEXT")
            .await
            .unwrap_err();
        assert_eq!(err, SecretStoreError::UnprotectedMedium);
        assert!(!secrets_path.exists());
        assert_eq!(
            fs::metadata(&private_dir).unwrap().permissions().mode() & 0o777,
            0o755,
            "relaxed directory permissions must never be silently rewritten"
        );

        // Encrypted envelope persistence works on relaxed user-owned paths.
        store
            .save_encrypted(b"AEAD_CIPHERTEXT_ENVELOPE")
            .await
            .unwrap();
        let loaded = store.load().await.unwrap().unwrap();
        assert_eq!(loaded.protection, StorageProtection::Unprotected);
        assert_eq!(loaded.bytes.expose_bytes(), b"AEAD_CIPHERTEXT_ENVELOPE");
    }

    #[tokio::test]
    async fn unix_downgraded_file_refuses_plaintext_and_preserves_prior_record() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();
        set_mode(&data_dir, 0o700);
        let secrets_path = data_dir.join("remote-access/auth-secrets.json");
        let store = FsSecretStore::new(secrets_path.clone(), 4096);

        store
            .save_protected_plaintext(b"ORIGINAL_RECORD")
            .await
            .unwrap();
        set_mode(&secrets_path, 0o644);

        let loaded = store.load().await.unwrap().unwrap();
        assert_eq!(loaded.protection, StorageProtection::Unprotected);
        assert_eq!(loaded.bytes.expose_bytes(), b"ORIGINAL_RECORD");

        let err = store
            .save_protected_plaintext(b"OVERWRITE_PLAINTEXT")
            .await
            .unwrap_err();
        assert_eq!(err, SecretStoreError::UnprotectedMedium);
        assert_eq!(fs::read(&secrets_path).unwrap(), b"ORIGINAL_RECORD");
        assert_eq!(
            fs::metadata(&secrets_path).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    #[tokio::test]
    async fn unix_world_writable_non_sticky_ancestor_refuses_plaintext() {
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("world_writable");
        fs::create_dir_all(&shared).unwrap();
        set_mode(&shared, 0o777);

        let secrets_path = shared.join("app/remote-access/auth-secrets.json");
        let store = FsSecretStore::new(secrets_path.clone(), 4096);

        assert_eq!(
            store.protection().await.unwrap(),
            StorageProtection::Unprotected
        );
        assert_eq!(
            store.save_protected_plaintext(b"SECRET").await.unwrap_err(),
            SecretStoreError::UnprotectedMedium
        );
        assert!(!secrets_path.exists());
    }

    #[tokio::test]
    async fn unix_symlinks_and_hardlinks_are_rejected_without_touching_targets() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("data");
        let private_dir = data_dir.join("remote-access");
        fs::create_dir_all(&private_dir).unwrap();
        set_mode(&data_dir, 0o700);
        set_mode(&private_dir, 0o700);

        let innocent = root.path().join("innocent.txt");
        fs::write(&innocent, b"INNOCENT").unwrap();
        set_mode(&innocent, 0o600);

        // 1. Symlink at secret file path.
        let secrets_path = private_dir.join("auth-secrets.json");
        std::os::unix::fs::symlink(&innocent, &secrets_path).unwrap();
        let store = FsSecretStore::new(secrets_path.clone(), 4096);

        assert!(matches!(
            store.load().await,
            Err(SecretStoreError::Io { .. })
        ));
        assert!(matches!(
            store.save_protected_plaintext(b"X").await,
            Err(SecretStoreError::Io { .. })
        ));
        assert!(matches!(
            store.save_encrypted(b"X").await,
            Err(SecretStoreError::Io { .. })
        ));
        assert_eq!(fs::read(&innocent).unwrap(), b"INNOCENT");
        fs::remove_file(&secrets_path).unwrap();

        // 2. Hardlink at secret file path (nlink == 2).
        fs::write(&secrets_path, b"HARDLINK_TARGET").unwrap();
        set_mode(&secrets_path, 0o600);
        let extra_link = root.path().join("extra-link");
        fs::hard_link(&secrets_path, &extra_link).unwrap();

        assert!(matches!(
            store.load().await,
            Err(SecretStoreError::Io { .. })
        ));
        assert!(matches!(
            store.save_protected_plaintext(b"NEW").await,
            Err(SecretStoreError::Io { .. })
        ));
        assert_eq!(fs::read(&extra_link).unwrap(), b"HARDLINK_TARGET");
        fs::remove_file(&extra_link).unwrap();
        fs::remove_file(&secrets_path).unwrap();

        // 3. Non-root symlink in ancestor chain.
        let real_parent = root.path().join("real_parent");
        fs::create_dir_all(&real_parent).unwrap();
        set_mode(&real_parent, 0o700);
        let link_parent = root.path().join("link_parent");
        std::os::unix::fs::symlink(&real_parent, &link_parent).unwrap();
        let store_via_symlink =
            FsSecretStore::new(link_parent.join("remote-access/auth-secrets.json"), 4096);
        assert!(matches!(
            store_via_symlink.save_protected_plaintext(b"X").await,
            Err(SecretStoreError::Io { .. })
        ));
    }

    #[tokio::test]
    async fn unix_max_record_bytes_bounded_on_write_and_load_preserving_prior_record() {
        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("data");
        fs::create_dir_all(&data_dir).unwrap();
        set_mode(&data_dir, 0o700);
        let secrets_path = data_dir.join("remote-access/auth-secrets.json");

        let store = FsSecretStore::new(secrets_path.clone(), 64);
        store
            .save_protected_plaintext(b"SMALL_VALID_RECORD")
            .await
            .unwrap();

        let oversized = vec![b'a'; 65];
        assert!(matches!(
            store.save_protected_plaintext(&oversized).await,
            Err(SecretStoreError::Io { .. })
        ));
        assert!(matches!(
            store.save_encrypted(&oversized).await,
            Err(SecretStoreError::Io { .. })
        ));
        assert_eq!(fs::read(&secrets_path).unwrap(), b"SMALL_VALID_RECORD");

        // Externally oversized file must be refused on load without unbounded read.
        fs::write(&secrets_path, oversized).unwrap();
        set_mode(&secrets_path, 0o600);
        assert!(matches!(
            store.load().await,
            Err(SecretStoreError::Io { .. })
        ));
    }

    #[tokio::test]
    async fn unix_fifo_target_is_rejected_promptly_without_blocking() {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};

        let root = tempfile::tempdir().unwrap();
        let data_dir = root.path().join("data");
        let ra_dir = data_dir.join("remote-access");
        fs::create_dir_all(&ra_dir).unwrap();
        set_mode(&data_dir, 0o700);
        set_mode(&ra_dir, 0o700);

        let fifo_path = ra_dir.join("auth-secrets.json");
        let c_path = CString::new(fifo_path.as_os_str().as_bytes()).unwrap();
        // SAFETY: Null-terminated valid C path in a private temp directory.
        let rc = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
        assert_eq!(
            rc,
            0,
            "mkfifo failed: {:?}",
            std::io::Error::last_os_error()
        );

        let store = FsSecretStore::new(fifo_path, 4096);
        let load_res = tokio::time::timeout(std::time::Duration::from_secs(2), store.load())
            .await
            .expect("FIFO load must fail promptly via O_NONBLOCK rather than blocking");
        assert!(matches!(load_res, Err(SecretStoreError::Io { .. })));

        let prot_res = tokio::time::timeout(std::time::Duration::from_secs(2), store.protection())
            .await
            .expect("FIFO protection probe must fail promptly rather than blocking");
        assert!(matches!(prot_res, Err(SecretStoreError::Io { .. })));
    }

    #[tokio::test]
    async fn unix_missing_dotdot_into_relaxed_ancestor_refuses_without_writing_temp_plaintext() {
        let root = tempfile::tempdir().unwrap();
        let public_dir = root.path().join("public-parent");
        fs::create_dir_all(&public_dir).unwrap();
        set_mode(&public_dir, 0o777);

        // `some` does not exist, so `some/../public-parent/remote-access/auth-secrets.json`
        // resolves lexically into `public-parent` (mode 0755, group/world accessible via parent).
        let aliased_path = root
            .path()
            .join("some/../public-parent/remote-access/auth-secrets.json");
        let store = FsSecretStore::new(aliased_path, 4096);

        assert_eq!(
            store.protection().await.unwrap(),
            StorageProtection::Unprotected
        );
        assert_eq!(
            store
                .save_protected_plaintext(b"MUST_NEVER_HIT_DISK_IN_PLAINTEXT")
                .await
                .unwrap_err(),
            SecretStoreError::UnprotectedMedium
        );
        assert!(
            !public_dir.join("remote-access").exists(),
            "refused protected plaintext write must not create remote-access or any temp file"
        );
        let entries: Vec<_> = fs::read_dir(&public_dir).unwrap().collect();
        assert!(entries.is_empty());
    }
}

#[cfg(windows)]
mod windows_tests {
    use std::fs;

    use nyanpasu_contracts::auth::{SecretStore, SecretStoreError, StorageProtection};

    use super::{
        super::windows_store::test_helpers::{
            apply_sddl_to_dir, dir_ancestor_acl_is_safe, path_ancestors_are_safe,
            verify_created_private_objects_owned_by_token_user,
        },
        FsSecretStore,
    };

    #[tokio::test]
    async fn windows_protected_dacl_roundtrip_and_relaxed_everyone_refusal() {
        let root = tempfile::tempdir().unwrap();
        let direct_private = root.path().join("explicit_owner_dir");
        let direct_temp = direct_private.join("auth-secrets.probe.tmp");
        verify_created_private_objects_owned_by_token_user(&direct_private, &direct_temp);

        let protected_path = root
            .path()
            .join("protected/remote-access/auth-secrets.json");
        let protected_store = FsSecretStore::new(protected_path.clone(), 4096);

        if path_ancestors_are_safe(protected_path.parent().unwrap()) {
            assert_eq!(
                protected_store.protection().await.unwrap(),
                StorageProtection::Protected
            );
            protected_store
                .save_protected_plaintext(b"WIN_PROTECTED_RECORD")
                .await
                .unwrap();
            let loaded = protected_store.load().await.unwrap().unwrap();
            assert_eq!(loaded.protection, StorageProtection::Protected);
            assert_eq!(loaded.bytes.expose_bytes(), b"WIN_PROTECTED_RECORD");

            // Hardlinked existing target must be refused.
            let link_alias = root
                .path()
                .join("protected/remote-access/hardlink-alias.json");
            fs::hard_link(&protected_path, &link_alias).unwrap();
            assert!(matches!(
                protected_store.load().await,
                Err(SecretStoreError::Io { .. })
            ));
        } else {
            // Host root/temp ancestors on this runner already grant untrusted write/append
            // rights (e.g. default drive root ACE): store MUST classify Unprotected and refuse
            // plaintext without weakening ancestor checks.
            assert_eq!(
                protected_store.protection().await.unwrap(),
                StorageProtection::Unprotected
            );
            assert_eq!(
                protected_store
                    .save_protected_plaintext(b"MUST_REFUSE_UNSAFE_ROOT")
                    .await
                    .unwrap_err(),
                SecretStoreError::UnprotectedMedium
            );
        }

        // Pre-existing directory granting Everyone access MUST classify Unprotected, refuse
        // plaintext without rewriting ACL, and allow encrypted writes.
        let relaxed_dir = root.path().join("relaxed/remote-access");
        fs::create_dir_all(&relaxed_dir).unwrap();
        apply_sddl_to_dir(&relaxed_dir, "D:P(A;OICI;GA;;;WD)", true);

        let relaxed_path = relaxed_dir.join("auth-secrets.json");
        let relaxed_store = FsSecretStore::new(relaxed_path.clone(), 4096);
        assert_eq!(
            relaxed_store.protection().await.unwrap(),
            StorageProtection::Unprotected
        );
        assert_eq!(
            relaxed_store
                .save_protected_plaintext(b"MUST_REFUSE")
                .await
                .unwrap_err(),
            SecretStoreError::UnprotectedMedium
        );
        assert!(!relaxed_path.exists());

        relaxed_store
            .save_encrypted(b"WIN_ENCRYPTED_RECORD")
            .await
            .unwrap();
        let loaded_relaxed = relaxed_store.load().await.unwrap().unwrap();
        assert_eq!(loaded_relaxed.protection, StorageProtection::Unprotected);
        assert_eq!(loaded_relaxed.bytes.expose_bytes(), b"WIN_ENCRYPTED_RECORD");

        // Non-NULL empty DACL denies everyone and MUST NOT report Protected.
        let empty_dacl_dir = root.path().join("empty_dacl/remote-access");
        fs::create_dir_all(&empty_dacl_dir).unwrap();
        apply_sddl_to_dir(&empty_dacl_dir, "D:P", true);
        let empty_store = FsSecretStore::new(empty_dacl_dir.join("auth-secrets.json"), 4096);
        assert_eq!(
            empty_store.protection().await.unwrap(),
            StorageProtection::Unprotected
        );

        // Relaxed ancestor (even before leaf exists) MUST refuse plaintext without creating temp/leaf.
        let relaxed_parent = root.path().join("relaxed_ancestor");
        fs::create_dir_all(&relaxed_parent).unwrap();
        apply_sddl_to_dir(&relaxed_parent, "D:P(A;OICI;GA;;;WD)", true);
        let nested_under_relaxed = relaxed_parent.join("remote-access/auth-secrets.json");
        let nested_store = FsSecretStore::new(nested_under_relaxed.clone(), 4096);
        assert_eq!(
            nested_store.protection().await.unwrap(),
            StorageProtection::Unprotected
        );
        assert_eq!(
            nested_store
                .save_protected_plaintext(b"MUST_REFUSE_RELAXED_ANCESTOR")
                .await
                .unwrap_err(),
            SecretStoreError::UnprotectedMedium
        );
        assert!(!relaxed_parent.join("remote-access").exists());
    }

    #[tokio::test]
    async fn windows_realistic_root_users_rx_and_inherit_only_creator_owner_protected_and_write_dac_rejected()
     {
        use super::super::windows_store::test_helpers::current_user_sid_string;

        let root = tempfile::tempdir().unwrap();
        let user_sid = current_user_sid_string();

        // Simulate standard Windows ancestor DACL: SYSTEM/BA/Owner full + Users Read&Execute +
        // CREATOR OWNER Inherit-Only GenericAll. Must evaluate safe on its own handle.
        let realistic_root = root.path().join("users_root");
        fs::create_dir_all(&realistic_root).unwrap();
        let realistic_sddl = format!(
            "D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)(A;OICI;GA;;;{user_sid})(A;;0x1200a9;;;BU)(A;OICIIO;GA;;;CO)"
        );
        apply_sddl_to_dir(&realistic_root, &realistic_sddl, true);
        assert!(dir_ancestor_acl_is_safe(&realistic_root));

        let protected_target = realistic_root.join("AppData/remote-access/auth-secrets.json");
        let protected_store = FsSecretStore::new(protected_target.clone(), 4096);
        if path_ancestors_are_safe(protected_target.parent().unwrap()) {
            assert_eq!(
                protected_store.protection().await.unwrap(),
                StorageProtection::Protected
            );
            protected_store
                .save_protected_plaintext(b"WIN_USERS_RX_CO_IO_OK")
                .await
                .unwrap();
            let loaded = protected_store.load().await.unwrap().unwrap();
            assert_eq!(loaded.protection, StorageProtection::Protected);
            assert_eq!(loaded.bytes.expose_bytes(), b"WIN_USERS_RX_CO_IO_OK");
        } else {
            assert_eq!(
                protected_store.protection().await.unwrap(),
                StorageProtection::Unprotected
            );
        }

        // Granting Builtin\Users WRITE_DAC | FILE_DELETE_CHILD (`0x00040040`) on ancestor MUST refuse plaintext.
        let unsafe_ancestor = root.path().join("users_write_dac_ancestor");
        fs::create_dir_all(&unsafe_ancestor).unwrap();
        let unsafe_sddl =
            format!("D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;{user_sid})(A;;0x00040040;;;BU)");
        apply_sddl_to_dir(&unsafe_ancestor, &unsafe_sddl, true);
        assert!(!dir_ancestor_acl_is_safe(&unsafe_ancestor));
        let refused_store = FsSecretStore::new(
            unsafe_ancestor.join("remote-access/auth-secrets.json"),
            4096,
        );
        assert_eq!(
            refused_store.protection().await.unwrap(),
            StorageProtection::Unprotected
        );
        assert_eq!(
            refused_store
                .save_protected_plaintext(b"MUST_REFUSE_WRITE_DAC_ANCESTOR")
                .await
                .unwrap_err(),
            SecretStoreError::UnprotectedMedium
        );
    }
}
