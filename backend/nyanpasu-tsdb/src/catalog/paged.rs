//! Copy-on-write paged arrays. They let two versions of the catalog, or of the head, share
//! every page neither has changed.
//!
//! An array is a spine of pages: each page sits behind its own `Arc`, and so does the spine.
//! Cloning an array clones one pointer. A write copies the spine and the page it lands on
//! when another version still holds them, and writes in place otherwise, so a version
//! derived from another costs the pages it changed, not the whole array (§7.1).
//!
//! # Page size
//!
//! A full page is one 4 KiB allocation, its `Arc` header included:
//!
//! - Copying a page costs 4 KiB. A batch copies at most one page per array it writes, plus
//!   one per hash-table slot it writes, and the copies count against the catalog budget at
//!   peak (§5.1). Smaller pages would copy less per write but cost more per byte stored.
//! - Each page costs a 16-byte spine slot and a 16-byte header, under 1% of 4 KiB; at 1
//!   KiB it would be 3%, and at 512 bytes 6%.
//! - 4,096 bytes is a size class of every common allocator, so none rounds a full page up.
//!
//! Small arrays do not pay a whole page: the first page starts at 64 bytes and doubles as
//! the array grows, up to the full size. Most label dimensions hold a handful of values
//! (§14.1), so an almost empty dimension costs a few hundred bytes instead of 16 KiB.
//!
//! # Accounting
//!
//! Every page and spine a write allocates or drops goes through a [`Ledger`], which counts
//! the bytes the writer holds beyond the version it started from. Each write has a `*_cost`
//! twin that says, without changing anything, how many bytes the write would allocate, so
//! the catalog can check its budget before it writes (§5.2). A cost is exact: the ledger
//! allocates precisely that much during the write.

use std::{iter, sync::Arc};

use crate::budget::{BudgetExceeded, Lease, MemoryAccount, MemoryPlan, Reservation};

/// The size of an `Arc`'s header: its strong and weak counts, which precede the data.
pub(crate) const ARC_HEADER: usize = 2 * size_of::<usize>();

/// The allocation of a full page, header included.
pub(crate) const PAGE_ALLOC: usize = 4096;

/// The data of a full page. 4,080 is a multiple of every element size the catalog stores
/// (1, 4, 8 and 12 bytes), so a full page is exactly [`PAGE_ALLOC`] bytes.
pub(crate) const PAGE_PAYLOAD: usize = PAGE_ALLOC - ARC_HEADER;

/// The data of the smallest page, a 64-byte allocation.
pub(crate) const FIRST_PAGE_PAYLOAD: usize = 64 - ARC_HEADER;

/// The bytes of an `Arc<[T]>` of `len` elements: the header, the data, and the padding that
/// rounds the allocation to the header's alignment.
pub(crate) const fn page_bytes<T>(len: usize) -> u64 {
    (ARC_HEADER + len * size_of::<T>()).next_multiple_of(align_of::<usize>()) as u64
}

/// The bytes of a spine of `count` pages of `T`: a header and one fat pointer per page.
pub(crate) const fn spine_bytes<T>(count: usize) -> u64 {
    (ARC_HEADER + count * size_of::<Arc<[T]>>()) as u64
}

/// The bytes a writer holds beyond the version it started from: the spines and pages it
/// copied or created, minus those it dropped again.
///
/// A page is the writer's own exactly when no other version holds it, so a page shared
/// with the base version is never counted, and dropping one frees nothing.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    owned: u64,
    peak: u64,
    allocated: u64,
    /// A draft's high-water staging charge; loaders use the resident policy alone.
    copies: Option<Lease>,
}

impl Ledger {
    /// Starts a draft's staging owner before any of its structure or pages are copied.
    pub(crate) fn staging(account: &MemoryAccount) -> Result<Self, BudgetExceeded> {
        Ok(Self {
            copies: Some(account.lease(MemoryPlan::default())?),
            ..Self::default()
        })
    }

