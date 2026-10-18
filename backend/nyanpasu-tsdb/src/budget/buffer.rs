//! Capacity-bounded storage that keeps its allocation charge until storage drops or transfers.

use std::ops::{Deref, DerefMut};

use super::{BudgetExceeded, MemoryAccount, MemoryPlan, Retain, Retained, account::Lease};

/// A capacity-bounded vector whose storage is covered by a lease from an operation envelope.
/// Its actual capacity in bytes never exceeds its charge; zero-sized elements need no charge.
#[derive(Debug)]
pub(crate) struct BudgetVec<T> {
    /// Must stay before `lease`: fields drop in declaration order, so storage drops before
    /// the lease returns its reservation credit. Do not reorder these fields.
    storage: Vec<T>,
    /// The charge covering this storage until retirement or transfer.
    lease: Option<Lease>,
}

impl<T> BudgetVec<T> {
    /// Allocates storage under an owned handle to the same envelope.
    pub(super) fn allocate(
        account: &MemoryAccount,
        capacity: usize,
    ) -> Result<Self, BudgetExceeded> {
        let lease = account.lease(MemoryPlan::array::<T>(capacity)?)?;
        let storage = Vec::with_capacity(capacity);
        let mut buffer = Self {
            storage,
            lease: Some(lease),
        };
        buffer.check();
        buffer
            .lease
            .as_mut()
            .expect("constructed with a lease")
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
        let mut next = Self::allocate(
            &self.lease.as_ref().expect("attached buffer").account,
            capacity,
        )?;
        next.storage.append(&mut self.storage);
        std::mem::swap(self, &mut next);
        drop(next);
        self.check();
        Ok(())
    }

    /// Allocates a fixed buffer's full planned capacity on its first use.
    /// Exceeding the cap, or failing an already-admitted enclosing plan, is a programming error.
    pub(crate) fn reserve(&mut self, additional: usize, cap: usize) {
        assert!(additional <= cap.saturating_sub(self.len()));
        if self.len() + additional > self.capacity() {
            assert_eq!(self.capacity(), 0, "a fixed buffer allocates once");
            self.try_grow_to(cap)
                .expect("the enclosing allocation plan covers this buffer");
        }
    }

    /// Drops obsolete storage before any replacement allocation, preserving equal capacities.
    pub(crate) fn discard_unless(&mut self, capacity: usize) {
        if self.capacity() == capacity {
            self.clear();
        } else {
            let empty = Self::allocate(&self.lease.as_ref().expect("attached buffer").account, 0)
                .expect("empty storage is free");
            *self = empty;
        }
    }

    /// Gives a discarded buffer exactly its new capacity, without preserving obsolete contents.
    pub(crate) fn reset_to(&mut self, capacity: usize) -> Result<(), BudgetExceeded> {
        self.discard_unless(capacity);
        self.try_grow_to(capacity)
    }

    /// Shrinks initialized length without releasing storage or credit.
    pub(crate) fn truncate(&mut self, len: usize) {
        self.storage.truncate(len);
    }

    /// Resizes initialized length within admitted capacity; exceeding it is a programming error.
    pub(crate) fn resize(&mut self, len: usize, value: T)
    where
        T: Copy,
    {
        assert!(len <= self.capacity());
        self.storage.resize(len, value);
        self.check();
    }

    /// Appends an iterator within the admitted capacity; it cannot trigger allocation.
    pub(crate) fn extend(&mut self, values: impl IntoIterator<Item = T>) {
        for value in values {
            self.push(value);
        }
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
                || self.capacity() as u64 * size_of::<T>() as u64
                    <= self.lease.as_ref().map_or(0, |lease| lease.bytes)
        );
    }
}

impl<T> Deref for BudgetVec<T> {
    type Target = [T];

    /// Borrows initialized elements without exposing a growable vector.
    fn deref(&self) -> &[T] {
        &self.storage
    }
}

impl<T> DerefMut for BudgetVec<T> {
    /// Mutably borrows initialized elements without exposing capacity-changing operations.
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.storage
    }
}

impl<T> Retain<T> for BudgetVec<T> {
    /// Freezes storage in place and moves its lease credit into the retained claim.
    fn retain(self, retained: &mut Retained<'_>) -> Result<FrozenVec<T>, BudgetExceeded> {
        self.check();
        let Self { storage, lease } = self;
        if let Some(lease) = lease {
            lease.retain(retained);
        }
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

impl std::io::Write for BudgetVec<u8> {
    /// Writes only within admitted capacity; an underestimated output plan is a programming error.
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    /// No output remains buffered outside this charged vector.
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<T> AsRef<[T]> for BudgetVec<T> {
    /// Borrows initialized elements without exposing the allocation handle.
    fn as_ref(&self) -> &[T] {
        &self.storage
    }
}

impl<T: serde::Serialize> serde::Serialize for BudgetVec<T> {
    /// Serializes the bounded sequence without allocating an intermediate collection.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.storage.serialize(serializer)
    }
}

impl<T> Default for BudgetVec<T> {
    /// Empty, allocation-free storage; decoding attaches an envelope before allocating.
    fn default() -> Self {
        Self {
            storage: Vec::new(),
            lease: None,
        }
    }
}

impl<T> BudgetVec<T> {
    /// Attaches decoded storage to its decoder's envelope before admission.
    /// Storage from a different operation drops before any new output allocation.
    pub(crate) fn attach(&mut self, account: &MemoryAccount) {
        if self
            .lease
            .as_ref()
            .is_some_and(|lease| !lease.account.same_envelope(account))
        {
            *self = Self::allocate(account, 0).expect("empty storage is free");
        }
        if self.lease.is_none() {
            self.lease = Some(
                account
                    .lease(MemoryPlan::default())
                    .expect("empty storage is free"),
            );
        }
    }
}

impl BudgetVec<u8> {
    /// Freezes an exact-capacity large-value buffer without a shrinking allocation.
    pub(crate) fn freeze_box(self) -> super::Reserved<Box<[u8]>> {
        assert_eq!(
            self.len(),
            self.capacity(),
            "boxed output must already have its exact layout"
        );
        let Self { storage, lease } = self;
        let lease = lease.expect("a retained large value has an allocation charge");
        let account = lease.account.clone();
        let mut retained = super::Retained::new(&account);
        lease.retain(&mut retained);
        super::Reserved::from_retained(storage.into_boxed_slice(), retained)
    }
}

impl BudgetVec<u8> {
    /// Lets zstd initialize spare capacity through its bounded Vec WriteBuf implementation.
    /// This narrow dependency boundary preserves capacity without zero-filling each block.
    pub(crate) fn compress_into(
        &mut self,
        context: &mut super::Reserved<zstd::bulk::Compressor<'static>>,
        input: &[u8],
    ) -> std::io::Result<usize> {
        self.clear();
        let capacity = self.capacity();
        let result = context.payload.compress_to_buffer(input, &mut self.storage);
        assert_eq!(self.capacity(), capacity, "zstd output cannot grow");
        self.check();
        result
    }

    /// Decodes through zstd's bounded spare-capacity writer while keeping storage charged.
    pub(crate) fn decompress_into(
        &mut self,
        context: &mut super::Reserved<zstd::bulk::Decompressor<'static>>,
        input: &[u8],
    ) -> std::io::Result<usize> {
        self.clear();
        let capacity = self.capacity();
        let result = context
            .payload
            .decompress_to_buffer(input, &mut self.storage);
        assert_eq!(self.capacity(), capacity, "zstd output cannot grow");
        self.check();
        result
    }
}
