//! The session index of kept candidates and the anonymous files that hold them.

use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read as _, Seek as _, SeekFrom},
    sync::Arc,
};

use camino::Utf8PathBuf;
use nyanpasu_config::runtime::snapshot::persistence::{
    self, CANDIDATE_ARCHIVE_VERSION, CandidateArchive, CandidateArchiveRef, SnapshotPersistError,
};
use nyanpasu_runtime_values::{candidate::CandidateSnapshotId, runtime_error::RuntimeError};
use parking_lot::Mutex;

use super::CandidateEvidence;
use crate::client::runtime;

/// How many candidates one session keeps at most.
const CAPACITY: usize = 8;
/// The largest compressed archive a save writes; a larger build keeps no copy.
const MAX_ENCODED_BYTES: u64 = 16 * 1024 * 1024;

/// Creates the file one candidate is written to. Called on a blocking thread.
pub(in crate::client) trait CandidateFiles: Send + Sync + 'static {
    /// A new file that no other process can open by name.
    fn create(&self) -> io::Result<File>;
}

/// Anonymous temporary files in a private cache directory. Windows opens
/// them exclusively and deletes them on close; Linux creates them without a
/// name; other Unix systems unlink them before they are returned, so nothing
/// is written while they have a name. The system reclaims them when the
/// process ends, however it ends.
pub(in crate::client) struct TempCandidateFiles {
    directory: Utf8PathBuf,
}

impl TempCandidateFiles {
    pub(in crate::client) fn new(paths: &nyanpasu_paths::PathResolver) -> Self {
        Self {
            directory: paths.cache_dir().join("candidate-inspection"),
        }
    }
}

impl CandidateFiles for TempCandidateFiles {
    fn create(&self) -> io::Result<File> {
        nyanpasu_persistence::retry_io(|| fs::create_dir_all(&self.directory))?;
        let metadata = nyanpasu_persistence::retry_io(|| fs::symlink_metadata(&self.directory))?;
        if runtime::is_symlink_or_reparse(&metadata) || !metadata.is_dir() {
            return Err(io::Error::other(
                "the candidate inspection directory is not a directory",
            ));
        }
        // The fallback that unlinks after creating briefly names the file.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))?;
        }
        tempfile::tempfile_in(&self.directory)
    }
}

struct Entry {
    id: CandidateSnapshotId,
    /// Reads seek from the start, so one read at a time.
    file: Mutex<File>,
}

#[derive(Default)]
struct Index {
    closed: bool,
    /// Oldest first.
    recent: VecDeque<Arc<Entry>>,
}

/// The candidates this session kept, each in its own anonymous file and
/// found only through this index: an ID from another session or instance,
/// or one already evicted, reads as a changed snapshot.
///
/// It is shared by the check, the queries and shutdown, which reach it from
/// different owners, so a lock serializes the index. The lock guards
/// publishing and eviction only: file IO and dropping evicted files happen
/// outside it.
pub(in crate::client) struct CandidateStore {
    files: Arc<dyn CandidateFiles>,
    max_encoded_bytes: u64,
    index: Mutex<Index>,
}

impl CandidateStore {
    pub(in crate::client) fn new(files: Arc<dyn CandidateFiles>) -> Self {
        Self {
            files,
            max_encoded_bytes: MAX_ENCODED_BYTES,
            index: Mutex::default(),
        }
    }

    /// Keeps `evidence` for inspection and returns its ID. A build too large
    /// to keep, a write that fails or a store that is closing keeps nothing,
    /// and the failure is reported without an ID.
    pub(in crate::client) async fn save(
        self: &Arc<Self>,
        evidence: CandidateEvidence,
    ) -> Option<CandidateSnapshotId> {
        if self.index.lock().closed {
            return None;
        }
        let store = self.clone();
        let written = nyanpasu_owner_support::blocking::join(
            tokio::task::spawn_blocking(move || store.write(&evidence)).await,
        );
        let entry = match written {
            Ok(entry) => Arc::new(entry),
            Err(error) => {
                tracing::warn!(%error, "could not keep the candidate for inspection");
                return None;
            }
        };
        let id = entry.id.clone();
        let evicted = {
            let mut index = self.index.lock();
            // A save that finishes after shutdown began keeps nothing.
            if index.closed {
                return None;
            }
            index.recent.push_back(entry);
            if index.recent.len() > CAPACITY {
                index.recent.pop_front()
            } else {
                None
            }
        };
        drop(evicted);
        Some(id)
    }

