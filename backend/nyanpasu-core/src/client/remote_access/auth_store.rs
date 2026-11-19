//! Credential hashing and minting, and the plain JSON secret record.

use std::collections::HashSet;

use nyanpasu_contracts::auth::{
    EntropySource, Grant, SecretHash, SecretStore, SecretString, TlsAuthority, TokenId, TokenName,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;
use zeroize::{Zeroize as _, Zeroizing};

use super::auth_error::RemoteAuthError;

const STORE_VERSION: u32 = 1;
/// Upper bound on the persisted record; 64 tokens take well under 32 KiB.
pub const MAX_SECRET_RECORD_BYTES: usize = 256 * 1024;

/// A persisted access token. Only the hash of its secret is kept.
pub(super) struct TokenRecord {
    pub(super) id: TokenId,
    pub(super) name: TokenName,
    pub(super) grant: Grant,
    pub(super) hash: SecretHash,
    pub(super) created_at_ms: i64,
    pub(super) expires_at_ms: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct StoredToken {
    id: TokenId,
    name: TokenName,
    grant: Grant,
    hash_hex: String,
    created_at_ms: i64,
    expires_at_ms: Option<i64>,
}

/// The local root and its private key. Absent until HTTPS is first turned on, so records written
/// before it existed still decode. The listener's short-lived certificates are never stored: the
/// root issues a new one after a restart.
#[derive(Serialize, Deserialize)]
struct StoredAuthority {
    certificate_hex: String,
    private_key_hex: String,
    not_after_ms: i64,
}

impl Drop for StoredAuthority {
    fn drop(&mut self) {
        self.private_key_hex.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
struct StoredRecord {
    version: u32,
    tokens: Vec<StoredToken>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    authority: Option<StoredAuthority>,
}

/// What the secret record holds.
pub(super) struct LoadedRecord {
    pub(super) tokens: Vec<TokenRecord>,
    pub(super) authority: Option<TlsAuthority>,
}

/// SHA-256 of a credential. Credentials carry 256 bits of entropy, so a fast hash is enough.
pub(super) fn sha256_secret(bytes: &[u8]) -> SecretHash {
    SecretHash::from_bytes(Sha256::digest(bytes).into())
}

/// Whether two hashes are equal, in constant time.
pub(super) fn constant_time_hash_eq(lhs: &SecretHash, rhs: &SecretHash) -> bool {
    bool::from(lhs.as_bytes().ct_eq(rhs.as_bytes()))
}

fn random_hex<const N: usize>(entropy: &dyn EntropySource) -> Result<String, RemoteAuthError> {
    let mut buf = Zeroizing::new([0u8; N]);
    entropy
        .fill_bytes(&mut *buf)
        .map_err(|error| RemoteAuthError::EntropyFailed {
            reason: error.reason,
        })?;
    Ok(hex::encode(*buf))
}

/// A fresh `npk_<id>_<secret>` token, with the id parsed and the hash to persist.
pub(super) fn mint_token(
    entropy: &dyn EntropySource,
) -> Result<(TokenId, SecretString, SecretHash), RemoteAuthError> {
    let id_hex = random_hex::<8>(entropy)?;
    let secret_hex = Zeroizing::new(random_hex::<32>(entropy)?);
    let raw = SecretString::new(format!("npk_{id_hex}_{}", *secret_hex));
    let hash = sha256_secret(raw.expose_secret().as_bytes());
    let id = TokenId::parse(id_hex).map_err(|error| RemoteAuthError::EntropyFailed {
        reason: error.to_string(),
    })?;
    Ok((id, raw, hash))
}

/// A fresh `npl_<secret>` launch code and its hash.
pub(super) fn mint_launch_code(
    entropy: &dyn EntropySource,
) -> Result<(SecretString, SecretHash), RemoteAuthError> {
    let secret_hex = Zeroizing::new(random_hex::<32>(entropy)?);
    let raw = SecretString::new(format!("npl_{}", *secret_hex));
    let hash = sha256_secret(raw.expose_secret().as_bytes());
    Ok((raw, hash))
}

/// A fresh `nps_<session id>_<secret>` cookie, with its public id and hash.
pub(super) fn mint_session_cookie(
    entropy: &dyn EntropySource,
) -> Result<(String, SecretString, SecretHash), RemoteAuthError> {
    let public_id = format!("sess_{}", random_hex::<8>(entropy)?);
    let secret_hex = Zeroizing::new(random_hex::<32>(entropy)?);
    let cookie = SecretString::new(format!("nps_{public_id}_{}", *secret_hex));
    let hash = sha256_secret(cookie.expose_secret().as_bytes());
    Ok((public_id, cookie, hash))
}

fn persistence_failed(reason: impl Into<String>) -> RemoteAuthError {
    RemoteAuthError::PersistenceFailed {
        reason: reason.into(),
    }
}

/// Loads the persisted tokens and certificate, failing closed on a corrupt, oversized or
/// inconsistent record.
pub(super) async fn load_record(
    store: &dyn SecretStore,
    max_tokens: usize,
) -> Result<LoadedRecord, RemoteAuthError> {
    let Some(bytes) = store
        .load()
        .await
        .map_err(|error| persistence_failed(error.to_string()))?
    else {
        return Ok(LoadedRecord {
            tokens: Vec::new(),
            authority: None,
        });
    };
    let record: StoredRecord = serde_json::from_slice(&bytes).map_err(|error| {
        persistence_failed(format!("corrupt secret record ({:?})", error.classify()))
    })?;
    if record.version != STORE_VERSION {
        return Err(persistence_failed(format!(
            "unsupported secret record version {}",
            record.version
        )));
    }
    if record.tokens.len() > max_tokens {
        return Err(persistence_failed(
            "secret record holds more tokens than allowed",
        ));
    }

    let mut seen = HashSet::with_capacity(record.tokens.len());
    let mut tokens = Vec::with_capacity(record.tokens.len());
    for stored in record.tokens {
        if !seen.insert(stored.id.clone()) {
            return Err(persistence_failed("secret record repeats a token id"));
        }
        // A grant is validated when it is issued; a record edited since must not widen a token
        // past what issuance allows, such as the owner role or `remote`.
        stored.grant.validate().map_err(|_| {
            persistence_failed("secret record holds a token grant that cannot be issued")
        })?;
        let mut hash = [0u8; 32];
        hex::decode_to_slice(&stored.hash_hex, &mut hash)
            .map_err(|_| persistence_failed("secret record holds a malformed token hash"))?;
        tokens.push(TokenRecord {
            id: stored.id,
            name: stored.name,
            grant: stored.grant,
            hash: SecretHash::from_bytes(hash),
            created_at_ms: stored.created_at_ms,
            expires_at_ms: stored.expires_at_ms,
        });
    }
    let authority = record
        .authority
        .as_ref()
        .map(decode_authority)
        .transpose()?;
    Ok(LoadedRecord { tokens, authority })
}

fn decode_authority(stored: &StoredAuthority) -> Result<TlsAuthority, RemoteAuthError> {
    let malformed = || persistence_failed("secret record holds a malformed TLS authority");
    let certificate = hex::decode(&stored.certificate_hex).map_err(|_| malformed())?;
    let private_key = hex::decode(&stored.private_key_hex).map_err(|_| malformed())?;
    if certificate.is_empty() || private_key.is_empty() {
        return Err(malformed());
    }
    Ok(TlsAuthority::new(
        certificate,
        private_key,
        stored.not_after_ms,
    ))
}

fn encode_authority(authority: &TlsAuthority) -> StoredAuthority {
    StoredAuthority {
        certificate_hex: hex::encode(authority.certificate_der()),
        private_key_hex: hex::encode(authority.private_key_der()),
        not_after_ms: authority.not_after_ms(),
    }
}

/// Persists `tokens` and the root as the whole record.
pub(super) async fn save_record<'a>(
    store: &dyn SecretStore,
    tokens: impl Iterator<Item = &'a TokenRecord>,
    authority: Option<&TlsAuthority>,
) -> Result<(), RemoteAuthError> {
    let record = StoredRecord {
        version: STORE_VERSION,
        tokens: tokens
            .map(|token| StoredToken {
                id: token.id.clone(),
                name: token.name.clone(),
                grant: token.grant,
                hash_hex: hex::encode(token.hash.as_bytes()),
                created_at_ms: token.created_at_ms,
                expires_at_ms: token.expires_at_ms,
            })
            .collect(),
        authority: authority.map(encode_authority),
    };
    let bytes = Zeroizing::new(serde_json::to_vec(&record).map_err(|error| {
        persistence_failed(format!(
            "cannot encode secret record ({:?})",
            error.classify()
        ))
    })?);
    store
        .save(&bytes)
        .await
        .map_err(|error| persistence_failed(error.to_string()))
}
