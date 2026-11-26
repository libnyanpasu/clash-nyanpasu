//! The store keeps each candidate in its own anonymous file and finds it only
//! through this session's index, under concurrent saves, reads and shutdown.

use std::{
    fs::File,
    io,
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, Ordering},
    },
};

use nyanpasu_config::application::ClashCore;
use nyanpasu_runtime_values::{candidate::CandidateSnapshotId, runtime_error::RuntimeError};

use super::{CandidateEvidence, CandidateFiles, CandidateStore, TempCandidateFiles};
use crate::client::{runtime::RuntimeRevisionAllocator, runtime_inspection};

fn evidence() -> CandidateEvidence {
    CandidateEvidence {
        revision: RuntimeRevisionAllocator::new().allocate(),
        target_core: ClashCore::default(),
        inspection: Arc::new(runtime_inspection::tests::inspection_data()),
        halted: None,
    }
}

async fn revision_of(
    store: &Arc<CandidateStore>,
    id: &CandidateSnapshotId,
) -> Result<u64, RuntimeError> {
    store.inspect(id, |archive| Ok(archive.revision)).await
}

/// Blocks each file creation until the test lets it through.
struct GatedFiles {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}

impl CandidateFiles for GatedFiles {
    fn create(&self) -> io::Result<File> {
        self.entered.wait();
        self.release.wait();
        tempfile::tempfile()
    }
}

/// Fails the first creation only.
struct FailingOnce(AtomicBool);

impl CandidateFiles for FailingOnce {
    fn create(&self) -> io::Result<File> {
        if self.0.swap(false, Ordering::SeqCst) {
            return Err(io::Error::other("disk full"));
        }
        tempfile::tempfile()
    }
}