    /// Admits a release's copies before any batch or catalog mutation can occur.
    pub(crate) fn reserve_copies(&mut self, bytes: u64) -> Result<(), BudgetExceeded> {
        if let Some(copies) = &mut self.copies {
            copies.try_cover(MemoryPlan::bytes(self.owned.saturating_add(bytes)))?;
        }
        Ok(())
    }

    /// The draft's staging owner, also covering its transient serialized label values.
    pub(super) fn account(&self) -> &MemoryAccount {
        self.copies
            .as_ref()
            .expect("a draft has a staging owner")
            .account()
    }

    /// Moves the draft's staging allowance to the publication owner.
    pub(crate) fn finish(self) -> Reservation {
        self.copies
            .expect("a draft has a staging owner")
            .into_reservation()
    }

    /// The bytes held now.
    pub(crate) fn owned(&self) -> u64 {
        self.owned
    }

    /// The most bytes held at any moment, transient copies included.
    pub(crate) fn peak(&self) -> u64 {
        self.peak
    }

    /// Every byte ever allocated. Its growth across a write is that write's cost.
    pub(crate) fn allocated(&self) -> u64 {
        self.allocated
    }

    /// Charges an allocation before its storage is created, retaining the draft's high water.
    pub(crate) fn alloc(&mut self, bytes: u64) -> Result<(), BudgetExceeded> {
        self.reserve_copies(bytes)?;
        self.owned += bytes;
        self.allocated += bytes;
        self.peak = self.peak.max(self.owned);
        Ok(())
    }

    fn free(&mut self, bytes: u64) {
        self.owned -= bytes;
    }
}

/// Pages behind a shared spine. [`Paged`] keeps every page but the last full; the
/// dictionary's heap sizes its pages itself.
pub(crate) struct Pages<T> {
    spine: Option<Arc<[Arc<[T]>]>>,
}

impl<T> Clone for Pages<T> {
    fn clone(&self) -> Self {
        Self {
            spine: self.spine.clone(),
        }
    }
}

impl<T: Copy> Pages<T> {
    pub(crate) const fn new() -> Self {
        Self { spine: None }
    }

    /// The bytes [`filled`](Self::filled) allocates.
    pub(crate) fn filled_cost(lens: &[usize]) -> u64 {
        if lens.is_empty() {
            return 0;
        }
        spine_bytes::<T>(lens.len()) + lens.iter().map(|&len| page_bytes::<T>(len)).sum::<u64>()
    }

    /// Pages of the lengths `lens`, each element `value`.
    pub(crate) fn filled(
        lens: &[usize],
        value: T,
        ledger: &mut Ledger,
    ) -> Result<Self, BudgetExceeded> {
        if lens.is_empty() {
            return Ok(Self::new());
        }
        ledger.alloc(Self::filled_cost(lens))?;
        let spine = lens
            .iter()
            .map(|&len| iter::repeat_n(value, len).collect::<Arc<[T]>>())
            .collect();
        Ok(Self { spine: Some(spine) })
    }

    pub(crate) fn count(&self) -> usize {
        self.spine.as_ref().map_or(0, |spine| spine.len())
    }

    pub(crate) fn page(&self, index: usize) -> &[T] {
        &self.spine()[index]
    }

    pub(crate) fn last(&self) -> Option<&[T]> {
        self.spine.as_ref()?.last().map(|page| &**page)
    }

    /// The bytes of the spine and every page.
    pub(crate) fn bytes(&self) -> u64 {
        self.spine.as_ref().map_or(0, |spine| {
            spine_bytes::<T>(spine.len())
                + spine
                    .iter()
                    .map(|page| page_bytes::<T>(page.len()))
                    .sum::<u64>()
        })
    }

    /// The bytes [`page_mut`](Self::page_mut) allocates for page `index`.
    pub(crate) fn write_cost(&self, index: usize) -> u64 {
        let spine = self.spine();
        let page = &spine[index];
        // Copying the spine shares every page with the old spine, so each then needs its
        // own copy too.
        let spine_shared = Arc::strong_count(spine) > 1;
        let page_cost = if spine_shared || Arc::strong_count(page) > 1 {
            page_bytes::<T>(page.len())
        } else {
            0
        };
        self.spine_copy_cost() + page_cost
    }

