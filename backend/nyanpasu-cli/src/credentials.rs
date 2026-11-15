//! CLI token storage: the OS keychain when available, otherwise a `0600` file.
//!
//! A token is never accepted as a command-line argument, so it cannot reach shell
//! history or the process list. It comes from standard input, from `NYANPASU_TOKEN`
//! when a context is added, or from `NYANPASU_TOKEN` at call time.

use std::{fs, io, path::PathBuf};

use crate::{config::TokenBackend, error::CliError};

/// The keychain service name of all CLI tokens.
const KEYRING_SERVICE: &str = "nyanpasu-cli";

/// One place a named secret can live.
pub trait SecretStore: Send + Sync {
    /// Reads a secret, or `None` when it was never stored.
    fn get(&self, name: &str) -> Result<Option<String>, CliError>;
    /// Stores or replaces a secret.
    fn set(&self, name: &str, secret: &str) -> Result<(), CliError>;
    /// Removes a secret; removing an absent one succeeds.
    fn delete(&self, name: &str) -> Result<(), CliError>;
}

/// The OS keychain through the `keyring` crate.
#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
    fn entry(name: &str) -> Result<keyring::Entry, CliError> {
        keyring::Entry::new(KEYRING_SERVICE, name)
            .map_err(|error| CliError::failed(format!("keychain unavailable: {error}")))
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, name: &str) -> Result<Option<String>, CliError> {
        match Self::entry(name)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(CliError::failed(format!("keychain read failed: {error}"))),
        }
    }

    fn set(&self, name: &str, secret: &str) -> Result<(), CliError> {
        Self::entry(name)?
            .set_password(secret)
            .map_err(|error| CliError::failed(format!("keychain write failed: {error}")))
    }

    fn delete(&self, name: &str) -> Result<(), CliError> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(CliError::failed(format!("keychain delete failed: {error}"))),
        }
    }
}

/// A store that refuses every write, for `NYANPASU_NO_KEYRING`: tokens then go to files.
#[derive(Debug, Default)]
pub struct DisabledStore;

impl SecretStore for DisabledStore {
    fn get(&self, _name: &str) -> Result<Option<String>, CliError> {
        Ok(None)
    }

    fn set(&self, _name: &str, _secret: &str) -> Result<(), CliError> {
        Err(CliError::failed("the keychain is disabled"))
    }

    fn delete(&self, _name: &str) -> Result<(), CliError> {
        Ok(())
    }
}

/// `0600` files in a `0700` directory, named after the validated context name.
#[derive(Debug)]
pub struct FileStore {
    directory: PathBuf,
}

impl FileStore {
    /// A store rooted at `directory`, which is created on first write.
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(format!("{name}.token"))
    }
}

#[cfg(unix)]
fn private_directory(directory: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;

    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
}