async fn wait(barrier: &Arc<Barrier>) {
    let barrier = barrier.clone();
    tokio::task::spawn_blocking(move || {
        barrier.wait();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn a_kept_candidate_decodes_and_its_evidence_is_released() {
    let store = CandidateStore::for_tests();
    let evidence = evidence();
    let inspection = Arc::downgrade(&evidence.inspection);
    let expected = evidence.inspection.graph.clone();
    let id = store.save(evidence).await.unwrap();
    assert!(inspection.upgrade().is_none(), "the store keeps no graph");
    let graph = store
        .inspect(&id, |archive| Ok(archive.graph.clone()))
        .await
        .unwrap();
    assert_eq!(graph, expected);
}

#[tokio::test]
async fn ids_from_elsewhere_or_evicted_read_as_changed() {
    let store = CandidateStore::for_tests();
    let foreign = CandidateSnapshotId::parse("a".repeat(21)).unwrap();
    assert!(matches!(
        revision_of(&store, &foreign).await,
        Err(RuntimeError::RuntimeSnapshotChanged)
    ));
    let first = store.save(evidence()).await.unwrap();
    let mut kept = vec![first.clone()];
    for _ in 0..8 {
        kept.push(store.save(evidence()).await.unwrap());
    }
    assert!(matches!(
        revision_of(&store, &first).await,
        Err(RuntimeError::RuntimeSnapshotChanged)
    ));
    assert_eq!(store.kept(), kept[1..]);
    for id in &kept[1..] {
        assert!(revision_of(&store, id).await.is_ok());
    }
}

#[tokio::test]
async fn concurrent_saves_both_stay_readable() {
    let entered = Arc::new(Barrier::new(3));
    let release = Arc::new(Barrier::new(3));
    let store = Arc::new(CandidateStore::new(Arc::new(GatedFiles {
        entered: entered.clone(),
        release: release.clone(),
    })));
    let first = tokio::spawn({
        let store = store.clone();
        async move { store.save(evidence()).await }
    });
    let second = tokio::spawn({
        let store = store.clone();
        async move { store.save(evidence()).await }
    });
    // Both writes are in progress at once.
    wait(&entered).await;
    wait(&release).await;
    let first = first.await.unwrap().unwrap();
    let second = second.await.unwrap().unwrap();
    assert_ne!(first, second);
    assert!(revision_of(&store, &first).await.is_ok());
    assert!(revision_of(&store, &second).await.is_ok());
}

#[tokio::test]
async fn a_read_survives_the_eviction_that_races_it() {
    let store = CandidateStore::for_tests();
    let id = store.save(evidence()).await.unwrap();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let read = tokio::spawn({
        let store = store.clone();
        let id = id.clone();
        let (entered, release) = (entered.clone(), release.clone());
        async move {
            store
                .inspect(&id, move |archive| {
                    entered.wait();
                    release.wait();
                    Ok(archive.revision)
                })
                .await
        }
    });
    wait(&entered).await;
    for _ in 0..8 {
        store.save(evidence()).await.unwrap();
    }
    assert!(!store.kept().contains(&id));
    wait(&release).await;
    assert!(read.await.unwrap().is_ok());
    assert!(matches!(
        revision_of(&store, &id).await,
        Err(RuntimeError::RuntimeSnapshotChanged)
    ));
}

#[tokio::test]
async fn a_save_that_finishes_after_close_keeps_nothing() {
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let store = Arc::new(CandidateStore::new(Arc::new(GatedFiles {
        entered: entered.clone(),
        release: release.clone(),
    })));
    let save = tokio::spawn({
        let store = store.clone();
        async move { store.save(evidence()).await }
    });
    wait(&entered).await;
    store.close();
    wait(&release).await;
    assert!(save.await.unwrap().is_none());
    assert!(store.kept().is_empty());
    // Closed: a later save does not even start.
    assert!(store.save(evidence()).await.is_none());
}

#[tokio::test]
async fn close_keeps_reads_until_release_drops_everything() {
    let store = CandidateStore::for_tests();
    let id = store.save(evidence()).await.unwrap();
    store.close();
    assert!(revision_of(&store, &id).await.is_ok());
    store.release();
    store.release();
    assert!(matches!(
        revision_of(&store, &id).await,
        Err(RuntimeError::RuntimeSnapshotChanged)
    ));
}

#[tokio::test]
async fn an_oversized_or_unwritable_candidate_keeps_nothing_and_the_store_recovers() {
    let oversized = Arc::new(
        CandidateStore::new(Arc::new(FailingOnce(AtomicBool::new(false))))
            .with_max_encoded_bytes(16),
    );
    assert!(oversized.save(evidence()).await.is_none());
    assert!(oversized.kept().is_empty());

    let store = Arc::new(CandidateStore::new(Arc::new(FailingOnce(AtomicBool::new(
        true,
    )))));
    assert!(store.save(evidence()).await.is_none());
    let id = store.save(evidence()).await.unwrap();
    assert!(revision_of(&store, &id).await.is_ok());
}

fn directory_entries(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("{error}"),
    }
}

#[tokio::test]
async fn kept_files_cannot_be_opened_by_name_and_vanish_on_release() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::utils::test_support::test_paths(dir.path(), dir.path());
    let directory = paths.cache_dir().join("candidate-inspection");
    let store = Arc::new(CandidateStore::new(Arc::new(TempCandidateFiles::new(
        &paths,
    ))));
    // A second instance sharing the data directory.
    let other = Arc::new(CandidateStore::new(Arc::new(TempCandidateFiles::new(
        &paths,
    ))));
    let id = store.save(evidence()).await.unwrap();
    let other_id = other.save(evidence()).await.unwrap();
    assert!(matches!(
        revision_of(&other, &id).await,
        Err(RuntimeError::RuntimeSnapshotChanged)
    ));
    assert!(revision_of(&store, &id).await.is_ok());
    assert!(revision_of(&other, &other_id).await.is_ok());

    for path in directory_entries(directory.as_std_path()) {
        assert!(
            File::open(&path).is_err(),
            "{} is open to other readers",
            path.display()
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert!(directory_entries(directory.as_std_path()).is_empty());
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    store.release();
    other.release();
    assert!(directory_entries(directory.as_std_path()).is_empty());
}

/// A file that keeps its name, as the Unix fallback leaves one whose unlink
/// failed.
#[cfg(unix)]
struct NamedFiles(std::path::PathBuf);

#[cfg(unix)]
impl CandidateFiles for NamedFiles {
    fn create(&self) -> io::Result<File> {
        File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&self.0)
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_file_that_still_has_a_name_receives_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("named");
    let store = Arc::new(CandidateStore::new(Arc::new(NamedFiles(path.clone()))));
    assert!(store.save(evidence()).await.is_none());
    assert!(store.kept().is_empty());
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
}
