//! Committing and publishing (§7.1):
//!
//! 1. under the registry's write lock, register the staged version as pending P+1;
//! 2. commit to storage, with no lock held, so an fsync never blocks a query;
//! 3. under the write lock, publish P+1, or withdraw it.
//!
//! Publishing cannot fail: staging allocated and reserved everything the version needs, so
//! publishing is an exchange of ownership (§4.4). A failed commit withdraws the version, and
//! flags the store for recovery when the batch may have applied (`Pending::fail`).

use super::{Written, stage::Staged};
use crate::{
    budget::Reservation,
    storage::{CommitFailure, Durability, StorageWrite},
    version::Registry,
};

/// Commits `batch`, in which `staged` was staged, and publishes its version. `head` is the
/// owner's reservation of the current head's bytes; it follows the published head.
///
/// On a failure the pending version is withdrawn and its reservation returned, whether or
/// not the batch applied: after an unknown outcome the owner recovers from storage (§7.5).
pub(crate) fn commit(
    batch: impl StorageWrite,
    staged: Staged,
    registry: &Registry,
    head: &mut Reservation,
    durability: Durability,
) -> Result<Written, CommitFailure> {
    let Staged {
        version,
        growth,
        written,
    } = staged;
    let pending = registry.register(version, growth);
    if let Err(failure) = batch.commit(durability) {
        pending.fail(&failure);
        return Err(failure);
    }
    pending.publish(head);
    Ok(written)
}
