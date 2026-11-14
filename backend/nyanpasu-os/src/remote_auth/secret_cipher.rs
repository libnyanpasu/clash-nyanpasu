//! `SecretCipher` implementation backed by XChaCha20-Poly1305 and per-instance OS keyring key.

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Key, KeyInit, Payload},
};
use nyanpasu_contracts::auth::{EntropySource, SecretBytes, SecretCipher, SecretCipherError};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use super::keyring_backend::{CredentialSecretBackend, KeyringBackendError};

const DEFAULT_KEYRING_SERVICE: &str = "clash-nyanpasu.remote-access";
const SEALED_MAGIC_V1: &[u8; 4] = b"NRA1";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 24;
const TAG_BYTES: usize = 16;
const MIN_SEALED_LEN: usize = SEALED_MAGIC_V1.len() + NONCE_BYTES + TAG_BYTES;

/// Derive a deterministic, per-instance credential account identifier from an installation path.
///
/// Separate data directories / test installations obtain distinct credential entries without
/// writing key identifiers into unprotected config files.
#[must_use]
pub fn instance_key_account(scope: &str, secrets_path: &Path) -> String {
    let normalized = normalize_instance_path(secrets_path);
    let mut hasher = Sha256::new();
    hasher.update(b"clash-nyanpasu:remote-auth-key:v1:");
    hasher.update(scope.as_bytes());
    hasher.update(b":");
    hasher.update(normalized.as_bytes());
    let digest = hasher.finalize();
    format!("remote-auth-v1-{}", &hex::encode(digest)[..24])
}

fn normalize_instance_path(path: &Path) -> String {
    let raw_abs = if path.is_absolute() {
        path.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(path)
    } else {
        path.to_path_buf()
    };

    // Canonicalize the deepest existing raw prefix BEFORE collapsing missing `..` components,
    // so symlinks like `/var/../app` on macOS resolve through the real symlink target first.
    let components: Vec<std::path::Component<'_>> = raw_abs.components().collect();
    let mut split_idx = 0usize;
    let mut base = std::path::PathBuf::new();
    for idx in (1..=components.len()).rev() {
        let prefix: std::path::PathBuf = components[..idx].iter().collect();
        if let Ok(canon) = dunce::canonicalize(&prefix) {
            base = canon;
            split_idx = idx;
            break;
        }
    }

    for comp in &components[split_idx..] {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if matches!(
                    base.components().next_back(),
                    Some(std::path::Component::Normal(_))
                ) {
                    base.pop();
                } else {
                    base.push(comp);
                }
            }
            other => base.push(other),
        }
    }
    base.to_string_lossy().into_owned()
}

/// Authenticated cipher wrapping [`CredentialSecretBackend`] with `XChaCha20Poly1305`.
///
/// Key retrieval is lazy: constructing the cipher does not touch the OS credential store, so
/// owner-protected deployments never trigger keychain prompts or errors when plaintext protected
/// persistence succeeds.
pub struct KeyringSecretCipher {
    service: String,
    account: String,
    backend: Arc<dyn CredentialSecretBackend>,
    entropy: Arc<dyn EntropySource>,
    key_lock: Mutex<()>,
}

impl std::fmt::Debug for KeyringSecretCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyringSecretCipher")
            .field("service", &self.service)
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

