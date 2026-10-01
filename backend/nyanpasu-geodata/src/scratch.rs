//! Build buffers outside the heap.
use std::marker::PhantomData;

use bytemuck::Pod;
use memmap2::MmapMut;

use crate::{GeoError, GeoResult};

/// A growable array of plain values in anonymous memory. The system allocator
/// keeps freed heap pages for reuse, so a heap-backed build would leave its
/// peak behind as the process's footprint; these pages return to the OS when
/// the buffer grows or drops.
pub(crate) struct ScratchVec<T> {
    map: Option<MmapMut>,
    len: usize,
    _values: PhantomData<T>,
}

impl<T: Pod> ScratchVec<T> {
    pub(crate) fn new() -> Self {
        Self {
            map: None,
            len: 0,
            _values: PhantomData,
        }
    }

    pub(crate) fn with_capacity(capacity: usize) -> GeoResult<Self> {
        let bytes = capacity
            .checked_mul(size_of::<T>())
            .ok_or(GeoError::TooMany("build values"))?;
        let map = match bytes {
            0 => None,
            bytes => Some(MmapMut::map_anon(bytes).map_err(GeoError::Memory)?),
        };
        Ok(Self {
            map,
            len: 0,
            _values: PhantomData,
        })
    }

    /// `len` zeroed values.
    pub(crate) fn zeroed(len: usize) -> GeoResult<Self> {
        // Anonymous pages start zeroed.
        let mut values = Self::with_capacity(len)?;
        values.len = len;
        Ok(values)
    }

    pub(crate) fn push(&mut self, value: T) -> GeoResult<()> {
        if self.len == self.capacity() {
            let page = 4096 / size_of::<T>();
            let mut grown = Self::with_capacity((self.len * 2).max(page).max(1))?;
            grown.whole()[..self.len].copy_from_slice(self.as_slice());
            grown.len = self.len;
            *self = grown;
        }
        let len = self.len;
        self.whole()[len] = value;
        self.len += 1;
        Ok(())
    }

    pub(crate) fn extend_from_slice(&mut self, values: &[T]) -> GeoResult<()> {
        values.iter().try_for_each(|value| self.push(*value))
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        let value = *self.as_slice().last()?;
        self.len -= 1;
        Some(value)
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        match &self.map {
            Some(map) => bytemuck::cast_slice(&map[..self.len * size_of::<T>()]),
            None => &[],
        }
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        let len = self.len;
        &mut self.whole()[..len]
    }

    fn capacity(&self) -> usize {
        self.map
            .as_ref()
            .map_or(0, |map| map.len() / size_of::<T>())
    }

    /// Every slot, including those past `len`.
    fn whole(&mut self) -> &mut [T] {
        let capacity = self.capacity();
        match &mut self.map {
            Some(map) => bytemuck::cast_slice_mut(&mut map[..capacity * size_of::<T>()]),
            None => &mut [],
        }
    }
}

/// Open-addressing map from `u64` keys below `u64::MAX` to `u32` values,
/// kept off the heap like `ScratchVec`.
pub(crate) struct ScratchMap {
    /// Key plus one; zero marks an empty slot.
    keys: ScratchVec<u64>,
    values: ScratchVec<u32>,
    len: usize,
}

impl ScratchMap {
    pub(crate) fn new() -> Self {
        Self {
            keys: ScratchVec::new(),
            values: ScratchVec::new(),
            len: 0,
        }
    }

    pub(crate) fn get(&self, key: u64) -> Option<u32> {
        if self.len == 0 {
            return None;
        }
        let stored = key + 1;
        let keys = self.keys.as_slice();
        let mut slot = self.slot(stored);
        loop {
            match keys[slot] {
                0 => return None,
                found if found == stored => return Some(self.values.as_slice()[slot]),
                _ => slot = (slot + 1) & (keys.len() - 1),
            }
        }
    }

    pub(crate) fn insert(&mut self, key: u64, value: u32) -> GeoResult<()> {
        // Half full at most, so probes stay short and always find a free slot.
        if (self.len + 1) * 2 > self.keys.len() {
            self.grow()?;
        }
        let stored = key + 1;
        let mut slot = self.slot(stored);
        loop {
            match self.keys.as_slice()[slot] {
                0 => {
                    self.keys.as_mut_slice()[slot] = stored;
                    self.len += 1;
                    break;
                }
                found if found == stored => break,
                _ => slot = (slot + 1) & (self.keys.len() - 1),
            }
        }
        self.values.as_mut_slice()[slot] = value;
        Ok(())
    }

    /// The table length is a power of two.
    fn slot(&self, stored: u64) -> usize {
        (stored.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) as usize & (self.keys.len() - 1)
    }

    fn grow(&mut self) -> GeoResult<()> {
        let capacity = (self.keys.len() * 2).max(1024);
        let old = std::mem::replace(
            self,
            Self {
                keys: ScratchVec::zeroed(capacity)?,
                values: ScratchVec::zeroed(capacity)?,
                len: 0,
            },
        );
        for (stored, value) in old.keys.as_slice().iter().zip(old.values.as_slice()) {
            if *stored != 0 {
                self.insert(stored - 1, *value)?;
            }
        }
        Ok(())
    }
}

impl Default for ScratchMap {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Pod> Default for ScratchVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushed_values_survive_growth() {
        let mut values = ScratchVec::<u128>::new();
        for i in 0..100_000u128 {
            values.push(i * 3).unwrap();
        }

        assert_eq!(values.len(), 100_000);
        assert_eq!(values.as_slice()[0], 0);
        assert_eq!(values.as_slice()[99_999], 299_997);
        assert_eq!(values.pop(), Some(299_997));
        assert_eq!(values.len(), 99_999);
    }

    #[test]
    fn an_empty_buffer_has_no_values() {
        let mut values = ScratchVec::<u32>::new();

        assert!(values.as_slice().is_empty());
        assert_eq!(values.pop(), None);
        assert!(
            ScratchVec::<u32>::with_capacity(0)
                .unwrap()
                .as_slice()
                .is_empty()
        );
    }

    #[test]
    fn zeroed_buffers_hold_zeros_and_are_writable() {
        let mut values = ScratchVec::<u64>::zeroed(5000).unwrap();
        values.as_mut_slice()[4999] = 7;

        assert_eq!(values.len(), 5000);
        assert!(values.as_slice()[..4999].iter().all(|v| *v == 0));
        assert_eq!(values.as_slice()[4999], 7);
    }

    #[test]
    fn a_map_finds_every_inserted_key_across_growth() {
        let mut map = ScratchMap::new();
        for key in (0..50_000u64).map(|k| k * 7919) {
            map.insert(key, key as u32 ^ 0x5a5a).unwrap();
        }

        for key in (0..50_000u64).map(|k| k * 7919) {
            assert_eq!(map.get(key), Some(key as u32 ^ 0x5a5a), "{key}");
        }
        assert_eq!(map.get(1), None);
        assert_eq!(ScratchMap::new().get(0), None);
    }

    #[test]
    fn inserting_a_key_again_replaces_its_value() {
        let mut map = ScratchMap::new();
        map.insert(0, 1).unwrap();
        map.insert(0, 2).unwrap();

        assert_eq!(map.get(0), Some(2));
    }

    #[test]
    fn values_sort_in_place() {
        let mut values = ScratchVec::<u32>::with_capacity(3).unwrap();
        for v in [3, 1, 2] {
            values.push(v).unwrap();
        }
        values.as_mut_slice().sort_unstable();

        assert_eq!(values.as_slice(), &[1, 2, 3]);
    }
}
