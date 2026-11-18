//! Status values derived from a commit, kept out of the engine's phases.

pub use nyanpasu_runtime_values::sync::SyncHead;

use super::commit::CommitManifest;

/// The head summary the status card shows.
#[must_use]
pub fn head_of(commit: &CommitManifest) -> SyncHead {
    SyncHead {
        generation: commit.generation,
        device_name: commit.device.name.clone(),
        created_at: commit.created_at.clone(),
    }
}