    /// Page `index`, writable. Copies the spine and the page first if another version holds
    /// them.
    pub(crate) fn page_mut(
        &mut self,
        index: usize,
        ledger: &mut Ledger,
    ) -> Result<&mut [T], BudgetExceeded> {
        let page = &mut self.spine_mut(ledger)?[index];
        if Arc::get_mut(page).is_none() {
            ledger.alloc(page_bytes::<T>(page.len()))?;
            *page = Arc::from(&page[..]);
        }
        Ok(Arc::get_mut(page).expect("a page just copied has no other holder"))
    }

    /// The bytes [`replace`](Self::replace) allocates besides the new page.
    pub(crate) fn spine_copy_cost(&self) -> u64 {
        match &self.spine {
            Some(spine) if Arc::strong_count(spine) > 1 => spine_bytes::<T>(spine.len()),
            _ => 0,
        }
    }

    /// Replaces page `index` with `page`, whose bytes the caller has already allocated
    /// through `ledger`.
    pub(crate) fn replace(
        &mut self,
        index: usize,
        page: Arc<[T]>,
        ledger: &mut Ledger,
    ) -> Result<(), BudgetExceeded> {
        let old = std::mem::replace(&mut self.spine_mut(ledger)?[index], page);
        drop_page(old, ledger);
        Ok(())
    }

    /// The bytes [`push`](Self::push) allocates besides the new page.
    pub(crate) fn push_spine_cost(&self) -> u64 {
        spine_bytes::<T>(self.count() + 1)
    }

    /// Appends `page`, whose bytes the caller has already allocated through `ledger`. The
    /// spine is a single allocation, so it is rebuilt one page longer.
    pub(crate) fn push(
        &mut self,
        page: Arc<[T]>,
        ledger: &mut Ledger,
    ) -> Result<(), BudgetExceeded> {
        ledger.alloc(self.push_spine_cost())?;
        let spine: Arc<[Arc<[T]>]> = match &self.spine {
            Some(spine) => spine.iter().cloned().chain(iter::once(page)).collect(),
            None => iter::once(page).collect(),
        };
        if let Some(old) = self.spine.replace(spine) {
            drop_spine(old, ledger);
        }
        Ok(())
    }

    /// Drops the array, freeing in `ledger` what only it held.
    pub(crate) fn discard(self, ledger: &mut Ledger) {
        if let Some(spine) = self.spine {
            drop_spine(spine, ledger);
        }
    }

    fn spine(&self) -> &Arc<[Arc<[T]>]> {
        self.spine
            .as_ref()
            .expect("an array with pages has a spine")
    }

    fn spine_mut(&mut self, ledger: &mut Ledger) -> Result<&mut [Arc<[T]>], BudgetExceeded> {
        let spine = self
            .spine
            .as_mut()
            .expect("an array with pages has a spine");
        if Arc::get_mut(spine).is_none() {
            ledger.alloc(spine_bytes::<T>(spine.len()))?;
            *spine = Arc::from(&spine[..]);
        }
        Ok(Arc::get_mut(spine).expect("a spine just copied has no other holder"))
    }
}

#[cfg(test)]
impl<T: Copy> Pages<T> {
    /// The bytes of this array that `base` does not share: its spine unless both hold the
    /// same one, and each page `base` does not hold at the same index.
    pub(crate) fn unshared(&self, base: &Self) -> u64 {
        let Some(spine) = &self.spine else {
            return 0;
        };
        if base
            .spine
            .as_ref()
            .is_some_and(|base| Arc::ptr_eq(spine, base))
        {
            return 0;
        }
        let base_page = |index: usize| base.spine.as_ref().and_then(|base| base.get(index));
        spine_bytes::<T>(spine.len())
            + spine
                .iter()
                .enumerate()
                .filter(|&(index, page)| !base_page(index).is_some_and(|b| Arc::ptr_eq(page, b)))
                .map(|(_, page)| page_bytes::<T>(page.len()))
                .sum::<u64>()
    }