impl KeyringSecretCipher {
    /// Construct a cipher bound to an explicit `(service, account)` identity and injected backend.
    #[must_use]
    pub fn new(
        service: impl Into<String>,
        account: impl Into<String>,
        backend: Arc<dyn CredentialSecretBackend>,
        entropy: Arc<dyn EntropySource>,
    ) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
            backend,
            entropy,
            key_lock: Mutex::new(()),
        }
    }

    /// Construct a cipher using the default application keyring service name.
    #[must_use]
    pub fn with_default_service(
        account: impl Into<String>,
        backend: Arc<dyn CredentialSecretBackend>,
        entropy: Arc<dyn EntropySource>,
    ) -> Self {
        Self::new(DEFAULT_KEYRING_SERVICE, account, backend, entropy)
    }

    async fn load_existing_key(&self) -> Result<Zeroizing<[u8; KEY_BYTES]>, SecretCipherError> {
        let _guard = self.key_lock.lock().await;
        match self.backend.get_key_hex(&self.service, &self.account).await {
            Ok(encoded) => decode_key_hex(&encoded),
            Err(KeyringBackendError::NotFound) => Err(SecretCipherError::Unavailable {
                reason: "encryption key missing from OS credential store".to_owned(),
            }),
            Err(KeyringBackendError::InvalidStoredFormat { reason }) => {
                Err(SecretCipherError::Unavailable {
                    reason: reason.to_owned(),
                })
            }
            Err(KeyringBackendError::Unavailable { reason }) => {
                Err(SecretCipherError::Unavailable {
                    reason: reason.to_owned(),
                })
            }
        }
    }

    async fn load_or_create_key(&self) -> Result<Zeroizing<[u8; KEY_BYTES]>, SecretCipherError> {
        let _guard = self.key_lock.lock().await;
        match self.backend.get_key_hex(&self.service, &self.account).await {
            Ok(encoded) => decode_key_hex(&encoded),
            Err(KeyringBackendError::NotFound) => {
                let mut key = Zeroizing::new([0u8; KEY_BYTES]);
                self.entropy.fill_bytes(key.as_mut()).map_err(|err| {
                    SecretCipherError::Unavailable {
                        reason: err.to_string(),
                    }
                })?;
                let encoded = Zeroizing::new(hex::encode(key.as_ref()));
                self.backend
                    .set_key_hex(&self.service, &self.account, &encoded)
                    .await
                    .map_err(|err| SecretCipherError::Unavailable {
                        reason: err.to_string(),
                    })?;
                Ok(key)
            }
            Err(KeyringBackendError::InvalidStoredFormat { reason }) => {
                Err(SecretCipherError::Unavailable {
                    reason: reason.to_owned(),
                })
            }
            Err(KeyringBackendError::Unavailable { reason }) => {
                Err(SecretCipherError::Unavailable {
                    reason: reason.to_owned(),
                })
            }
        }
    }
}

fn decode_key_hex(encoded: &str) -> Result<Zeroizing<[u8; KEY_BYTES]>, SecretCipherError> {
    let trimmed = encoded.trim();
    let decoded =
        Zeroizing::new(
            hex::decode(trimmed).map_err(|_| SecretCipherError::Unavailable {
                reason: "OS credential store key encoding is malformed".to_owned(),
            })?,
        );
    if decoded.len() != KEY_BYTES {
        return Err(SecretCipherError::Unavailable {
            reason: "OS credential store key has invalid byte length".to_owned(),
        });
    }
    let mut key = Zeroizing::new([0u8; KEY_BYTES]);
    key.copy_from_slice(&decoded);
    Ok(key)
}

#[async_trait]
impl SecretCipher for KeyringSecretCipher {
    async fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, SecretCipherError> {
        let key_bytes = self.load_or_create_key().await?;
        let cipher = XChaCha20Poly1305::new(&Key::<XChaCha20Poly1305>::from(*key_bytes));

        let mut nonce_bytes = [0u8; NONCE_BYTES];
        self.entropy.fill_bytes(&mut nonce_bytes).map_err(|err| {
            SecretCipherError::Unavailable {
                reason: err.to_string(),
            }
        })?;
        let nonce = XNonce::from(nonce_bytes);

        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| SecretCipherError::Crypto {
                reason: "AEAD encryption failed".to_owned(),
            })?;

        let mut out = Vec::with_capacity(SEALED_MAGIC_V1.len() + NONCE_BYTES + ciphertext.len());
        out.extend_from_slice(SEALED_MAGIC_V1);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    async fn open(&self, ciphertext: &[u8], aad: &[u8]) -> Result<SecretBytes, SecretCipherError> {
        if ciphertext.len() < MIN_SEALED_LEN {
            return Err(SecretCipherError::Crypto {
                reason: "sealed envelope is shorter than minimum header and authentication tag"
                    .to_owned(),
            });
        }
        if &ciphertext[..SEALED_MAGIC_V1.len()] != SEALED_MAGIC_V1 {
            return Err(SecretCipherError::Crypto {
                reason: "sealed envelope has unsupported version header".to_owned(),
            });
        }

        let key_bytes = self.load_existing_key().await?;
        let cipher = XChaCha20Poly1305::new(&Key::<XChaCha20Poly1305>::from(*key_bytes));

        let nonce_start = SEALED_MAGIC_V1.len();
        let body_start = nonce_start + NONCE_BYTES;
        let nonce = XNonce::try_from(&ciphertext[nonce_start..body_start]).map_err(|_| {
            SecretCipherError::Crypto {
                reason: "sealed envelope nonce header has invalid length".to_owned(),
            }
        })?;
        let body = &ciphertext[body_start..];

        let plaintext = Zeroizing::new(
            cipher
                .decrypt(&nonce, Payload { msg: body, aad })
                .map_err(|_| SecretCipherError::Crypto {
                    reason: "ciphertext authentication or AAD verification failed".to_owned(),
                })?,
        );
        Ok(SecretBytes::new(plaintext.to_vec()))
    }
}
