//! Committing and publishing (§7.1):
//!
//! 1. under the registry's write lock, register the staged version as pending P+1;
//! 2. commit to storage, with no lock held, so an fsync never blocks a query;
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
    budget::Reservation,
    storage::{CommitFailure, Durability, StorageWrite},
    version::{Registry, Version},
};

/// A version staged in a transaction, with the memory it holds until it is published or
/// withdrawn.
pub(crate) struct Next {
    pub(crate) version: Version,
    /// The bytes by which `version`'s head outgrows the current one's, reserved from the
    /// head budget. Publishing hands them to the owner's head reservation; withdrawing the
    /// version returns them.
    pub(crate) growth: Reservation,
    /// The staging memory still allocated (§7.1, §7.4): what `version` copies from the
    /// current version, which stays allocated twice until the old version goes.
    pub(crate) staging: Reservation,
}

/// Commits `batch`, in which `staged` was staged, and publishes its version. `head` is the
/// owner's reservation of the current head's bytes; it follows the published head.
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
        staging,
        written,
    } = staged;
    let next = Next {
        version,
        growth,
        staging,
    };
    publish(batch, next, registry, head, durability)?;
    Ok(written)
}

/// Commits `batch`, in which `next` was staged, and publishes its version. `head` is the
/// owner's reservation of the current head's bytes; it follows the published head.
///
/// On a failure the pending version is withdrawn and its reservation returned, whether or
/// not the batch applied: after an unknown outcome the owner recovers from storage (§7.5).
/// The staging memory is returned once the version is published or withdrawn: the copies it
/// covered are then held once, by the current version or by the snapshots that reserved them.
pub(crate) fn publish(
    batch: impl StorageWrite,
    next: Next,
    registry: &Registry,
    head: &mut Reservation,
    durability: Durability,
) -> Result<(), CommitFailure> {
    let Next {
        version,
        growth,
        staging,
    } = next;
    let pending = registry.register(version, growth);
    if let Err(failure) = batch.commit(durability) {
        pending.fail(&failure);
        drop(staging);
        return Err(failure);
    }
    pending.publish(head);
    drop(staging);
    Ok(())
}
