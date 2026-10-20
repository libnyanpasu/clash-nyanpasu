//! A disk image in memory under redb, which fails the I/O a test chooses: the tests' way to
//! drive redb into crashes, I/O failures and unknown commit outcomes for real.
//!
//! The disk keeps two images. The *current* one has every write so far, as the running
//! process sees the file. The *durable* one has what the last successful `sync_data` saw,
//! which is what survives a crash. Each instance opens the durable image. A clean close ends
//! with a sync, so then the two are the same; after a crash or a failed sync, what was not
//! synced is lost.

use std::{
    fmt,
    io::{self, ErrorKind},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use redb::StorageBackend;

/// A simulated disk. Clones share it.
#[derive(Clone, Default)]
pub(super) struct FaultDisk {
    disk: Arc<Mutex<Disk>>,
    /// Shared ordinal recorder, disabled for the older single-fault conformance tests.
    schedule: crate::verification::faults::FaultSchedule,
}

#[derive(Default)]
struct Disk {
    current: Vec<u8>,
    durable: Vec<u8>,
    /// Numbers the instances: only the newest one's backend works.
    generation: u64,
    /// Whether the header page, at offset 0, was written since the last sync.
    header_written: bool,
    fault: Option<Fault>,
    stats: Stats,
}

/// What the backends have done to the disk.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Stats {
    pub(super) reads: u64,
    pub(super) bytes_written: u64,
    pub(super) syncs: u64,
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    /// The next `read`, `write`, `set_len` or `sync_data` fails.
    NextIo,
    /// The first sync after a write of the header fails. redb writes the header and then
    /// syncs once to complete a durable commit (`page_manager.rs:1735-1757`), so this is
    /// that sync. When `applied`, every write reaches the durable image before it fails.
    CommitSync { applied: bool },
    /// The next `set_len` that shrinks the disk fails: the truncation with which redb rolls
    /// back the growth of an aborted transaction (`page_manager.rs:2206-2216`).
    Truncate,
}

impl FaultDisk {
    /// Independent replay image, with no graceful close of the original instance.
    pub(super) fn fork(&self) -> Self {
        let durable = self.lock().durable.clone();
        Self {
            disk: Arc::new(Mutex::new(Disk {
                current: durable.clone(),
                durable,
                ..Disk::default()
            })),
            schedule: Default::default(),
        }
    }

    /// The same controller used above the storage port; invalidation owns only disk state.
    pub(super) fn schedule(&self) -> crate::verification::faults::FaultSchedule {
        let disk = self.disk.clone();
        self.schedule.actions(
            || {},
            move || {
                lock(&disk).generation += 1;
            },
        );
        self.schedule.clone()
    }

    /// The backend of a new instance, which opens the durable image. The backend of every
    /// earlier instance stops working, as if its process had exited.
    pub(super) fn backend(&self) -> FaultBackend {
        let mut disk = self.lock();
        disk.generation += 1;
        disk.current = disk.durable.clone();
        disk.header_written = false;
        FaultBackend {
            disk: self.disk.clone(),
            generation: disk.generation,
            schedule: self.schedule.clone(),
        }
    }

    /// Crashes the running instance: from now on its backend fails every call, so nothing
    /// it writes reaches the disk, its close included.
    pub(super) fn crash(&self) {
        self.lock().generation += 1;
    }

    pub(super) fn fail_next_io(&self) {
        self.arm(Fault::NextIo);
    }

    pub(super) fn fail_commit_sync(&self, applied: bool) {
        self.arm(Fault::CommitSync { applied });
    }

    pub(super) fn fail_truncate(&self) {
        self.arm(Fault::Truncate);
    }

    /// The length of the current image.
    pub(super) fn len(&self) -> u64 {
        self.lock().current.len() as u64
    }

    /// # Panics
    ///
    /// When an armed fault has not fired: the operation it was armed for never reached the
    /// disk, so the failure the caller saw was not redb's.
    pub(super) fn assert_fault_fired(&self) {
        let fault = self.lock().fault;
        assert!(fault.is_none(), "{fault:?} never fired");
    }