#[cfg(not(unix))]
fn private_directory(directory: &std::path::Path) -> io::Result<()> {
    // The per-user profile directory is already private to the user on Windows.
    fs::create_dir_all(directory)
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, secret: &str) -> io::Result<()> {
    use std::{
        io::Write as _,
        os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
    };

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // `mode` applies only to a new file; tighten one that already existed.
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(secret.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, secret: &str) -> io::Result<()> {
    fs::write(path, secret)
}

impl SecretStore for FileStore {
    fn get(&self, name: &str) -> Result<Option<String>, CliError> {
        match fs::read_to_string(self.path(name)) {
            Ok(secret) => Ok(Some(secret.trim().to_owned())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(CliError::failed(format!("cannot read token file: {error}"))),
        }
    }

    fn set(&self, name: &str, secret: &str) -> Result<(), CliError> {
        private_directory(&self.directory)
            .and_then(|()| write_private(&self.path(name), secret))
            .map_err(|error| CliError::failed(format!("cannot write token file: {error}")))
    }

    fn delete(&self, name: &str) -> Result<(), CliError> {
        match fs::remove_file(self.path(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(CliError::failed(format!(
                "cannot delete token file: {error}"
            ))),
        }
    }
}

/// Both stores, so a token can prefer the keychain and fall back to a file.
pub struct TokenVault {
    keyring: Box<dyn SecretStore>,
    file: Box<dyn SecretStore>,
}

impl TokenVault {
    /// The production vault: the OS keychain unless `use_keyring` is false, then a file
    /// under `token_dir`.
    pub fn new(token_dir: PathBuf, use_keyring: bool) -> Self {
        let keyring: Box<dyn SecretStore> = if use_keyring {
            Box::new(KeyringStore)
        } else {
            Box::new(DisabledStore)
        };
        Self::with_stores(keyring, Box::new(FileStore::new(token_dir)))
    }

    /// A vault over explicit stores.
    pub fn with_stores(keyring: Box<dyn SecretStore>, file: Box<dyn SecretStore>) -> Self {
        Self { keyring, file }
    }

    /// Stores a token, preferring the keychain, and reports where it went.
    ///
    /// # Errors
    /// Fails when neither store accepts the secret.
    pub fn save(&self, name: &str, token: &str) -> Result<TokenBackend, CliError> {
        // A secret stored earlier elsewhere must not outlive its replacement.
        self.delete(name)?;
        if self.keyring.set(name, token).is_ok() {
            return Ok(TokenBackend::Keyring);
        }
        self.file.set(name, token)?;
        Ok(TokenBackend::File)
    }

    /// Reads the token from the store the context recorded. There is no fallback to
    /// the other store, so a context never resolves a credential it did not choose.
    ///
    /// # Errors
    /// Fails on a store read failure.
    pub fn load(&self, name: &str, backend: TokenBackend) -> Result<Option<String>, CliError> {
        match backend {
            TokenBackend::Keyring => self.keyring.get(name),
            TokenBackend::File => self.file.get(name),
        }
    }

    /// Removes the token from both stores; an unavailable keychain is not an error.
    ///
    /// # Errors
    /// Fails only when the file store cannot delete.
    pub fn delete(&self, name: &str) -> Result<(), CliError> {
        let _ = self.keyring.delete(name);
        self.file.delete(name)
    }
}

/// Normalizes a pasted token: surrounding whitespace is dropped, the rest must be
/// one non-empty word.
///
/// # Errors
/// An empty token or one containing whitespace is a usage error.
pub fn clean_token(raw: &str) -> Result<String, CliError> {
    let token = raw.trim();
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        return Err(CliError::usage("the token is empty or contains whitespace"));
    }
    Ok(token.to_owned())
}

#[cfg(test)]
pub(crate) mod fakes {
    use std::{collections::BTreeMap, sync::Mutex};

    use super::*;

    /// An in-memory store; `available: false` makes writes fail like a missing keychain.
    pub struct MemoryStore {
        pub available: bool,
        pub secrets: Mutex<BTreeMap<String, String>>,
    }

    impl MemoryStore {
        pub fn new(available: bool) -> Self {
            Self {
                available,
                secrets: Mutex::default(),
            }
        }
    }

    impl SecretStore for MemoryStore {
        fn get(&self, name: &str) -> Result<Option<String>, CliError> {
            Ok(self.secrets.lock().unwrap().get(name).cloned())
        }

        fn set(&self, name: &str, secret: &str) -> Result<(), CliError> {
            if !self.available {
                return Err(CliError::failed("unavailable"));
            }
            self.secrets
                .lock()
                .unwrap()
                .insert(name.to_owned(), secret.to_owned());
            Ok(())
        }

        fn delete(&self, name: &str) -> Result<(), CliError> {
            self.secrets.lock().unwrap().remove(name);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fakes::MemoryStore, *};

    #[test]
    fn prefers_the_keyring_and_falls_back_to_a_file() {
        let vault = TokenVault::with_stores(
            Box::new(MemoryStore::new(true)),
            Box::new(MemoryStore::new(true)),
        );
        assert_eq!(vault.save("a", "npk_1_x").unwrap(), TokenBackend::Keyring);
        assert_eq!(
            vault.load("a", TokenBackend::Keyring).unwrap().as_deref(),
            Some("npk_1_x")
        );

        let vault = TokenVault::with_stores(
            Box::new(MemoryStore::new(false)),
            Box::new(MemoryStore::new(true)),
        );
        assert_eq!(vault.save("a", "npk_1_x").unwrap(), TokenBackend::File);
        assert_eq!(
            vault.load("a", TokenBackend::File).unwrap().as_deref(),
            Some("npk_1_x")
        );
    }

    #[test]
    fn load_reads_only_the_recorded_backend() {
        let vault = TokenVault::with_stores(
            Box::new(MemoryStore::new(false)),
            Box::new(MemoryStore::new(true)),
        );
        vault.save("a", "secret").unwrap();
        assert_eq!(vault.load("a", TokenBackend::Keyring).unwrap(), None);
    }

    #[test]
    fn delete_clears_both_stores() {
        let vault = TokenVault::with_stores(
            Box::new(MemoryStore::new(true)),
            Box::new(MemoryStore::new(true)),
        );
        vault.save("a", "secret").unwrap();
        vault.delete("a").unwrap();
        assert_eq!(vault.load("a", TokenBackend::Keyring).unwrap(), None);
        assert_eq!(vault.load("a", TokenBackend::File).unwrap(), None);
    }

    #[test]
    fn the_file_store_round_trips_and_is_owner_only() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileStore::new(directory.path().join("tokens"));
        assert_eq!(store.get("a").unwrap(), None);
        store.set("a", "npk_1_x").unwrap();
        store.set("a", "npk_2_y").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("npk_2_y"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let file_mode = fs::metadata(store.path("a")).unwrap().permissions().mode();
            assert_eq!(file_mode & 0o777, 0o600);
            let directory_mode = fs::metadata(directory.path().join("tokens"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(directory_mode & 0o777, 0o700);
        }

        store.delete("a").unwrap();
        store.delete("a").unwrap();
        assert_eq!(store.get("a").unwrap(), None);
    }

    #[test]
    fn clean_token_rejects_whitespace_and_empty_input() {
        assert_eq!(clean_token(" npk_1_x\n").unwrap(), "npk_1_x");
        assert!(clean_token("").is_err());
        assert!(clean_token("two words").is_err());
    }
}
