//! The remote commit manifest and vault format.
//!
//! A commit is the complete portable state at one generation, so a device
//! never replays history. Objects are content-addressed and immutable; a
//! commit's id is derived from its own canonical JSON, so a torn or foreign
//! object fails verification instead of being trusted.

use std::collections::{BTreeMap, BTreeSet};

use nyanpasu_config::sync::{ObjectId, canonical_json};
use nyanpasu_paths::{ItemCodec, LogicalPath, SyncDocumentKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use snafu::{ResultExt as _, Snafu, ensure};

/// The remote protocol version this build writes.
pub const COMMIT_FORMAT: u32 = 1;
/// How many preceding commit ids a manifest records.
pub const MAX_ANCESTORS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommitId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VaultId(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(String);

#[derive(Debug, Snafu)]
pub enum CommitError {
    #[snafu(display("{value:?} is not a commit id"))]
    MalformedCommitId { value: String },
    #[snafu(display("{value:?} is not a vault id"))]
    MalformedVaultId { value: String },
    #[snafu(display("{value:?} is not a device id"))]
    MalformedDeviceId { value: String },
    #[snafu(display("commit id {found} does not match its content ({expected})"))]
    IdMismatch { found: String, expected: String },
    #[snafu(display("commit format {found} is newer than this build supports ({supported})"))]
    FormatTooNew { found: u32, supported: u32 },
    #[snafu(display("the commit is not valid JSON"))]
    Decode { source: serde_json::Error },
    #[snafu(display("the commit cannot be encoded"))]
    Encode { source: serde_json::Error },
    #[snafu(display("{logical} cannot be decoded with the document codec"))]
    UnknownDocument { logical: LogicalPath },
}

impl VaultId {
    /// A `v_`-prefixed random vault id.
    #[must_use]
    pub fn new(random: &str) -> Self {
        Self(format!("v_{random}"))
    }

    pub fn parse(value: &str) -> Result<Self, CommitError> {
        ensure!(
            value
                .strip_prefix("v_")
                .is_some_and(|rest| !rest.is_empty()),
            MalformedVaultIdSnafu { value }
        );
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl DeviceId {
    /// A `d_`-prefixed random device id.
    #[must_use]
    pub fn new(random: &str) -> Self {
        Self(format!("d_{random}"))
    }

    pub fn parse(value: &str) -> Result<Self, CommitError> {
        ensure!(
            value
                .strip_prefix("d_")
                .is_some_and(|rest| !rest.is_empty()),
            MalformedDeviceIdSnafu { value }
        );
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl CommitId {
    /// The first 32 lowercase hex characters of a SHA-256 digest, the form a
    /// commit name carries.
    pub fn parse(value: &str) -> Result<Self, CommitError> {
        ensure!(
            value.len() == 32
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            MalformedCommitIdSnafu { value }
        );
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CommitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// How a commit's name is chosen on the remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitNaming {
    /// `commits/<generation>-<id>.json`: conditional create is unavailable, so
    /// two devices may write the same generation and a fork is merged.
    Content,
    /// `commits/<generation>.json` created with `IfAbsent`: linear history.
    Exclusive,
}

/// The reserved encryption selection; only `none` exists in this delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VaultEncryption {
    None,
}

/// `format.json`, written once when the vault is created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultFormat {
    pub format: u32,
    pub vault_id: VaultId,
    pub created_at: String,
    pub commit_naming: CommitNaming,
    pub encryption: VaultEncryption,
}

impl VaultFormat {
    pub fn parse(bytes: &[u8]) -> Result<Self, CommitError> {
        let format: Self = serde_json::from_slice(bytes).context(DecodeSnafu)?;
        ensure!(
            format.format <= COMMIT_FORMAT,
            FormatTooNewSnafu {
                found: format.format,
                supported: COMMIT_FORMAT,
            }
        );
        Ok(format)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, CommitError> {
        let value = serde_json::to_value(self).context(EncodeSnafu)?;
        Ok(canonical_json(&value))
    }
}

/// The codec a manifest records. The document kind is implied by the logical
/// path, so the wire form stays small.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitCodec {
    Document,
    Text,
    Binary,
}

impl CommitCodec {
    /// The item codec this manifest row describes.
    ///
    /// # Errors
    ///
    /// [`CommitError::UnknownDocument`] for a `document` row whose key this
    /// build does not know.
    pub fn into_item_codec(self, logical: &LogicalPath) -> Result<ItemCodec, CommitError> {
        Ok(match self {
            Self::Text => ItemCodec::Text,
            Self::Binary => ItemCodec::Binary,
            Self::Document => ItemCodec::Document(document_kind(logical).ok_or_else(|| {
                CommitError::UnknownDocument {
                    logical: logical.clone(),
                }
            })?),
        })
    }
}

impl From<ItemCodec> for CommitCodec {
    fn from(codec: ItemCodec) -> Self {
        match codec {
            ItemCodec::Document(_) => Self::Document,
            ItemCodec::Text => Self::Text,
            ItemCodec::Binary => Self::Binary,
        }
    }
}

fn document_kind(logical: &LogicalPath) -> Option<SyncDocumentKind> {
    match logical.as_str() {
        "app/application" => Some(SyncDocumentKind::Application),
        "app/clash" => Some(SyncDocumentKind::Clash),
        "app/profiles" => Some(SyncDocumentKind::Profiles),
        _ => None,
    }
}

/// Where an item was last changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangedAt {
    pub generation: u64,
    pub device: DeviceId,
}

/// A live item in a commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveItem {
    pub codec: CommitCodec,
    pub sha256: ObjectId,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portable_revision: Option<u32>,
    pub changed: ChangedAt,
}

/// A deletion, kept so a device without a baseline can tell it from "new here".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    pub deleted: TombstoneBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TombstoneBody {
    pub generation: u64,
    pub device: DeviceId,
    pub at: String,
    pub last_sha256: ObjectId,
}

/// One row of a commit's item table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CommitItem {
    Live(LiveItem),
    Deleted(Tombstone),
}

impl CommitItem {
    #[must_use]
    pub fn sha256(&self) -> Option<&ObjectId> {
        match self {
            Self::Live(item) => Some(&item.sha256),
            Self::Deleted(_) => None,
        }
    }

    #[must_use]
    pub fn is_deleted(&self) -> bool {
        matches!(self, Self::Deleted(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitDevice {
    pub id: DeviceId,
    pub name: String,
}

/// The complete portable state at one generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitManifest {
    pub format: u32,
    pub generation: u64,
    pub id: CommitId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<CommitId>,
    #[serde(default)]
    pub ancestors: Vec<CommitId>,
    pub device: CommitDevice,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    pub items: BTreeMap<LogicalPath, CommitItem>,
}

impl CommitManifest {
    /// Builds a manifest and derives its id from its content.
    ///
    /// # Errors
    ///
    /// [`CommitError::Encode`] when the manifest cannot be serialized.
    pub fn new(
        generation: u64,
        parent: Option<CommitId>,
        mut ancestors: Vec<CommitId>,
        device: CommitDevice,
        created_at: String,
        app_version: Option<String>,
        items: BTreeMap<LogicalPath, CommitItem>,
    ) -> Result<Self, CommitError> {
        ancestors.truncate(MAX_ANCESTORS);
        let mut manifest = Self {
            format: COMMIT_FORMAT,
            generation,
            // Placeholder; replaced below once the body is known.
            id: CommitId(String::new()),
            parent,
            ancestors,
            device,
            created_at,
            app_version,
            items,
        };
        let id = manifest.compute_id()?;
        manifest.id = id;
        Ok(manifest)
    }

    /// The id this manifest's body hashes to.
    ///
    /// # Errors
    ///
    /// [`CommitError::Encode`] when the manifest cannot be serialized.
    pub fn compute_id(&self) -> Result<CommitId, CommitError> {
        let mut value = serde_json::to_value(self).context(EncodeSnafu)?;
        if let Value::Object(map) = &mut value {
            map.remove("id");
        }
        let digest = Sha256::digest(canonical_json(&value));
        let hex = hex::encode(digest);
        Ok(CommitId(hex[..32].to_owned()))
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, CommitError> {
        let value = serde_json::to_value(self).context(EncodeSnafu)?;
        Ok(canonical_json(&value))
    }

    /// Decodes and verifies a manifest read from the remote.
    ///
    /// # Errors
    ///
    /// [`CommitError::Decode`] for malformed JSON, [`CommitError::FormatTooNew`]
    /// for a newer protocol, and [`CommitError::IdMismatch`] when the body does
    /// not hash to the id it claims.
    pub fn parse(bytes: &[u8]) -> Result<Self, CommitError> {
        let manifest: Self = serde_json::from_slice(bytes).context(DecodeSnafu)?;
        ensure!(
            manifest.format <= COMMIT_FORMAT,
            FormatTooNewSnafu {
                found: manifest.format,
                supported: COMMIT_FORMAT,
            }
        );
        let expected = manifest.compute_id()?;
        ensure!(
            expected == manifest.id,
            IdMismatchSnafu {
                found: manifest.id.as_str().to_owned(),
                expected: expected.as_str().to_owned(),
            }
        );
        Ok(manifest)
    }

    /// The object ids this commit references, for upload avoidance and GC.
    #[must_use]
    pub fn object_ids(&self) -> BTreeSet<ObjectId> {
        self.items
            .values()
            .filter_map(|item| item.sha256().cloned())
            .collect()
    }

    /// The next generation after this commit.
    #[must_use]
    pub fn next_generation(&self) -> u64 {
        self.generation.saturating_add(1)
    }

    /// The ancestors a child of this commit records: this id, then the newest
    /// of its own ancestors.
    #[must_use]
    pub fn child_ancestors(&self) -> Vec<CommitId> {
        let mut ancestors = Vec::with_capacity(MAX_ANCESTORS);
        ancestors.push(self.id.clone());
        ancestors.extend(self.ancestors.iter().take(MAX_ANCESTORS - 1).cloned());
        ancestors
    }

    /// Whether `id` is this commit or one of its recorded ancestors.
    #[must_use]
    pub fn is_ancestor_of(&self, id: &CommitId) -> bool {
        &self.id == id || self.ancestors.contains(id)
    }
}

/// The newest commit in a listing: highest generation, then lowest id.
#[must_use]
pub fn head_of(manifests: impl IntoIterator<Item = CommitManifest>) -> Option<CommitManifest> {
    manifests.into_iter().max_by(|left, right| {
        left.generation
            .cmp(&right.generation)
            .then_with(|| right.id.cmp(&left.id))
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn device() -> CommitDevice {
        CommitDevice {
            id: DeviceId::new("Qm3x"),
            name: "keiko-laptop".to_owned(),
        }
    }

    fn manifest(generation: u64, parent: Option<CommitId>) -> CommitManifest {
        let logical = LogicalPath::parse("app/application").unwrap();
        let item = CommitItem::Live(LiveItem {
            codec: CommitCodec::Document,
            sha256: ObjectId::of(b"application"),
            size: 11,
            portable_revision: Some(1),
            changed: ChangedAt {
                generation,
                device: DeviceId::new("Qm3x"),
            },
        });
        CommitManifest::new(
            generation,
            parent,
            Vec::new(),
            device(),
            "2026-11-14T12:03:11Z".to_owned(),
            None,
            BTreeMap::from([(logical, item)]),
        )
        .unwrap()
    }

    #[test]
    fn a_commit_round_trips_and_verifies_its_id() {
        let commit = manifest(1, None);
        let bytes = commit.to_bytes().unwrap();
        assert_eq!(CommitManifest::parse(&bytes).unwrap(), commit);
    }

    #[test]
    fn a_tampered_commit_is_rejected() {
        let commit = manifest(1, None);
        let mut value: Value = serde_json::from_slice(&commit.to_bytes().unwrap()).unwrap();
        value["generation"] = json!(99);
        let bytes = canonical_json(&value);
        assert!(matches!(
            CommitManifest::parse(&bytes),
            Err(CommitError::IdMismatch { .. })
        ));
    }

    #[test]
    fn a_newer_commit_format_is_reported() {
        let mut value: Value =
            serde_json::from_slice(&manifest(1, None).to_bytes().unwrap()).unwrap();
        value["format"] = json!(2);
        assert!(matches!(
            CommitManifest::parse(&canonical_json(&value)),
            Err(CommitError::FormatTooNew {
                found: 2,
                supported: 1
            })
        ));
    }

    #[test]
    fn tombstones_serialize_as_a_deleted_row() {
        let logical = LogicalPath::parse("profiles/a.yaml").unwrap();
        let item = CommitItem::Deleted(Tombstone {
            deleted: TombstoneBody {
                generation: 41,
                device: DeviceId::new("8hTz"),
                at: "2026-11-10T08:00:00Z".to_owned(),
                last_sha256: ObjectId::of(b"old"),
            },
        });
        let value = serde_json::to_value(BTreeMap::from([(logical, item)])).unwrap();
        assert_eq!(
            value["profiles/a.yaml"]["deleted"]["last_sha256"],
            json!(ObjectId::of(b"old").as_str())
        );
    }

    #[test]
    fn a_child_records_its_parent_and_ancestors() {
        let first = manifest(1, None);
        let second = CommitManifest::new(
            2,
            Some(first.id.clone()),
            first.child_ancestors(),
            device(),
            "2026-11-14T12:03:12Z".to_owned(),
            None,
            first.items.clone(),
        )
        .unwrap();
        assert!(second.is_ancestor_of(&first.id));
        assert!(!first.is_ancestor_of(&second.id));
        let third = CommitManifest::new(
            3,
            Some(second.id.clone()),
            second.child_ancestors(),
            device(),
            "2026-11-14T12:03:13Z".to_owned(),
            None,
            first.items.clone(),
        )
        .unwrap();
        assert!(third.is_ancestor_of(&first.id));
    }

    #[test]
    fn the_head_is_the_highest_generation_then_lowest_id() {
        let mut older = manifest(3, None);
        let mut newer = manifest(4, None);
        older.id = CommitId::parse(&"f".repeat(32)).unwrap();
        newer.id = CommitId::parse(&"a".repeat(32)).unwrap();
        assert_eq!(head_of([older, newer.clone()]).unwrap(), newer);
        let mut tie = manifest(4, None);
        tie.id = CommitId::parse(&"b".repeat(32)).unwrap();
        assert_eq!(
            head_of([tie, newer.clone()]).unwrap(),
            newer,
            "the lowest id wins a generation tie"
        );
    }

    #[test]
    fn document_rows_resolve_their_kind_from_the_key() {
        assert_eq!(
            CommitCodec::Document
                .into_item_codec(&LogicalPath::parse("app/clash").unwrap())
                .unwrap(),
            ItemCodec::Document(SyncDocumentKind::Clash)
        );
        assert!(matches!(
            CommitCodec::Document.into_item_codec(&LogicalPath::parse("icons/a.png").unwrap()),
            Err(CommitError::UnknownDocument { .. })
        ));
    }

    #[test]
    fn a_vault_format_round_trips_and_rejects_a_newer_version() {
        let format = VaultFormat {
            format: 1,
            vault_id: VaultId::new("4f0c"),
            created_at: "2026-11-14T12:00:00Z".to_owned(),
            commit_naming: CommitNaming::Content,
            encryption: VaultEncryption::None,
        };
        let bytes = format.to_bytes().unwrap();
        assert_eq!(VaultFormat::parse(&bytes).unwrap(), format);
        let mut value: Value = serde_json::from_slice(&bytes).unwrap();
        value["format"] = json!(2);
        assert!(matches!(
            VaultFormat::parse(&canonical_json(&value)),
            Err(CommitError::FormatTooNew { .. })
        ));
    }
}