    fn write(&self, evidence: &CandidateEvidence) -> Result<Entry, SnapshotPersistError> {
        let mut file = self.files.create()?;
        ensure_anonymous(&file)?;
        let inspection = &evidence.inspection;
        persistence::write_candidate_archive(
            &CandidateArchiveRef {
                format_version: CANDIDATE_ARCHIVE_VERSION,
                revision: evidence.revision.get(),
                target_core: &evidence.target_core,
                graph: &inspection.graph,
                final_node: inspection.final_node,
                step_logs: &inspection.step_logs,
                halted: evidence.halted.as_ref(),
            },
            BufWriter::new(&mut file),
            self.max_encoded_bytes,
        )?;
        let id = CandidateSnapshotId::parse(nanoid::nanoid!(21, &nanoid::alphabet::SAFE))
            .expect("a safe-alphabet nanoid of 21 characters is a candidate snapshot id");
        Ok(Entry {
            id,
            file: Mutex::new(file),
        })
    }

    /// Decodes the candidate `id` names and projects it on a blocking thread;
    /// the decoded build is dropped with the projection's input.
    ///
    /// # Errors
    /// [`RuntimeError::RuntimeSnapshotChanged`] when this session holds no
    /// such candidate or its file no longer decodes; otherwise what `project`
    /// returns.
    pub(in crate::client) async fn inspect<T: Send + 'static>(
        self: &Arc<Self>,
        id: &CandidateSnapshotId,
        project: impl FnOnce(&CandidateArchive) -> Result<T, RuntimeError> + Send + 'static,
    ) -> Result<T, RuntimeError> {
        let entry = self
            .index
            .lock()
            .recent
            .iter()
            .find(|entry| entry.id == *id)
            .cloned()
            .ok_or(RuntimeError::RuntimeSnapshotChanged)?;
        nyanpasu_owner_support::blocking::join(
            tokio::task::spawn_blocking(move || {
                let archive = read(&entry, MAX_ENCODED_BYTES).map_err(|error| {
                    tracing::debug!(%error, "the kept candidate does not decode");
                    RuntimeError::RuntimeSnapshotChanged
                })?;
                project(&archive)
            })
            .await,
        )
    }

    /// Stops keeping candidates. Saves already writing finish without being
    /// published, and what was kept stays readable until [`Self::release`].
    /// Repeating it changes nothing.
    pub(in crate::client) fn close(&self) {
        self.index.lock().closed = true;
    }

    /// Closes the store and drops every kept candidate, deleting its file.
    /// Repeating it changes nothing.
    pub(in crate::client) fn release(&self) {
        let released = {
            let mut index = self.index.lock();
            index.closed = true;
            std::mem::take(&mut index.recent)
        };
        drop(released);
    }
}

/// A candidate holds configuration that never took effect, so it is written
/// only to a file that has no name. The Unix fallback that unlinks after
/// creating ignores a failed unlink; such a file is refused before anything
/// is written to it.
#[cfg(unix)]
fn ensure_anonymous(file: &File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    if file.metadata()?.nlink() == 0 {
        Ok(())
    } else {
        Err(io::Error::other("the candidate file still has a name"))
    }
}

/// Windows opens the file exclusively and deletes it on close.
#[cfg(not(unix))]
fn ensure_anonymous(_file: &File) -> io::Result<()> {
    Ok(())
}

fn read(entry: &Entry, limit: u64) -> Result<CandidateArchive, SnapshotPersistError> {
    let mut file = entry.file.lock();
    file.seek(SeekFrom::Start(0))?;
    // The file is this session's own and its size was capped when written.
    persistence::read_candidate_archive(BufReader::new(&mut *file).take(limit))
}

/// Anonymous files in the system's temporary directory, for tests that do
/// not look at where candidates are written.
#[cfg(test)]
struct SystemTempFiles;

#[cfg(test)]
impl CandidateFiles for SystemTempFiles {
    fn create(&self) -> io::Result<File> {
        tempfile::tempfile()
    }
}

#[cfg(test)]
impl CandidateStore {
    pub(in crate::client) fn for_tests() -> Arc<Self> {
        Arc::new(Self::new(Arc::new(SystemTempFiles)))
    }

    pub(in crate::client) fn with_max_encoded_bytes(mut self, limit: u64) -> Self {
        self.max_encoded_bytes = limit;
        self
    }

    pub(in crate::client) fn kept(&self) -> Vec<CandidateSnapshotId> {
        self.index
            .lock()
            .recent
            .iter()
            .map(|entry| entry.id.clone())
            .collect()
    }
}
