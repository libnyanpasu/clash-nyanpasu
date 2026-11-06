//! Session-scoped rejected builds, persisted without retaining their graphs.

use std::{
    fs,
    io::Write,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use camino::Utf8PathBuf;
use nyanpasu_config::{
    profile::ProfileId,
    runtime::snapshot::{
        OperatorTag, StoredConfigSnapshotsGraph,
        persistence::{self, RejectedBuildArchive},
    },
};
use nyanpasu_paths::PathResolver;

use super::{
    application_workflow::ports::RejectedBuildStore,
    runtime::{self, RuntimeSnapshot},
    runtime_error::RuntimeError,
};
use crate::corrupt;

const ID_LENGTH: usize = 21;

#[derive(Debug)]
pub(in crate::client) struct FileRejectedBuildStore {
    directory: Utf8PathBuf,
    current: Arc<Mutex<Option<RejectedBuildMetadata>>>,
}

#[derive(Debug)]
struct RejectedBuildMetadata {
    id: String,
    source_profile_id: Option<ProfileId>,
}

impl FileRejectedBuildStore {
    pub(in crate::client) fn new(paths: &PathResolver) -> Self {
        Self {
            directory: paths.cache_dir().join("rejected-config"),
            current: Arc::new(Mutex::new(None)),
        }
    }

    fn path(&self, id: &str) -> Result<Utf8PathBuf, RuntimeError> {
        if id.len() != ID_LENGTH || !id.chars().all(|ch| nanoid::alphabet::SAFE.contains(&ch)) {
            return Err(RuntimeError::RuntimeSnapshotChanged);
        }
        Ok(self.directory.join(format!("rejected-{id}.bin")))
    }

    fn sweep(&self) -> std::io::Result<()> {
        *self
            .current
            .lock()
            .expect("rejected build metadata updates do not panic") = None;
        let metadata = match corrupt::retry_io(|| fs::symlink_metadata(&self.directory)) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if runtime::is_symlink_or_reparse(&metadata) || !metadata.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid rejected build directory",
            ));
        }
        let mut failure = None;
        let mut entries = corrupt::retry_io(|| fs::read_dir(&self.directory))?;
        while let Some(entry) = corrupt::retry_io(|| entries.next().transpose())? {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let metadata = corrupt::retry_io(|| fs::symlink_metadata(entry.path()))?;
            if ((name.starts_with("rejected-") && metadata.is_file())
                || name.starts_with(".atomicwrite"))
                && let Err(error) = remove_entry(&entry.path())
            {
                tracing::warn!(path = %entry.path().display(), %error, "could not remove rejected build entry");
                failure = Some(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    fn write(&self, snapshot: &RuntimeSnapshot) -> anyhow::Result<String> {
        self.sweep()?;
        let id = snapshot.inspection_id.clone();
        let path = self.path(&id)?;
        let archive = RejectedBuildArchive {
            format_version: persistence::REJECTED_BUILD_ARCHIVE_VERSION,
            id: id.clone(),
            revision: snapshot.revision.get(),
            target_core: snapshot.target_core,
            graph: snapshot.inspection.graph.clone(),
            final_node: snapshot.inspection.final_node,
            step_logs: snapshot.inspection.step_logs.clone(),
        };
        let bytes = persistence::encode_rejected_archive(&archive)?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        atomicwrites::AtomicFile::new(&path, atomicwrites::OverwriteBehavior::DisallowOverwrite)
            .write_with_options(|file| file.write_all(&bytes), options)?;
        *self
            .current
            .lock()
            .expect("rejected build metadata updates do not panic") = Some(RejectedBuildMetadata {
            id: id.clone(),
            source_profile_id: source_profile(&snapshot.inspection.graph).cloned(),
        });
        Ok(id)
    }
}

fn remove_entry(path: &std::path::Path) -> std::io::Result<()> {
    let metadata = corrupt::retry_io(|| fs::symlink_metadata(path))?;
    if runtime::is_symlink_or_reparse(&metadata) {
        return Ok(());
    }
    if metadata.is_dir() {
        let mut entries = corrupt::retry_io(|| fs::read_dir(path))?;
        while let Some(entry) = corrupt::retry_io(|| entries.next().transpose())? {
            remove_entry(&entry.path())?;
        }
        corrupt::retry_io(|| fs::remove_dir(path))
    } else {
        corrupt::retry_io(|| fs::remove_file(path))
    }
}

fn source_profile(graph: &StoredConfigSnapshotsGraph) -> Option<&ProfileId> {
    match &graph.nodes.get(graph.root_id as usize)?.tag {
        OperatorTag::FileConfigRoot { profile_id, .. }
        | OperatorTag::CompositionRoot { profile_id, .. } => Some(profile_id),
        _ => None,
    }
}

#[async_trait]
impl RejectedBuildStore for FileRejectedBuildStore {
    async fn save(&self, snapshot: Arc<RuntimeSnapshot>) -> Option<String> {
        if let Err(error) = runtime::prepare_private_dir(&self.directory).await {
            tracing::warn!(%error, "could not prepare rejected build directory");
            self.clear().await;
            return None;
        }
        let store = Self {
            directory: self.directory.clone(),
            current: self.current.clone(),
        };
        match tokio::task::spawn_blocking(move || store.write(&snapshot)).await {
            Ok(Ok(id)) => Some(id),
            result => {
                tracing::warn!(?result, "could not persist rejected build");
                self.clear().await;
                None
            }
        }
    }

    async fn clear_source(&self, snapshot: Arc<RuntimeSnapshot>) {
        let Some(profile) = source_profile(&snapshot.inspection.graph) else {
            return;
        };
        let id = {
            let current = self
                .current
                .lock()
                .expect("rejected build metadata updates do not panic");
            let Some(current) = current.as_ref() else {
                return;
            };
            if current.source_profile_id.as_ref() != Some(profile) {
                return;
            }
            current.id.clone()
        };
        let store = Self {
            directory: self.directory.clone(),
            current: self.current.clone(),
        };
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let metadata = match corrupt::retry_io(|| fs::symlink_metadata(&store.directory)) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            if runtime::is_symlink_or_reparse(&metadata) || !metadata.is_dir() {
                return Ok(());
            }
            match remove_entry(store.path(&id)?.as_std_path()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let mut current = store
                .current
                .lock()
                .expect("rejected build metadata updates do not panic");
            if current.as_ref().is_some_and(|current| current.id == id) {
                *current = None;
            }
            Ok(())
        })
        .await;
        if !matches!(result, Ok(Ok(()))) {
            tracing::warn!(?result, "could not clear resolved rejected build");
        }
    }

    async fn clear(&self) {
        let store = Self {
            directory: self.directory.clone(),
            current: self.current.clone(),
        };
        let result = tokio::task::spawn_blocking(move || store.sweep()).await;
        if !matches!(result, Ok(Ok(()))) {
            tracing::warn!(?result, "could not clear rejected build files");
        }
    }

    fn load(&self, id: &str) -> Result<RejectedBuildArchive, RuntimeError> {
        let path = self.path(id)?;
        let load = || -> anyhow::Result<RejectedBuildArchive> {
            let directory = corrupt::retry_io(|| fs::symlink_metadata(&self.directory))?;
            let metadata = corrupt::retry_io(|| fs::symlink_metadata(&path))?;
            anyhow::ensure!(
                !runtime::is_symlink_or_reparse(&directory)
                    && directory.is_dir()
                    && !runtime::is_symlink_or_reparse(&metadata)
                    && metadata.is_file(),
                "invalid rejected build file"
            );
            let bytes = corrupt::retry_io(|| fs::read(&path))?;
            let archive = persistence::decode_rejected_archive(&bytes)?;
            anyhow::ensure!(archive.id == id, "rejected build identity changed");
            Ok(archive)
        };
        load().map_err(|error| {
            tracing::debug!(%error, "rejected build is unavailable");
            RuntimeError::RuntimeSnapshotChanged
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{
        runtime::{RuntimeRevisionAllocator, RuntimeSnapshotData},
        runtime_inspection,
    };
    use nyanpasu_config::{application::ClashCore, runtime::snapshot::ConfigExecutionRole};

    fn snapshot(profile: &str) -> Arc<RuntimeSnapshot> {
        snapshot_with_root(OperatorTag::FileConfigRoot {
            profile_id: ProfileId(profile.to_owned()),
            role: ConfigExecutionRole::Selected,
        })
    }

    fn snapshot_with_root(root: OperatorTag) -> Arc<RuntimeSnapshot> {
        let mut data = runtime_inspection::tests::inspection_data();
        data.graph.nodes[0].tag = root;
        Arc::new(RuntimeSnapshot::from_data(
            RuntimeRevisionAllocator::new().allocate(),
            ClashCore::default(),
            RuntimeSnapshotData {
                config_text: Arc::from("mode: rule\n"),
                exists_keys: Vec::new(),
                postprocessing_output: Default::default(),
                inspection: Arc::new(data),
            },
        ))
    }

    fn store(dir: &tempfile::TempDir) -> FileRejectedBuildStore {
        FileRejectedBuildStore::new(&crate::utils::test_support::test_paths(
            dir.path(),
            dir.path(),
        ))
    }

    #[tokio::test]
    async fn rejected_files_replace_release_and_clear_only_the_same_source() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let first = snapshot("one");
        let released = Arc::downgrade(&first);
        let first_id = store.save(first).await.unwrap();
        assert!(released.upgrade().is_none());
        let second_id = store.save(snapshot("two")).await.unwrap();
        assert!(matches!(
            store.load(&first_id),
            Err(RuntimeError::RuntimeSnapshotChanged)
        ));
        store.clear_source(snapshot("one")).await;
        assert!(store.load(&second_id).is_ok());
        store.clear_source(snapshot("two")).await;
        assert!(matches!(
            store.load(&second_id),
            Err(RuntimeError::RuntimeSnapshotChanged)
        ));
        assert_eq!(fs::read_dir(&store.directory).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn rejected_source_cleanup_removes_corrupt_files_without_decoding() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let id = store.save(snapshot("one")).await.unwrap();
        let path = store.path(&id).unwrap();
        fs::write(&path, b"corrupt archive").unwrap();

        store.clear_source(snapshot("two")).await;
        assert!(path.is_file());
        store
            .clear_source(snapshot_with_root(OperatorTag::BareRoot))
            .await;
        assert!(path.is_file());
        store.clear_source(snapshot("one")).await;
        assert!(!path.exists());
        assert!(store.current.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn rejected_bare_build_has_no_source_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let id = store
            .save(snapshot_with_root(OperatorTag::BareRoot))
            .await
            .unwrap();
        let path = store.path(&id).unwrap();
        store.clear_source(snapshot("one")).await;
        store
            .clear_source(snapshot_with_root(OperatorTag::BareRoot))
            .await;
        assert!(path.is_file());
    }

    #[tokio::test]
    async fn rejected_write_failure_removes_the_previous_file_and_atomic_temporaries() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let old = store.save(snapshot("old")).await.unwrap();
        let next = snapshot("next");
        let obstacle = store.path(&next.inspection_id).unwrap();
        fs::create_dir(&obstacle).unwrap();
        assert!(store.save(next).await.is_none());
        assert!(store.current.lock().unwrap().is_none());
        assert!(matches!(
            store.load(&old),
            Err(RuntimeError::RuntimeSnapshotChanged)
        ));
        assert_eq!(fs::read_dir(&store.directory).unwrap().count(), 1);
        assert!(obstacle.is_dir());
    }

    #[tokio::test]
    async fn rejected_sweep_removes_session_files_and_preserves_unrelated_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let id = store.save(snapshot("one")).await.unwrap();
        fs::write(store.directory.join("rejected-old.bin"), "old").unwrap();
        fs::write(store.directory.join("unrelated"), "keep").unwrap();
        fs::create_dir(store.directory.join(".atomicwrite-old")).unwrap();
        fs::write(
            store.directory.join(".atomicwrite-old/tmpfile.tmp"),
            "partial",
        )
        .unwrap();
        store.clear().await;
        assert!(store.current.lock().unwrap().is_none());
        assert!(matches!(
            store.load(&id),
            Err(RuntimeError::RuntimeSnapshotChanged)
        ));
        assert_eq!(fs::read_dir(&store.directory).unwrap().count(), 1);
        assert_eq!(
            fs::read(store.directory.join("unrelated")).unwrap(),
            b"keep"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejected_sweep_and_reads_skip_symlinks_and_private_files_have_private_modes() {
        use std::os::unix::fs::{self as unix_fs, PermissionsExt as _};
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        let id = store.save(snapshot("one")).await.unwrap();
        assert_eq!(
            fs::metadata(store.path(&id).unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&store.directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let external = dir.path().join("external");
        fs::write(&external, "secret").unwrap();
        let link = store
            .directory
            .join(format!("rejected-{}.bin", "a".repeat(ID_LENGTH)));
        unix_fs::symlink(&external, &link).unwrap();
        store.clear().await;
        assert!(fs::symlink_metadata(link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&external).unwrap(), b"secret");
        assert!(matches!(
            store.load(&"a".repeat(ID_LENGTH)),
            Err(RuntimeError::RuntimeSnapshotChanged)
        ));
    }

    #[test]
    fn rejected_invalid_ids_are_rejected_before_any_filesystem_access() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        for id in [
            "",
            "../escape",
            "/tmp/escape",
            "aaaaaaaaaaaaaaaaaaaa/",
            "aaaaaaaaaaaaaaaaaaaa.",
        ] {
            assert!(store.path(id).is_err(), "invalid id {id:?}");
            assert!(matches!(
                store.load(id),
                Err(RuntimeError::RuntimeSnapshotChanged)
            ));
        }
        assert!(!store.directory.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
