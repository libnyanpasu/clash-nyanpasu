//! Committing and publishing (§7.1):
//!
//! 1. under the registry's write lock, register the staged version as pending P+1;
//! 2. commit with only the owner's G-read guard, so no R guard spans an fsync;
//! 3. under the write lock, publish P+1, or withdraw it.
//!
//! Publishing cannot fail: staging allocated and reserved everything the version needs, so
//! publishing is an exchange of ownership (§4.4). A failed commit withdraws the version, and
//! flags the store for recovery when the batch may have applied (`Pending::fail`).
//!
//! A batch and a maintenance round (`maintain`) publish their versions the same way
//! ([`publish`]).

use super::{Written, stage::Staged};
use crate::{
    storage::{CommitFailure, Durability, StorageWrite},
    version::{Registry, StagedVersion},
};

/// Commits `batch`, in which `staged` was staged, and publishes its version and resident claim.
pub(crate) fn commit(
    batch: impl StorageWrite,
    staged: Staged,
    registry: &Registry,
    durability: Durability,
) -> Result<Written, CommitFailure> {
    let Staged {
        version,
        growth,
        staging,
        written,
    } = staged;
    let next = StagedVersion {
        version,
        growth,
        staging,
    };
    publish(batch, next, registry, durability)?;
    Ok(written)
}

/// Commits `batch`, in which `next` was staged, and transfers its claims to the registry owner.
///
/// On a failure the pending version is withdrawn and its reservation returned, whether or
/// not the batch applied: after an unknown outcome the owner recovers from storage (§7.5).
/// The staging memory is returned once the version is published or withdrawn: the copies it
/// covered are then held once, by the current version or by the snapshots that reserved them.
pub(crate) fn publish(
    batch: impl StorageWrite,
    next: StagedVersion,
    registry: &Registry,
    durability: Durability,
) -> Result<(), CommitFailure> {
    let pending = registry.register(next);
    if let Err(failure) = batch.commit(durability) {
        pending.fail(&failure);
        return Err(failure);
    }
    pending.publish();
    Ok(())
}
