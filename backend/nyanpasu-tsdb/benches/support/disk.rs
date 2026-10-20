//! Successful physical writes and syncs, including checkpoint and backend close I/O.

use redb::{BackendError, StorageBackend, backends::FileBackend};
use std::{
    fs::{File, OpenOptions},
    io,
    ops::Bound,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// One benchmark instance's counters, outside every store budget.
#[derive(Debug, Default)]
pub struct Counts {
    bytes: AtomicU64,
    syncs: AtomicU64,
    closes: AtomicU64,
}
impl Counts {
    /// Cumulative successful physical write bytes and sync calls.
    pub fn read(&self) -> (u64, u64) {
        (
            self.bytes.load(Ordering::Relaxed),
            self.syncs.load(Ordering::Relaxed),
        )
    }
    /// Successful backend closes, not just facade retirement.
    pub fn closes(&self) -> u64 {
        self.closes.load(Ordering::Relaxed)
    }
}
/// File backend with transparent byte-range locking and explicit instance-owned counters.
#[derive(Debug)]
pub struct Counting {
    inner: FileBackend,
    counts: Arc<Counts>,
}
impl Counting {
    /// Creates or reopens only the harness-owned file.
    pub fn open(path: &Path, counts: Arc<Counts>) -> Self {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap();
        Self {
            inner: FileBackend::new(file).unwrap(),
            counts,
        }
    }
    /// A known direct I/O sequence independently checks the counting wrapper.
    pub fn self_check() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let counts = Arc::new(Counts::default());
        let disk = Self {
            inner: FileBackend::new(
                File::options()
                    .read(true)
                    .write(true)
                    .open(temp.path())
                    .unwrap(),
            )
            .unwrap(),
            counts: counts.clone(),
        };
        disk.set_len(64).unwrap();
        disk.write(0, &[1, 2, 3]).unwrap();
        disk.write(8, &[4, 5]).unwrap();
        disk.sync_data().unwrap();
        let mut bytes = [0; 3];
        disk.read(0, &mut bytes).unwrap();
        assert_eq!(bytes, [1, 2, 3]);
        assert_eq!(counts.read(), (5, 1));
        disk.close().unwrap();
        assert_eq!(counts.closes(), 1);
    }
}
impl StorageBackend for Counting {
    fn len(&self) -> io::Result<u64> {
        self.inner.len()
    }
    fn read(&self, offset: u64, out: &mut [u8]) -> io::Result<()> {
        self.inner.read(offset, out)
    }
    fn set_len(&self, len: u64) -> io::Result<()> {
        self.inner.set_len(len)
    }
    fn write(&self, offset: u64, bytes: &[u8]) -> io::Result<()> {
        self.inner.write(offset, bytes)?;
        self.counts
            .bytes
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        Ok(())
    }
    fn sync_data(&self) -> io::Result<()> {
        self.inner.sync_data()?;
        self.counts.syncs.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn close(&self) -> io::Result<()> {
        self.inner.close()?;
        self.counts.closes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn try_lock_range(&self, start: Bound<u64>, end: Bound<u64>) -> Result<bool, BackendError> {
        self.inner.try_lock_range(start, end)
    }
    fn try_lock_shared_range(
        &self,
        start: Bound<u64>,
        end: Bound<u64>,
    ) -> Result<bool, BackendError> {
        self.inner.try_lock_shared_range(start, end)
    }
    fn lock_range(&self, start: Bound<u64>, end: Bound<u64>) -> Result<(), BackendError> {
        self.inner.lock_range(start, end)
    }
    fn lock_shared_range(&self, start: Bound<u64>, end: Bound<u64>) -> Result<(), BackendError> {
        self.inner.lock_shared_range(start, end)
    }
    fn unlock_range(&self, start: Bound<u64>, end: Bound<u64>) -> Result<(), BackendError> {
        self.inner.unlock_range(start, end)
    }
    fn query_lock_range(&self, start: Bound<u64>, end: Bound<u64>) -> Result<bool, BackendError> {
        self.inner.query_lock_range(start, end)
    }
}
