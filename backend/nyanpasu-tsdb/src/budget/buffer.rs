//! Capacity-bounded storage that keeps its allocation charge until storage drops or transfers.

use std::ops::{Deref, DerefMut};

use super::{BudgetExceeded, MemoryAccount, MemoryPlan, Retain, Retained, account::Lease};

/// A capacity-bounded vector whose storage is covered by a lease from an operation envelope.
/// Its actual capacity in bytes never exceeds its charge; zero-sized elements need no charge.
#[derive(Debug)]
pub(crate) struct BudgetVec<'a, T> {
    /// Must stay before `lease`: fields drop in declaration order, so storage drops before
    /// the lease returns its reservation credit. Do not reorder these fields.
    storage: Vec<T>,
    /// The charge covering this storage until retirement or transfer.
    lease: Lease<'a>,
}

impl<'a, T> BudgetVec<'a, T> {
    /// Checks the layout and admits its full capacity before allocating storage.
    pub(super) fn new(account: &'a MemoryAccount, capacity: usize) -> Result<Self, BudgetExceeded> {
        let lease = account.lease(MemoryPlan::array::<T>(capacity)?)?;
        let storage = Vec::with_capacity(capacity);
        let mut buffer = Self { storage, lease };
        buffer.check();
        buffer
            .lease
            .allocated((buffer.storage.capacity() * size_of::<T>()) as u64);
        Ok(buffer)
    }

    /// Reserves the whole new capacity beside the old allocation before moving elements.
    /// The old storage drops before its credit returns. Failed admission leaves the buffer,
    /// its contents and its capacity unchanged; a capacity already covered does nothing.
    pub(crate) fn try_grow_to(&mut self, capacity: usize) -> Result<(), BudgetExceeded> {
        if capacity <= self.storage.capacity() {
            return Ok(());
        }
        let mut next = Self::new(self.lease.account, capacity)?;
        next.storage.append(&mut self.storage);
        std::mem::swap(self, &mut next);
        drop(next);
        self.check();
        Ok(())
    }

    /// Appends one element without growing storage.
    ///
    /// # Panics
    ///
    /// Exceeding capacity is a programming error.
    pub(crate) fn push(&mut self, value: T) {
        assert!(self.storage.len() < self.storage.capacity());
        self.storage.push(value);
        self.check();
    }

    /// Appends a batch without growing storage.
    ///
    /// # Panics
    ///
    /// Exceeding the remaining capacity is a programming error.
    pub(crate) fn extend_from_slice(&mut self, values: &[T])
    where
        T: Copy,
    {
        assert!(values.len() <= self.storage.capacity() - self.storage.len());
        self.storage.extend_from_slice(values);
        self.check();
    }

    /// Drops all elements while retaining the storage capacity and its charge.
    pub(crate) fn clear(&mut self) {
        self.storage.clear();
        self.check();
    }

    /// Removes the last element while retaining the storage capacity and its charge.
    pub(crate) fn pop(&mut self) -> Option<T> {
        self.storage.pop()
    }

    /// Returns the actual element capacity, including the sentinel for zero-sized elements.
    pub(crate) fn capacity(&self) -> usize {
        self.storage.capacity()
    }

    /// Checks that actual heap capacity is covered without charging a zero-sized sentinel.
    fn check(&self) {
        debug_assert!(
            size_of::<T>() == 0
                || self.capacity() as u64 * size_of::<T>() as u64 <= self.lease.bytes
        );
    }
}

impl<T> Deref for BudgetVec<'_, T> {
    type Target = [T];

    /// Borrows initialized elements without exposing a growable vector.
    fn deref(&self) -> &[T] {
        &self.storage
    }
}

impl<T> DerefMut for BudgetVec<'_, T> {
    /// Mutably borrows initialized elements without exposing capacity-changing operations.
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.storage
    }
}

impl<T> Retain<T> for BudgetVec<'_, T> {
    /// Freezes storage in place and moves its lease credit into the retained claim.
    fn retain(self, retained: &mut Retained<'_>) -> Result<FrozenVec<T>, BudgetExceeded> {
        self.check();
        let Self { storage, lease } = self;
        lease.retain(retained);
        Ok(FrozenVec { storage })
    }
}

/// Immutable vector storage whose charge belongs to the surrounding retained claim.
/// Construction moves charged storage without reallocating; no growable handle is exposed.
#[derive(Debug)]
pub(crate) struct FrozenVec<T> {
    /// The storage kept alive by the surrounding retained payload.
    storage: Vec<T>,
}

impl<T> FrozenVec<T> {
    /// Returns actual capacity for independent retained-payload accounting in tests.
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.storage.capacity()
    }
}

impl<T> Deref for FrozenVec<T> {
    type Target = [T];

    /// Borrows the frozen elements without exposing mutable storage.
    fn deref(&self) -> &[T] {
        &self.storage
    }
}
