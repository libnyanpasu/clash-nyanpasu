//! In-process test credential backend for `KeyringSecretCipher` without accessing real OS keychains.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use zeroize::Zeroizing;

use super::keyring_backend::{CredentialSecretBackend, KeyringBackendError};

/// Deterministic in-memory [`CredentialSecretBackend`] for tests.
///
/// Supports read/write persistence across cipher restarts as well as injected unavailability or
/// entry corruption failures, while never touching the host user's OS keyring.
#[derive(Debug, Default, Clone)]
pub struct InMemoryCredentialBackend {
    state: Arc<Mutex<BackendState>>,
}

#[derive(Debug, Default)]
struct BackendState {
    entries: BTreeMap<(String, String), String>,
    forced_read_error: Option<KeyringBackendError>,
    forced_write_error: Option<KeyringBackendError>,
    get_calls: usize,
    set_calls: usize,
}

impl InMemoryCredentialBackend {
    /// Construct an empty in-memory credential backend.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Force subsequent `get_key_hex` calls to fail with `error` (`None` clears).
    pub fn set_forced_read_error(&self, error: Option<KeyringBackendError>) {
        self.state.lock().unwrap().forced_read_error = error;
    }

    /// Force subsequent `set_key_hex` calls to fail with `error` (`None` clears).
    pub fn set_forced_write_error(&self, error: Option<KeyringBackendError>) {
        self.state.lock().unwrap().forced_write_error = error;
    }

    /// Overwrite or corrupt a raw entry directly for testing fail-closed behavior.
    pub fn insert_raw_entry(
        &self,
        service: impl Into<String>,
        account: impl Into<String>,
        raw_value: impl Into<String>,
    ) {
        self.state
            .lock()
            .unwrap()
            .entries
            .insert((service.into(), account.into()), raw_value.into());
    }

    /// Remove a stored entry to simulate a missing key after encryption.
    pub fn remove_entry(&self, service: &str, account: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .entries
            .remove(&(service.to_owned(), account.to_owned()))
            .is_some()
    }

    /// Number of `get_key_hex` + `set_key_hex` backend calls observed.
    #[must_use]
    pub fn call_counts(&self) -> (usize, usize) {
        let guard = self.state.lock().unwrap();
        (guard.get_calls, guard.set_calls)
    }

    /// Snapshot all currently stored `(service, account)` keys.
    #[must_use]
    pub fn stored_identities(&self) -> Vec<(String, String)> {
        self.state.lock().unwrap().entries.keys().cloned().collect()
    }
}

#[async_trait]
impl CredentialSecretBackend for InMemoryCredentialBackend {
    async fn get_key_hex(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Zeroizing<String>, KeyringBackendError> {
        let mut guard = self.state.lock().unwrap();
        guard.get_calls += 1;
        if let Some(err) = guard.forced_read_error.clone() {
            return Err(err);
        }
        guard
            .entries
            .get(&(service.to_owned(), account.to_owned()))
            .cloned()
            .map(Zeroizing::new)
            .ok_or(KeyringBackendError::NotFound)
    }

    async fn set_key_hex(
        &self,
        service: &str,
        account: &str,
        hex_secret: &str,
    ) -> Result<(), KeyringBackendError> {
        let mut guard = self.state.lock().unwrap();
        guard.set_calls += 1;
        if let Some(err) = guard.forced_write_error.clone() {
            return Err(err);
        }
        guard.entries.insert(
            (service.to_owned(), account.to_owned()),
            hex_secret.to_owned(),
        );
        Ok(())
    }
}
