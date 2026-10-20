//! Capacity-bounded storage that keeps its allocation charge until storage drops or transfers.

use std::{
    collections::BinaryHeap,
    ops::{Deref, DerefMut},
};

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
        let lease = self.lease.as_mut().expect("attached buffer");
        if self.storage.capacity() == 0 && lease.bytes != 0 {
            let planned = (lease.bytes / size_of::<T>() as u64) as usize;
            if capacity <= planned {
                self.storage = Vec::with_capacity(planned);
                lease.allocated((self.storage.capacity() * size_of::<T>()) as u64);
                self.check();
                return Ok(());
            }
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

    /// Charges an empty buffer's planned capacity now, allocating only on its first growth.
    /// This keeps future stripe storage beside directory admission without eager allocation.
    /// Calling it on allocated or already planned storage is a programming error.
    pub(crate) fn plan_capacity(&mut self, capacity: usize) -> Result<(), BudgetExceeded> {
        let lease = self.lease.as_ref().expect("attached buffer");
        assert!(self.storage.is_empty() && lease.bytes == 0);
        assert!(size_of::<T>() == 0 || self.storage.capacity() == 0);
        self.lease = Some(lease.account.lease(MemoryPlan::array::<T>(capacity)?)?);
        Ok(())
    }

    /// Appends an item, admitting a whole doubled replacement beside the old buffer if full.
    /// Failure preserves the original storage and contents.
    pub(crate) fn try_push(&mut self, value: T) -> Result<(), BudgetExceeded> {
        if self.len() == self.capacity() {
            self.try_grow_to(self.capacity().saturating_mul(2).max(4))?;
        }
        self.push(value);
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

    /// Clears storage that covers `capacity`, keeping its lease. Otherwise retires it before
    /// any replacement allocation and returns the bytes the replacement needs admitted.
    pub(crate) fn discard_if_too_small(&mut self, capacity: usize) -> u64 {
        if self.capacity() >= capacity {
            self.clear();
            0
        } else {
            self.discard_unless(0);
            (capacity * size_of::<T>()) as u64
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

impl<T: Ord> BudgetVec<T> {
    /// Transfers admitted storage to std's heap without allocating or changing its capacity.
    pub(crate) fn into_heap(self) -> BudgetHeap<T> {
        self.check();
        let Self { storage, lease } = self;
        BudgetHeap {
            storage: BinaryHeap::from(storage),
            lease,
        }
    }
}

/// A max heap whose backing vector and capacity remain covered by an operation's lease.
#[derive(Debug)]
pub(crate) struct BudgetHeap<T> {
    /// Storage drops before its lease returns credit, just as in `BudgetVec`.
    storage: BinaryHeap<T>,
    /// The unchanged charge of the vector transferred into this heap.
    lease: Option<Lease>,
}

impl<T: Ord> BudgetHeap<T> {
    /// Inserts without growing; exceeding the leased capacity is a programming error.
    pub(crate) fn push(&mut self, value: T) {
        assert!(
            self.storage.len() < self.capacity(),
            "heap capacity exceeded"
        );
        self.storage.push(value);
    }

    /// Removes the maximum while retaining the backing storage and its charge.
    pub(crate) fn pop(&mut self) -> Option<T> {
        self.storage.pop()
    }

    /// Borrows the maximum without changing storage.
    #[cfg(test)]
    pub(crate) fn peek(&self) -> Option<&T> {
        self.storage.peek()
    }

    /// Returns the initialized entry count without exposing the growable heap.
    pub(crate) fn len(&self) -> usize {
        self.storage.len()
    }

    /// Sorts in place and returns the same charged backing vector in ascending order.
    pub(crate) fn into_sorted_vec(self) -> BudgetVec<T> {
        BudgetVec {
            storage: self.storage.into_sorted_vec(),
            lease: self.lease,
        }
    }

    /// Whether there are no initialized entries.
    pub(crate) fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    /// Returns the unchanged leased element capacity.
    pub(crate) fn capacity(&self) -> usize {
        self.storage.capacity()
    }

    /// Drops entries while retaining storage and its charge.
    pub(crate) fn clear(&mut self) {
        self.storage.clear();
    }

    /// Drops obsolete storage before replacement, reusing a matching capacity.
    pub(crate) fn discard_unless(&mut self, capacity: usize) {
        if self.capacity() == capacity {
            self.clear();
        } else {
            let empty = self
                .lease
                .as_ref()
                .expect("attached heap")
                .account
                .buffer(0)
                .expect("empty storage is free")
                .into_heap();
            *self = empty;
        }
    }

    /// Clears obsolete entries and allocates exactly the newly admitted capacity.
    pub(crate) fn reset_to(&mut self, capacity: usize) -> Result<(), BudgetExceeded> {
        self.discard_unless(capacity);
        if self.capacity() < capacity {
            *self = self
                .lease
                .as_ref()
                .expect("attached heap")
                .account
                .buffer(capacity)?
                .into_heap();
        }
        Ok(())
    }
}

impl Retained<'_> {
    /// Moves a charged vector into a public graph's fixed vector field during sealing.
    /// The consuming finalizer must not grow it or let it escape the retained graph.
    pub(crate) fn vector<T>(&mut self, buffer: BudgetVec<T>) -> Vec<T> {
        buffer.check();
        if let Some(lease) = &buffer.lease {
            assert!(lease.account.same_envelope(self.account));
        }
        let BudgetVec { storage, lease } = buffer;
        if let Some(lease) = lease {
            lease.retain(self);
        }
        storage
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

/// Fixed-capacity child storage whose aggregate owner holds its charge.
/// Only a consuming finalizer may detach it from a charged buffer.
#[derive(Debug)]
pub(crate) struct FixedVec<T> {
    /// Storage must remain inside the graph sealed by its transfer claim.
    storage: Vec<T>,
}

impl Retained<'_> {
    /// Admits a fixed child and transfers its charge to this graph before returning storage.
    pub(crate) fn buffer<T>(&mut self, capacity: usize) -> Result<FixedVec<T>, BudgetExceeded> {
        let buffer = self.account.buffer(capacity)?;
        Ok(FixedVec {
            storage: self.vector(buffer),
        })
    }
}

impl<T> FixedVec<T> {
    /// Appends within the immutable allocation capacity.
    pub(crate) fn push(&mut self, value: T) {
        assert!(self.len() < self.capacity());
        self.storage.push(value);
    }

    /// Appends only within the immutable allocation capacity.
    pub(crate) fn extend(&mut self, values: impl IntoIterator<Item = T>) {
        for value in values {
            self.push(value);
        }
    }

    /// Copies a batch after one capacity check, without per-byte append overhead.
    pub(crate) fn extend_from_slice(&mut self, values: &[T])
    where
        T: Copy,
    {
        assert!(values.len() <= self.capacity() - self.len());
        self.storage.extend_from_slice(values);
    }

    /// Clears elements while the aggregate owner retains this allocation's charge.
    pub(crate) fn clear(&mut self) {
        self.storage.clear();
    }

    /// Fills within the immutable allocation capacity.
    pub(crate) fn resize(&mut self, len: usize, value: T)
    where
        T: Copy,
    {
        assert!(len <= self.capacity());
        self.storage.resize(len, value);
    }

    /// Returns allocated element capacity, without exposing a growable handle.
    pub(crate) fn capacity(&self) -> usize {
        self.storage.capacity()
    }
}

impl<T> Deref for FixedVec<T> {
    type Target = [T];
    /// Borrows initialized children while their enclosing owner holds the charge.
    fn deref(&self) -> &[T] {
        &self.storage
    }
}

impl<T> DerefMut for FixedVec<T> {
    /// Mutates initialized children without changing allocation capacity.
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.storage
    }
}

/// A consuming iterator that keeps the array's charge until its remaining storage drops.
pub(crate) struct BudgetIntoIter<T> {
    /// The allocation and any unconsumed elements drop before their lease.
    storage: std::vec::IntoIter<T>,
    /// The original array's charge, retained even after its final element is yielded.
    _lease: Option<Lease>,
}

impl<T> IntoIterator for BudgetVec<T> {
    type Item = T;
    type IntoIter = BudgetIntoIter<T>;
    /// Moves elements while keeping the array allocation charged until the iterator drops.
    fn into_iter(self) -> Self::IntoIter {
        BudgetIntoIter {
            storage: self.storage.into_iter(),
            _lease: self.lease,
        }
    }
}

impl<T> Iterator for BudgetIntoIter<T> {
    type Item = T;
    /// Yields the next element without retiring the backing array early.
    fn next(&mut self) -> Option<T> {
        self.storage.next()
    }
    /// Preserves the exact remaining length for bounded consumers.
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.storage.size_hint()
    }
}

impl<T> ExactSizeIterator for BudgetIntoIter<T> {}

impl<'a, T> IntoIterator for &'a BudgetVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    /// Borrows the initialized prefix without changing its allocation or charge.
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut BudgetVec<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    /// Mutably borrows the initialized prefix while preserving its admitted capacity.
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<'a, T> IntoIterator for &'a FixedVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    /// Borrows children while their aggregate owner retains the charge.
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T> IntoIterator for &'a mut FixedVec<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    /// Mutably borrows children without exposing their allocation handles.
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