    pub(super) fn stats(&self) -> Stats {
        self.lock().stats
    }

    fn arm(&self, fault: Fault) {
        let earlier = self.lock().fault.replace(fault);
        assert!(earlier.is_none(), "{earlier:?} was still armed");
    }

    fn lock(&self) -> MutexGuard<'_, Disk> {
        lock(&self.disk)
    }
}

/// Assertions run after the lock is released, so a poisoned lock still guards a consistent
/// disk.
fn lock(disk: &Mutex<Disk>) -> MutexGuard<'_, Disk> {
    disk.lock().unwrap_or_else(PoisonError::into_inner)
}

fn injected() -> io::Error {
    io::Error::other("injected I/O failure")
}

impl Disk {
    fn take_io_fault(&mut self) -> io::Result<()> {
        if matches!(self.fault, Some(Fault::NextIo)) {
            self.fault = None;
            return Err(injected());
        }
        Ok(())
    }
}

/// One instance's view of a [`FaultDisk`].
pub(super) struct FaultBackend {
    disk: Arc<Mutex<Disk>>,
    generation: u64,
    /// Injected explicitly by its owning FaultDisk.
    schedule: crate::verification::faults::FaultSchedule,
}

impl FaultBackend {
    /// A crashed generation never reaches a later write, including destruction's close I/O.
    fn step(&self, name: &'static str) -> io::Result<()> {
        drop(self.disk()?);
        self.schedule.io(name)
    }

    /// The disk, while this backend's instance is the newest.
    fn disk(&self) -> io::Result<MutexGuard<'_, Disk>> {
        let disk = lock(&self.disk);
        if disk.generation != self.generation {
            return Err(io::Error::other("the instance crashed or was replaced"));
        }
        Ok(disk)
    }
}

// The derived form would print the images.
impl fmt::Debug for FaultBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FaultBackend")
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

fn out_of_range() -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, "beyond the end of the disk")
}

// Offsets and lengths fit in `usize` on the 64-bit targets the crate supports.
impl StorageBackend for FaultBackend {
    fn close(&self) -> io::Result<()> {
        self.step("disk.close")
    }

    fn len(&self) -> io::Result<u64> {
        self.step("disk.len")?;
        Ok(self.disk()?.current.len() as u64)
    }

    fn read(&self, offset: u64, out: &mut [u8]) -> io::Result<()> {
        self.step("disk.read")?;
        let mut disk = self.disk()?;
        disk.take_io_fault()?;
        disk.stats.reads += 1;
        let start = offset as usize;
        let bytes = disk
            .current
            .get(start..start + out.len())
            .ok_or_else(out_of_range)?;
        out.copy_from_slice(bytes);
        Ok(())
    }

    fn set_len(&self, len: u64) -> io::Result<()> {
        self.step("disk.set_len")?;
        let mut disk = self.disk()?;
        disk.take_io_fault()?;
        if matches!(disk.fault, Some(Fault::Truncate)) && len < disk.current.len() as u64 {
            disk.fault = None;
            return Err(injected());
        }
        disk.current.resize(len as usize, 0);
        Ok(())
    }

    fn sync_data(&self) -> io::Result<()> {
        self.step("disk.sync.before")?;
        let mut disk = self.disk()?;
        disk.take_io_fault()?;
        if let Some(Fault::CommitSync { applied }) = disk.fault
            && disk.header_written
        {
            disk.fault = None;
            if applied {
                disk.durable = disk.current.clone();
            }
            return Err(injected());
        }
        disk.stats.syncs += 1;
        disk.durable = disk.current.clone();
        disk.header_written = false;
        drop(disk);
        self.step("disk.sync.after")
    }

    fn write(&self, offset: u64, data: &[u8]) -> io::Result<()> {
        self.step("disk.write")?;
        let mut disk = self.disk()?;
        disk.take_io_fault()?;
        let start = offset as usize;
        disk.current
            .get_mut(start..start + data.len())
            .ok_or_else(out_of_range)?
            .copy_from_slice(data);
        disk.stats.bytes_written += data.len() as u64;
        disk.header_written |= offset == 0;
        Ok(())
    }
}
