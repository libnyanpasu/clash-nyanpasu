//! Full-value sync status and conflict values shared with the frontend.
//!
//! `SyncError` is a typed enum so the UI localizes by variant instead of
//! parsing text. The status carries a revision, so a delayed read cannot
//! replace a newer value.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Whether this build and this device can sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncAvailability {
    /// The shell supplied no sync adapters.
    Unsupported,
    Disabled,
    Enabled,
}

/// The phase of the current or last cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncPhase {
    Idle,
    Scanning,
    Fetching,
    Planning,
    Downloading,
    Snapshotting,
    Applying,
    Uploading,
    Committing,
    Confirming,
    Finalizing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncProgress {
    pub done: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncOutcome {
    Synced,
    UpToDate,
    Degraded,
    Failed,
}

/// A classified sync failure. The UI localizes by variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyncError {
    Unauthorized,
    Unreachable,
    QuotaExceeded,
    VaultTooNew { found: u32, supported: u32 },
    VaultMismatch,
    ProviderLacksConditionalCreate,
    Storage { reason: String },
}

/// Why an item is held back rather than synced or applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyncBlockedReason {
    SchemaTooNew {
        remote_revision: u32,
        supported: u32,
    },
    NotUtf8,
    UnportableName,
    CaseCollision,
    /// The core refused this exact candidate. It is not retried until either
    /// side's content changes or the user retries explicitly.
    RuntimeRefused {
        reason: String,
    },
    /// The candidate could not be checked; a later cycle retries it.
    RuntimeUnavailable {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct BlockedItem {
    /// The logical key, or the physical path when it has no valid key.
    pub item: String,
    pub reason: SyncBlockedReason,
}

/// The remote head, for the status card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncHead {
    pub generation: u64,
    pub device_name: String,
    pub created_at: String,
}

/// Which side of a conflict a resolution chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ConflictChoice {
    Local,
    Remote,
    KeepBoth,
}

/// One unit both sides changed, with both values so the UI can show them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncConflict {
    /// Stable across cycles, so a resolution addresses the same unit.
    pub id: String,
    /// The logical item, such as `app/application` or `profiles/<file>`.
    pub item: String,
    /// The unit within the item, such as `theme_mode` or `items/<uid>`.
    pub unit: String,
    pub kind: SyncConflictKind,
    /// The local value as JSON text: a document field or a content hash.
    pub local: Option<String>,
    /// The remote value as JSON text, in the same form as `local`.
    pub remote: Option<String>,
    /// The user's recorded choice, consumed by the next cycle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<ConflictChoice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncConflictKind {
    ModifyModify,
    ModifyDelete,
    DeleteActive,
}

/// A recorded decision the next cycle consumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncResolution {
    pub conflict_id: String,
    pub choice: ConflictChoice,
}

/// The full sync status, published as `SyncStatusChanged`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SyncStatus {
    /// Increases with every change, so a delayed read cannot replace a newer value.
    pub revision: u64,
    pub availability: SyncAvailability,
    pub phase: SyncPhase,
    pub progress: Option<SyncProgress>,
    pub last_success_at: Option<i64>,
    pub last_result: Option<SyncOutcome>,
    pub last_error: Option<SyncError>,
    pub open_conflicts: u32,
    pub blocked_items: Vec<BlockedItem>,
    pub head: Option<SyncHead>,
    /// The last cycle merged a diverged remote branch.
    #[serde(default)]
    pub diverged: bool,
}

impl Default for SyncStatus {
    fn default() -> Self {
        Self {
            revision: 0,
            availability: SyncAvailability::Disabled,
            phase: SyncPhase::Idle,
            progress: None,
            last_success_at: None,
            last_result: None,
            last_error: None,
            open_conflicts: 0,
            blocked_items: Vec::new(),
            head: None,
            diverged: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_status_is_disabled_and_idle() {
        let status = SyncStatus::default();
        assert_eq!(status.availability, SyncAvailability::Disabled);
        assert_eq!(status.phase, SyncPhase::Idle);
        assert!(status.blocked_items.is_empty());
    }

    #[test]
    fn status_round_trips_and_localizes_errors_by_variant() {
        let status = SyncStatus {
            revision: 3,
            availability: SyncAvailability::Enabled,
            phase: SyncPhase::Fetching,
            last_error: Some(SyncError::VaultTooNew {
                found: 2,
                supported: 1,
            }),
            ..Default::default()
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["last_error"]["kind"], "vault_too_new");
        assert_eq!(serde_json::from_value::<SyncStatus>(json).unwrap(), status);
    }
}