    /// Whether both arrays hold the same spine, and so every page.
    pub(crate) fn same(&self, other: &Self) -> bool {
        match (&self.spine, &other.spine) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

/// Drops a page, freeing it in `ledger` if no other version holds it.
fn drop_page<T>(page: Arc<[T]>, ledger: &mut Ledger) {
    if Arc::strong_count(&page) == 1 {
        ledger.free(page_bytes::<T>(page.len()));
    }
}

/// Drops a spine. Only a spine no other version holds frees anything: then its pages are
/// held by it alone, or shared with another version's spine.
fn drop_spine<T>(spine: Arc<[Arc<[T]>]>, ledger: &mut Ledger) {
    if Arc::strong_count(&spine) == 1 {
        ledger.free(spine_bytes::<T>(spine.len()));
        for page in spine.iter() {
            // A page this spine alone holds goes with it.
            if Arc::strong_count(page) == 1 {
                ledger.free(page_bytes::<T>(page.len()));
            }
        }
    }
}

/// An array of `T` in pages of [`PAGE_PAYLOAD`] bytes. Every page but the last is full.
pub(crate) struct Paged<T> {
    pages: Pages<T>,
    len: usize,
}

impl<T> Clone for Paged<T> {
    fn clone(&self) -> Self {
        Self {
            pages: self.pages.clone(),
            len: self.len,
        }
    }
}

/// How a push makes room for its element.
enum Push {
    /// The last page has room.
    InPlace,
    /// The last page grows to this many elements.
    Grow(usize),
    /// A new page of this many elements.
    NewPage(usize),
}

impl<T: Copy + Default> Paged<T> {
    /// Elements in a full page.
    pub(crate) const PER_PAGE: usize = PAGE_PAYLOAD / size_of::<T>();

    /// Elements in the first page of a growing array.
    const FIRST_PAGE: usize = {
        let len = FIRST_PAGE_PAYLOAD / size_of::<T>();
        if len == 0 { 1 } else { len }
    };

    pub(crate) const fn new() -> Self {
        Self {
            pages: Pages::new(),
            len: 0,
        }
    }

    /// The bytes [`filled`](Self::filled) allocates.
    pub(crate) fn filled_cost(len: usize) -> u64 {
        if len == 0 {
            return 0;
        }
        let pages = len.div_ceil(Self::PER_PAGE);
        let last = len - (pages - 1) * Self::PER_PAGE;
        spine_bytes::<T>(pages)
            + (pages as u64 - 1) * page_bytes::<T>(Self::PER_PAGE)
            + page_bytes::<T>(last)
    }

    /// An array of `len` copies of `value`, its pages full but the last.
    pub(crate) fn filled(
        len: usize,
        value: T,
        ledger: &mut Ledger,
    ) -> Result<Self, BudgetExceeded> {
        if len == 0 {
            return Ok(Self::new());
        }
        ledger.alloc(Self::filled_cost(len))?;
        let pages = len.div_ceil(Self::PER_PAGE);
        let spine = (0..pages)
            .map(|page| {
                let page_len = Self::PER_PAGE.min(len - page * Self::PER_PAGE);
                iter::repeat_n(value, page_len).collect::<Arc<[T]>>()
            })
            .collect();
        Ok(Self {
            pages: Pages { spine: Some(spine) },
            len,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// # Panics
    ///
    /// If `index` is out of bounds.
    pub(crate) fn get(&self, index: usize) -> T {
        assert!(
            index < self.len,
            "index {index} past an array of {}",
            self.len
        );
        self.pages.page(index / Self::PER_PAGE)[index % Self::PER_PAGE]
    }

    /// The bytes [`set`](Self::set) allocates. `index` may also be a free slot of the last
    /// page, which is where a push writes.
    pub(crate) fn set_cost(&self, index: usize) -> u64 {
        self.pages.write_cost(index / Self::PER_PAGE)
    }

    /// # Panics
    ///
    /// If `index` is out of bounds.
    pub(crate) fn set(
        &mut self,
        index: usize,
        value: T,
        ledger: &mut Ledger,
    ) -> Result<(), BudgetExceeded> {
        assert!(
            index < self.len,
            "index {index} past an array of {}",
            self.len
        );
        self.pages.page_mut(index / Self::PER_PAGE, ledger)?[index % Self::PER_PAGE] = value;
        Ok(())
    }

    /// The bytes [`push`](Self::push) allocates.
    pub(crate) fn push_cost(&self) -> u64 {
        match self.push_plan() {
            Push::InPlace => self.set_cost(self.len),
            Push::Grow(len) => self.pages.spine_copy_cost() + page_bytes::<T>(len),
            Push::NewPage(len) => self.pages.push_spine_cost() + page_bytes::<T>(len),
        }
    }

    pub(crate) fn push(&mut self, value: T, ledger: &mut Ledger) -> Result<(), BudgetExceeded> {
        match self.push_plan() {
            Push::InPlace => {}
            Push::Grow(len) => {
                ledger.alloc(page_bytes::<T>(len))?;
                let last = self.pages.count() - 1;
                let old = self.pages.page(last);
                let page = old
                    .iter()
                    .copied()
                    .chain(iter::repeat_n(T::default(), len - old.len()))
                    .collect();
                self.pages.replace(last, page, ledger)?;
            }
            Push::NewPage(len) => {
                ledger.alloc(page_bytes::<T>(len))?;
                self.pages
                    .push(iter::repeat_n(T::default(), len).collect(), ledger)?;
            }
        }
        let index = self.len;
        self.len += 1;
        self.pages.page_mut(index / Self::PER_PAGE, ledger)?[index % Self::PER_PAGE] = value;
        Ok(())
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.pages.bytes()
    }

    /// Drops the array, freeing in `ledger` what only it held.
    pub(crate) fn discard(self, ledger: &mut Ledger) {
        self.pages.discard(ledger);
    }

    fn capacity(&self) -> usize {
        match self.pages.last() {
            None => 0,
            Some(last) => (self.pages.count() - 1) * Self::PER_PAGE + last.len(),
        }
    }

    fn push_plan(&self) -> Push {
        if self.len < self.capacity() {
            return Push::InPlace;
        }
        // A filled array's last page holds only its elements, so the last page, not just
        // a growing first one, may be short.
        match self.pages.last() {
            None => Push::NewPage(Self::FIRST_PAGE.min(Self::PER_PAGE)),
            Some(last) if last.len() < Self::PER_PAGE => {
                Push::Grow((2 * last.len()).min(Self::PER_PAGE))
            }
            Some(_) => Push::NewPage(Self::PER_PAGE),
        }
    }
}

#[cfg(test)]
impl<T: Copy + Default> Paged<T> {
    pub(crate) fn unshared(&self, base: &Self) -> u64 {
        self.pages.unshared(&base.pages)
    }

    pub(crate) fn same(&self, other: &Self) -> bool {
        self.pages.same(&other.pages)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Applies `write` and checks that it allocated exactly `cost`.
    fn exact<R>(ledger: &mut Ledger, cost: u64, write: impl FnOnce(&mut Ledger) -> R) -> R {
        let before = ledger.allocated();
        let result = write(ledger);
        assert_eq!(ledger.allocated() - before, cost, "predicted cost");
        result
    }

    /// Refusing a shared page's charge happens before the page is copied or changed.
    #[test]
    fn a_page_copy_is_refused_before_its_allocation() {
        let base = Paged::filled(2 * Paged::<u32>::PER_PAGE, 7, &mut Ledger::default()).unwrap();
        let mut next = base.clone();
        let spine = spine_bytes::<u32>(2);
        let budget = crate::budget::MemoryBudget::new(spine + PAGE_ALLOC as u64 - 1);
        let account = MemoryAccount::new(&budget, "staging", MemoryPlan::default()).unwrap();
        let mut ledger = Ledger::staging(&account).unwrap();
        let error = next.set(0, 9, &mut ledger).unwrap_err();
        assert_eq!(
            (error.needed, error.available),
            (PAGE_ALLOC as u64, PAGE_ALLOC as u64 - 1)
        );
        assert_eq!(ledger.allocated(), spine);
        assert_eq!(budget.reserved(), spine);
        assert!(Arc::ptr_eq(&next.pages.spine()[0], &base.pages.spine()[0]));
        assert_eq!(next.get(0), 7);
        assert_eq!(base.get(0), 7);
        drop(next);
        drop(ledger);
        drop(account);
        assert_eq!(budget.reserved(), 0);
    }

    #[test]
    fn full_pages_are_one_allocation_class() {
        assert_eq!(page_bytes::<u8>(Paged::<u8>::PER_PAGE), PAGE_ALLOC as u64);
        assert_eq!(page_bytes::<u32>(Paged::<u32>::PER_PAGE), PAGE_ALLOC as u64);
        assert_eq!(page_bytes::<u64>(Paged::<u64>::PER_PAGE), PAGE_ALLOC as u64);
        assert_eq!(
            page_bytes::<[u32; 3]>(Paged::<[u32; 3]>::PER_PAGE),
            PAGE_ALLOC as u64
        );
        assert_eq!(page_bytes::<u32>(Paged::<u32>::FIRST_PAGE), 64);
        assert_eq!(page_bytes::<[u32; 3]>(Paged::<[u32; 3]>::FIRST_PAGE), 64);
        // A dedicated page of an odd length is padded to the header's alignment.
        assert_eq!(page_bytes::<u8>(5), 24);
    }

    #[test]
    fn the_first_page_doubles_until_it_is_full() {
        let mut ledger = Ledger::default();
        let mut array = Paged::<u32>::new();
        let mut page_lens = Vec::new();
        for value in 0..2 * Paged::<u32>::PER_PAGE as u32 + 1 {
            exact(&mut ledger, array.push_cost(), |ledger| {
                array.push(value, ledger).unwrap()
            });
            let lens: Vec<usize> = (0..array.pages.count())
                .map(|page| array.pages.page(page).len())
                .collect();
            if page_lens.last() != Some(&lens) {
                page_lens.push(lens);
            }
        }
        assert_eq!(
            page_lens,
            [
                vec![12],
                vec![24],
                vec![48],
                vec![96],
                vec![192],
                vec![384],
                vec![768],
                vec![1020],
                vec![1020, 1020],
                vec![1020, 1020, 1020],
            ]
        );
        for index in 0..array.len() {
            assert_eq!(array.get(index), index as u32);
        }
        // The pages replaced while growing were the writer's own, so they were freed: what
        // it holds is exactly the final array.
        assert_eq!(ledger.owned(), array.bytes());
        assert_eq!(array.bytes(), spine_bytes::<u32>(3) + 3 * PAGE_ALLOC as u64);
    }

    /// A filled array's last page is as short as its elements; a push grows it before
    /// starting another page, so every page but the last stays full.
    #[test]
    fn a_push_fills_a_short_last_page_of_a_filled_array() {
        type Span = [u32; 3];
        let per_page = Paged::<Span>::PER_PAGE;
        let mut ledger = Ledger::default();
        let mut array = Paged::filled(per_page + 269, Span::default(), &mut ledger).unwrap();
        for value in 0..per_page as u32 {
            exact(&mut ledger, array.push_cost(), |ledger| {
                array.push([value; 3], ledger).unwrap()
            });
        }
        let lens: Vec<usize> = (0..array.pages.count())
            .map(|page| array.pages.page(page).len())
            .collect();
        assert_eq!(lens, [per_page, per_page, per_page]);
        for index in 0..per_page {
            assert_eq!(array.get(per_page + 269 + index), [index as u32; 3]);
        }
        assert_eq!(ledger.owned(), array.bytes());
    }

    #[test]
    fn a_write_to_a_shared_array_copies_its_spine_and_one_page() {
        let mut ledger = Ledger::default();
        let base = Paged::filled(3 * Paged::<u64>::PER_PAGE, 7u64, &mut ledger).unwrap();
        let mut next = base.clone();
        let mut ledger = Ledger::default();

        let index = Paged::<u64>::PER_PAGE + 5;
        let cost = next.set_cost(index);
        assert_eq!(cost, spine_bytes::<u64>(3) + PAGE_ALLOC as u64);
        exact(&mut ledger, cost, |ledger| {
            next.set(index, 9, ledger).unwrap()
        });
        assert_eq!(next.get(index), 9);
        assert_eq!(base.get(index), 7);
        assert_eq!(next.unshared(&base), cost);
        assert_eq!(ledger.owned(), cost);

        // The copies are the writer's own now: writing them again copies nothing.
        assert_eq!(next.set_cost(index + 1), 0);
        exact(&mut ledger, 0, |ledger| {
            next.set(index + 1, 9, ledger).unwrap()
        });
        // Another page of the shared array still needs a copy, but not the spine.
        assert_eq!(next.set_cost(0), PAGE_ALLOC as u64);
    }

    #[test]
    fn discarding_frees_only_what_no_other_version_holds() {
        let mut ledger = Ledger::default();
        let base = Paged::filled(2 * Paged::<u32>::PER_PAGE, 1u32, &mut ledger).unwrap();
        let mut next = base.clone();
        let mut ledger = Ledger::default();
        next.set(0, 2, &mut ledger).unwrap();
        assert_eq!(ledger.owned(), spine_bytes::<u32>(2) + PAGE_ALLOC as u64);
        next.discard(&mut ledger);
        assert_eq!(ledger.owned(), 0);
        assert_eq!(base.get(0), 1);

        // An array the writer shares only with itself frees nothing until the last holder
        // drops it.
        let mut ledger = Ledger::default();
        let own = Paged::filled(10, 0u32, &mut ledger).unwrap();
        let other = own.clone();
        own.discard(&mut ledger);
        assert_eq!(ledger.owned(), Paged::<u32>::filled_cost(10));
        other.discard(&mut ledger);
        assert_eq!(ledger.owned(), 0);
    }

    #[derive(Clone, Debug)]
    enum Op {
        Push(u32),
        Set(usize, u32),
        /// Publish the working array as a new base.
        Publish,
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            4 => any::<u32>().prop_map(Op::Push),
            3 => (any::<usize>(), any::<u32>()).prop_map(|(index, value)| Op::Set(index, value)),
            1 => Just(Op::Publish),
        ]
    }

    proptest! {
        // Every write allocates exactly its predicted cost, and what the ledger says the
        // writer holds is exactly what the base does not share.
        #[test]
        fn costs_are_exact_and_the_ledger_matches_sharing(
            ops in prop::collection::vec(op(), 1..3000),
        ) {
            let mut base = Paged::<u32>::new();
            let mut next = base.clone();
            let mut ledger = Ledger::default();
            let mut model = Vec::new();
            for op in ops {
                match op {
                    Op::Push(value) => {
                        exact(&mut ledger, next.push_cost(), |ledger| next.push(value, ledger).unwrap());
                        model.push(value);
                    }
                    Op::Set(index, value) if !model.is_empty() => {
                        let index = index % model.len();
                        exact(&mut ledger, next.set_cost(index), |ledger| {
                            next.set(index, value, ledger).unwrap()
                        });
                        model[index] = value;
                    }
                    Op::Set(..) => {}
                    Op::Publish => {
                        base = next.clone();
                        ledger = Ledger::default();
                    }
                }
                prop_assert_eq!(ledger.owned(), next.unshared(&base));
                prop_assert!(ledger.peak() >= ledger.owned());
            }
            prop_assert_eq!(next.len(), model.len());
            for (index, &value) in model.iter().enumerate() {
                prop_assert_eq!(next.get(index), value);
            }
        }
    }
}
