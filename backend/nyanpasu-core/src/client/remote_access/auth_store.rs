//! Cryptographic hashing, constant-time verification, credential minting, and protected
//! durable store serialization/deserialization.

use std::collections::HashSet;

use nyanpasu_contracts::auth::{
    EntropySource, LoadedSecretRecord, ScopeSet, SecretCipher, SecretHash, SecretStore,
    SecretStoreError, SecretString, StorageProtection, TlsMaterialBlob, TokenId, TokenName,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::error::RemoteAuthError;

pub(super) const PERSISTENCE_AAD: &[u8] = b"nyanpasu.remote-access.secrets.v1";
pub(super) const STORE_SCHEMA_VERSION: u32 = 1;
pub(super) const MAX_PERSISTED_RECORD_BYTES: usize = 512 * 1024;
pub(super) const MAX_TLS_CERT_PEM_BYTES: usize = 64 * 1024;
pub(super) const MAX_TLS_KEY_PEM_BYTES: usize = 64 * 1024;
const TLS_FINGERPRINT_HEX_LEN: usize = 64;

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub(super) struct PersistedTokenRecord {
    #[zeroize(skip)]
    pub(super) id: TokenId,
    #[zeroize(skip)]
    pub(super) name: TokenName,
    #[zeroize(skip)]
    pub(super) scopes: ScopeSet,
    pub(super) token_hash: SecretHash,
    pub(super) created_at_ms: i64,
    pub(super) expires_at_ms: Option<i64>,
    pub(super) last_used_at_ms: Option<i64>,
}

#[derive(Clone, Default)]
pub(super) struct PersistedAuthState {
    pub(super) tokens: Vec<PersistedTokenRecord>,
    pub(super) tls_material: Option<TlsMaterialBlob>,
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct StoredTokenRecordDto {
    id: String,
    name: String,
    #[zeroize(skip)]
    scopes: ScopeSet,
    token_hash_hex: String,
    created_at_ms: i64,
    expires_at_ms: Option<i64>,
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct StoredTlsMaterialDto {
    cert_pem: String,
    private_key_pem: String,
    fingerprint_sha256: String,
    expires_at_ms: Option<i64>,
}

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct PersistedAuthPayloadV1 {
    version: u32,
    tokens: Vec<StoredTokenRecordDto>,
    tls: Option<StoredTlsMaterialDto>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
enum StoredEnvelope {
    PlainV1 {
        payload: PersistedAuthPayloadV1,
    },
    EncryptedV1 {
        version: u32,
        ciphertext_hex: String,
    },
}

/// Compute SHA-256 digest of credential bytes cleanly wrapped in redacted `SecretHash`.
pub(super) fn sha256_secret(bytes: &[u8]) -> SecretHash {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest: [u8; 32] = hasher.finalize().into();
    SecretHash::from_bytes(digest)
}

/// Constant-time verification of two SHA-256 secret hashes using `subtle::ConstantTimeEq`.
pub(super) fn constant_time_hash_eq(lhs: &SecretHash, rhs: &SecretHash) -> bool {
    bool::from(lhs.as_bytes().ct_eq(rhs.as_bytes()))
}

fn random_hex<const N: usize>(entropy: &dyn EntropySource) -> Result<String, RemoteAuthError> {
    let mut buf = Zeroizing::new([0u8; N]);
    entropy
        .fill_bytes(&mut *buf)
        .map_err(|err| RemoteAuthError::EntropyFailed { reason: err.reason })?;
    Ok(hex::encode(*buf))
}

pub(super) fn mint_token_credential(
    entropy: &dyn EntropySource,
) -> Result<(TokenId, SecretString, SecretHash), RemoteAuthError> {
    let id_hex = random_hex::<8>(entropy)?;
    let mut secret_bytes = Zeroizing::new([0u8; 32]);
    entropy
        .fill_bytes(&mut *secret_bytes)
        .map_err(|err| RemoteAuthError::EntropyFailed { reason: err.reason })?;
    let secret_hex = Zeroizing::new(hex::encode(*secret_bytes));
    let raw = SecretString::new(format!("npk_{id_hex}_{}", *secret_hex));
    // Design §4.2: persist only SHA-256 of the 256-bit secret portion.
    let hash = sha256_secret(&*secret_bytes);
    let id = TokenId::parse(id_hex).map_err(|err| RemoteAuthError::EntropyFailed {
        reason: err.to_string(),
    })?;
    Ok((id, raw, hash))
}

pub(super) fn mint_launch_code_credential(
    entropy: &dyn EntropySource,
) -> Result<(SecretString, SecretHash), RemoteAuthError> {
    let secret_hex = Zeroizing::new(random_hex::<32>(entropy)?);
    let raw = SecretString::new(format!("npl_{}", *secret_hex));
    let hash = sha256_secret(raw.expose_secret().as_bytes());
    Ok((raw, hash))
}

pub(super) fn mint_prefixed_cookie_credential(
    entropy: &dyn EntropySource,
    prefix: &str,
    id_prefix: &str,
) -> Result<(String, SecretString, SecretHash), RemoteAuthError> {
    let id_suffix = random_hex::<8>(entropy)?;
    let public_id = format!("{id_prefix}{id_suffix}");
    let secret_hex = Zeroizing::new(random_hex::<32>(entropy)?);
    let cookie = SecretString::new(format!("{prefix}_{public_id}_{}", *secret_hex));
    let hash = sha256_secret(cookie.expose_secret().as_bytes());
    Ok((public_id, cookie, hash))
}

pub(super) fn validate_tls_blob(blob: &TlsMaterialBlob) -> Result<(), RemoteAuthError> {
    if blob.cert_pem.trim().is_empty() || blob.cert_pem.len() > MAX_TLS_CERT_PEM_BYTES {
        return Err(RemoteAuthError::InvalidInput {
            reason: "TLS certificate PEM is empty or exceeds maximum length".to_owned(),
        });
    }
    let key = blob.private_key_pem.expose_secret();
    if key.trim().is_empty() || key.len() > MAX_TLS_KEY_PEM_BYTES {
        return Err(RemoteAuthError::InvalidInput {
            reason: "TLS private key PEM is empty or exceeds maximum length".to_owned(),
        });
    }
    if blob.fingerprint_sha256.len() != TLS_FINGERPRINT_HEX_LEN
        || !blob
            .fingerprint_sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(RemoteAuthError::InvalidInput {
            reason: "TLS certificate fingerprint must be 64 lowercase hexadecimal characters"
                .to_owned(),
        });
    }
    Ok(())
}

fn validate_and_decode_payload(
    payload: PersistedAuthPayloadV1,
    max_tokens: usize,
) -> Result<PersistedAuthState, RemoteAuthError> {
    if payload.version != STORE_SCHEMA_VERSION {
        return Err(RemoteAuthError::PersistenceFailed {
            reason: format!(
                "unsupported secret store payload version {}",
                payload.version
            ),
        });
    }
    if payload.tokens.len() > max_tokens {
        return Err(RemoteAuthError::PersistenceFailed {
            reason: format!(
                "persisted token count {} exceeds maximum bound {max_tokens}",
                payload.tokens.len()
            ),
        });
    }

    let mut seen_ids = HashSet::with_capacity(payload.tokens.len());
    let mut tokens = Vec::with_capacity(payload.tokens.len());
    for dto in &payload.tokens {
        let id =
            TokenId::parse(dto.id.as_str()).map_err(|err| RemoteAuthError::PersistenceFailed {
                reason: format!("invalid persisted token id: {err}"),
            })?;
        if !seen_ids.insert(id.clone()) {
            return Err(RemoteAuthError::PersistenceFailed {
                reason: "duplicate persisted token id in secret store".to_owned(),
            });
        }
        let name = TokenName::parse(dto.name.as_str()).map_err(|err| {
            RemoteAuthError::PersistenceFailed {
                reason: format!("invalid persisted token name: {err}"),
            }
        })?;
        let mut decoded = Zeroizing::new([0u8; 32]);
        hex::decode_to_slice(&dto.token_hash_hex, &mut *decoded).map_err(|_| {
            RemoteAuthError::PersistenceFailed {
                reason: "invalid persisted token hash encoding".to_owned(),
            }
        })?;
        tokens.push(PersistedTokenRecord {
            id,
            name,
            scopes: dto.scopes.clone(),
            token_hash: SecretHash::from_bytes(*decoded),
            created_at_ms: dto.created_at_ms,
            expires_at_ms: dto.expires_at_ms,
            last_used_at_ms: None,
        });
    }

    let tls_material = match &payload.tls {
        Some(dto) => {
            let blob = TlsMaterialBlob {
                cert_pem: dto.cert_pem.clone(),
                private_key_pem: SecretString::new(dto.private_key_pem.clone()),
                fingerprint_sha256: dto.fingerprint_sha256.clone(),
                expires_at_ms: dto.expires_at_ms,
            };
            validate_tls_blob(&blob).map_err(|err| RemoteAuthError::PersistenceFailed {
                reason: format!("invalid persisted TLS material: {err}"),
            })?;
            Some(blob)
        }
        None => None,
    };

    Ok(PersistedAuthState {
        tokens,
        tls_material,
    })
}

/// Load persisted secret state from `store`, failing closed on corruption, unprotected
/// plaintext, missing cipher, decryption errors, or malformed/oversized payloads.
pub(super) async fn load_persisted_state(
    store: &dyn SecretStore,
    cipher: Option<&dyn SecretCipher>,
    max_tokens: usize,
    max_record_bytes: usize,
) -> Result<PersistedAuthState, RemoteAuthError> {
    let loaded = store
        .load()
        .await
        .map_err(|err| RemoteAuthError::PersistenceFailed {
            reason: err.to_string(),
        })?;
    let Some(LoadedSecretRecord { bytes, protection }) = loaded else {
        return Ok(PersistedAuthState::default());
    };

    if bytes.expose_bytes().len() > max_record_bytes {
        return Err(RemoteAuthError::PersistenceFailed {
            reason: "persisted secret store record exceeds maximum byte size".to_owned(),
        });
    }

    let envelope: StoredEnvelope = serde_json::from_slice(bytes.expose_bytes()).map_err(|err| {
        RemoteAuthError::PersistenceFailed {
            reason: format!(
                "corrupt secret store envelope ({:?} at line {}, column {})",
                err.classify(),
                err.line(),
                err.column()
            ),
        }
    })?;

    match envelope {
        StoredEnvelope::PlainV1 { payload } => {
            if protection != StorageProtection::Protected {
                return Err(RemoteAuthError::PersistenceFailed {
                    reason: "plaintext secret record found on an unprotected medium".to_owned(),
                });
            }
            validate_and_decode_payload(payload, max_tokens)
        }
        StoredEnvelope::EncryptedV1 {
            version,
            ciphertext_hex,
        } => {
            if version != STORE_SCHEMA_VERSION {
                return Err(RemoteAuthError::PersistenceFailed {
                    reason: format!("unsupported encrypted secret store version {version}"),
                });
            }
            let cipher = cipher.ok_or(RemoteAuthError::UnprotectedStorageWithoutCipher)?;
            let ciphertext = Zeroizing::new(hex::decode(ciphertext_hex).map_err(|_| {
                RemoteAuthError::PersistenceFailed {
                    reason: "corrupt encrypted secret store hex encoding".to_owned(),
                }
            })?);
            let plaintext = cipher
                .open(&ciphertext, PERSISTENCE_AAD)
                .await
                .map_err(|err| RemoteAuthError::PersistenceFailed {
                    reason: err.to_string(),
                })?;
            if plaintext.expose_bytes().len() > max_record_bytes {
                return Err(RemoteAuthError::PersistenceFailed {
                    reason: "decrypted secret store payload exceeds maximum byte size".to_owned(),
                });
            }
            let payload: PersistedAuthPayloadV1 = serde_json::from_slice(plaintext.expose_bytes())
                .map_err(|err| RemoteAuthError::PersistenceFailed {
                    reason: format!(
                        "corrupt decrypted secret payload ({:?} at line {}, column {})",
                        err.classify(),
                        err.line(),
                        err.column()
                    ),
                })?;
            validate_and_decode_payload(payload, max_tokens)
        }
    }
}

fn build_payload_dto(
    tokens: &[PersistedTokenRecord],
    tls_material: Option<&TlsMaterialBlob>,
) -> PersistedAuthPayloadV1 {
    let token_dtos = tokens
        .iter()
        .map(|record| StoredTokenRecordDto {
            id: record.id.as_str().to_owned(),
            name: record.name.as_str().to_owned(),
            scopes: record.scopes.clone(),
            token_hash_hex: hex::encode(record.token_hash.as_bytes()),
            created_at_ms: record.created_at_ms,
            expires_at_ms: record.expires_at_ms,
        })
        .collect();
    let tls = tls_material.map(|blob| StoredTlsMaterialDto {
        cert_pem: blob.cert_pem.clone(),
        private_key_pem: blob.private_key_pem.expose_secret().to_owned(),
        fingerprint_sha256: blob.fingerprint_sha256.clone(),
        expires_at_ms: blob.expires_at_ms,
    });
    PersistedAuthPayloadV1 {
        version: STORE_SCHEMA_VERSION,
        tokens: token_dtos,
        tls,
    }
}

async fn save_sealed_envelope(
    store: &dyn SecretStore,
    cipher: Option<&dyn SecretCipher>,
    payload: &PersistedAuthPayloadV1,
    max_record_bytes: usize,
) -> Result<(), RemoteAuthError> {
    let cipher = cipher.ok_or(RemoteAuthError::UnprotectedStorageWithoutCipher)?;
    let plaintext = Zeroizing::new(serde_json::to_vec(payload).map_err(|err| {
        RemoteAuthError::PersistenceFailed {
            reason: format!("failed to serialize secret payload ({:?})", err.classify()),
        }
    })?);
    if plaintext.len() > max_record_bytes {
        return Err(RemoteAuthError::PersistenceFailed {
            reason: "secret store payload exceeds maximum byte size".to_owned(),
        });
    }
    let ciphertext = Zeroizing::new(cipher.seal(&plaintext, PERSISTENCE_AAD).await.map_err(
        |err| RemoteAuthError::PersistenceFailed {
            reason: err.to_string(),
        },
    )?);
    let envelope = StoredEnvelope::EncryptedV1 {
        version: STORE_SCHEMA_VERSION,
        ciphertext_hex: hex::encode(&*ciphertext),
    };
    let encoded =
        serde_json::to_vec(&envelope).map_err(|err| RemoteAuthError::PersistenceFailed {
            reason: format!(
                "failed to encode encrypted secret envelope ({:?})",
                err.classify()
            ),
        })?;
    if encoded.len() > max_record_bytes {
        return Err(RemoteAuthError::PersistenceFailed {
            reason: "encrypted secret store record exceeds maximum byte size".to_owned(),
        });
    }
    store
        .save_encrypted(&encoded)
        .await
        .map_err(|err| RemoteAuthError::PersistenceFailed {
            reason: err.to_string(),
        })
}

/// Persist secret store snapshot enforcing write-time medium protection and byte limits.
///
/// If an initially protected medium downgrades to `SecretStoreError::UnprotectedMedium` at
/// write time, falls back to sealing with `SecretCipher` (`save_encrypted`) when available
/// or fails closed with `RemoteAuthError::UnprotectedStorageWithoutCipher` and zero
/// plaintext writes.
pub(super) async fn save_persisted_state(
    store: &dyn SecretStore,
    cipher: Option<&dyn SecretCipher>,
    tokens: &[PersistedTokenRecord],
    tls_material: Option<&TlsMaterialBlob>,
    max_record_bytes: usize,
) -> Result<(), RemoteAuthError> {
    let protection =
        store
            .protection()
            .await
            .map_err(|err| RemoteAuthError::PersistenceFailed {
                reason: err.to_string(),
            })?;

    if protection == StorageProtection::Unprotected {
        let payload = build_payload_dto(tokens, tls_material);
        return save_sealed_envelope(store, cipher, &payload, max_record_bytes).await;
    }

    // Build PlainV1 envelope bytes ONLY for protected medium write, zeroizing immediately after.
    let plain_bytes = {
        let plain_envelope = StoredEnvelope::PlainV1 {
            payload: build_payload_dto(tokens, tls_material),
        };
        Zeroizing::new(serde_json::to_vec(&plain_envelope).map_err(|err| {
            RemoteAuthError::PersistenceFailed {
                reason: format!(
                    "failed to serialize protected plaintext envelope ({:?})",
                    err.classify()
                ),
            }
        })?)
    };

    if plain_bytes.len() > max_record_bytes {
        return Err(RemoteAuthError::PersistenceFailed {
            reason: "protected secret store record exceeds maximum byte size".to_owned(),
        });
    }

    match store.save_protected_plaintext(&plain_bytes).await {
        Ok(()) => Ok(()),
        Err(SecretStoreError::UnprotectedMedium) => {
            let payload = build_payload_dto(tokens, tls_material);
            save_sealed_envelope(store, cipher, &payload, max_record_bytes).await
        }
        Err(err) => Err(RemoteAuthError::PersistenceFailed {
            reason: err.to_string(),
        }),
    }
}
