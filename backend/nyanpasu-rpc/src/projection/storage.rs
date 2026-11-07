//! Storage snapshots retain their revisions at the transport boundary.

use nyanpasu_core::storage::{
    StorageItem as DomainStorageItem, StorageRevision as DomainStorageRevision,
};
use nyanpasu_rpc_protocol::wire::storage::{StorageItem, StorageRevision};

use super::IntoWire;

impl IntoWire for DomainStorageRevision {
    type Wire = StorageRevision;

    fn into_wire(self) -> Self::Wire {
        StorageRevision(self.get())
    }
}

impl<T> IntoWire for DomainStorageItem<T> {
    type Wire = StorageItem<T>;

    fn into_wire(self) -> Self::Wire {
        StorageItem {
            value: self.value,
            revision: self.revision.into_wire(),
        }
    }
}
