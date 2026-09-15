use super::*;
use crate::{
    client::core_lifecycle::ports::PreparedCoreBinary,
    core::download::{DownloadStatus, DownloaderState},
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Semaphore, mpsc};

#[derive(Debug)]
enum Event {
    FetchStarted,
    Preparing,
    PrepareDropped,
}

struct FakeBackend {
    events: mpsc::UnboundedSender<Event>,
    fetch_gate: Semaphore,
    prepare_gate: Semaphore,
    fetches: AtomicUsize,
    prepares: AtomicUsize,
}

struct PreparationGuard(mpsc::UnboundedSender<Event>);
impl Drop for PreparationGuard {
    fn drop(&mut self) {
        let _ = self.0.send(Event::PrepareDropped);
    }
}

#[async_trait]
impl UpdaterBackend for FakeBackend {
    async fn fetch_manifest(
        &self,
        _mirror: Option<(String, Instant)>,
    ) -> Result<(ManifestVersion, (String, Instant))> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        self.events.send(Event::FetchStarted).unwrap();
        self.fetch_gate.acquire().await.unwrap().forget();
        let mut manifest = ManifestVersion::default();
        manifest.latest.mihomo = "test-version".into();
        manifest
            .arch_template
            .mihomo
            .insert(get_arch().unwrap().into(), "mihomo-{}.gz".into());
        Ok((manifest, ("https://github.com".into(), Instant::now())))
    }

    async fn prepare(
        &self,
        _core: ClashCore,
        _mirror: String,
        artifact: String,
        _tag: CoreTypeMeta,
        progress: UpdaterProgress,
    ) -> Result<PreparedCoreBinary> {
        assert_eq!(artifact, "mihomo-test-version.gz");
        self.prepares.fetch_add(1, Ordering::SeqCst);
        let _guard = PreparationGuard(self.events.clone());
        progress.report(
            UpdaterState::Downloading,
            Some(DownloadStatus {
                state: DownloaderState::Downloading,
                downloaded: 42,
                total: 100,
                speed: 12.0,
            }),
        );
        self.events.send(Event::Preparing).unwrap();
        self.prepare_gate.acquire().await.unwrap().forget();
        anyhow::bail!("simulated download failure")
    }
}

struct UnusedInstaller;
#[async_trait]
impl CoreUpdateInstaller for UnusedInstaller {
    async fn install(&self, _artifact: PreparedCoreBinary) -> Result<()> {
        panic!("a failed or blocked preparation must never install")
    }
}

async fn setup() -> (
    UpdaterClient,
    Arc<FakeBackend>,
    mpsc::UnboundedReceiver<Event>,
) {
    let (events, receiver) = mpsc::unbounded_channel();
    let backend = Arc::new(FakeBackend {
        events,
        fetch_gate: Semaphore::new(0),
        prepare_gate: Semaphore::new(0),
        fetches: AtomicUsize::new(0),
        prepares: AtomicUsize::new(0),
    });
    let client = UpdaterClient::spawn(backend.clone(), Arc::new(UnusedInstaller))
        .await
        .unwrap();
    (client, backend, receiver)
}

async fn next_event(events: &mut mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap()
}

async fn settled_report(client: &UpdaterClient, id: usize) -> UpdaterSummary {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let summary = client.inspect(id).await.unwrap();
            if matches!(
                summary.state,
                UpdaterState::Done | UpdaterState::Failed(_) | UpdaterState::Pending(_)
            ) {
                return summary;
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn concurrent_fetches_coalesce_and_queries_remain_responsive() {
    let (client, backend, mut events) = setup().await;
    let (first, second, ()) = tokio::join!(client.fetch_latest(), client.fetch_latest(), async {
        assert!(matches!(next_event(&mut events).await, Event::FetchStarted));
        // The query acknowledges that both preceding requests were accepted
        // while their single backend fetch is still blocked.
        assert!(client.inspect(999).await.is_err());
        assert_eq!(backend.fetches.load(Ordering::SeqCst), 1);
        backend.fetch_gate.add_permits(1);
    });
    assert_eq!(first.unwrap().mihomo, "test-version");
    assert_eq!(second.unwrap().mihomo, "test-version");
    assert_eq!(backend.fetches.load(Ordering::SeqCst), 1);
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn duplicate_admission_reports_progress_failure_and_retention() {
    let (client, backend, mut events) = setup().await;
    backend.fetch_gate.add_permits(1);
    client.fetch_latest().await.unwrap();
    assert!(matches!(next_event(&mut events).await, Event::FetchStarted));
    let id = client.update(ClashCore::Mihomo).await.unwrap();
    assert!(matches!(next_event(&mut events).await, Event::Preparing));
    assert_eq!(client.update(ClashCore::Mihomo).await.unwrap(), id);
    assert_eq!(backend.prepares.load(Ordering::SeqCst), 1);
    let progress = client.inspect(id).await.unwrap();
    assert!(matches!(progress.state, UpdaterState::Downloading));
    assert_eq!(progress.downloader.downloaded, 42);
    assert_eq!(progress.downloader.total, 100);
    backend.prepare_gate.add_permits(1);
    let failed = settled_report(&client, id).await;
    assert!(
        matches!(failed.state, UpdaterState::Failed(reason) if reason.contains("simulated download failure"))
    );
    assert_eq!(failed.downloader.downloaded, 42);
    client
        .0
        .0
        .cast(Message::Prune(
            Instant::now() + RETENTION + Duration::from_secs(1),
        ))
        .unwrap();
    assert!(client.inspect(id).await.is_err());
    let new_id = client.update(ClashCore::Mihomo).await.unwrap();
    assert_ne!(id, new_id);
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_drops_blocked_worker_before_acknowledging() {
    let (client, backend, mut events) = setup().await;
    backend.fetch_gate.add_permits(1);
    client.fetch_latest().await.unwrap();
    assert!(matches!(next_event(&mut events).await, Event::FetchStarted));
    let id = client.update(ClashCore::Mihomo).await.unwrap();
    assert!(matches!(next_event(&mut events).await, Event::Preparing));
    client.shutdown().await.unwrap();
    assert!(matches!(events.try_recv().unwrap(), Event::PrepareDropped));
    assert!(
        matches!(client.inspect(id).await.unwrap().state, UpdaterState::Failed(reason) if reason.contains("shut down"))
    );
    assert!(client.update(ClashCore::Mihomo).await.is_err());
}

#[tokio::test]
async fn shutdown_completes_pending_fetch_with_error() {
    let (client, _backend, mut events) = setup().await;
    let (fetch, ()) = tokio::join!(client.fetch_latest(), async {
        assert!(matches!(next_event(&mut events).await, Event::FetchStarted));
        client.shutdown().await.unwrap();
    });
    assert!(fetch.unwrap_err().to_string().contains("shutting down"));
}

struct ReadyBackend {
    fetch_panics: AtomicUsize,
    prepare_panics: AtomicUsize,
}

impl ReadyBackend {
    fn new(fetch_panics: usize, prepare_panics: usize) -> Arc<Self> {
        Arc::new(Self {
            fetch_panics: AtomicUsize::new(fetch_panics),
            prepare_panics: AtomicUsize::new(prepare_panics),
        })
    }
}

fn consume_panic(counter: &AtomicUsize) -> bool {
    counter
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |count| {
            count.checked_sub(1)
        })
        .is_ok()
}

#[async_trait]
impl UpdaterBackend for ReadyBackend {
    async fn fetch_manifest(
        &self,
        _mirror: Option<(String, Instant)>,
    ) -> Result<(ManifestVersion, (String, Instant))> {
        assert!(
            !consume_panic(&self.fetch_panics),
            "simulated manifest panic"
        );
        let mut manifest = ManifestVersion::default();
        manifest.latest.mihomo = "test-version".into();
        manifest
            .arch_template
            .mihomo
            .insert(get_arch().unwrap().into(), "mihomo-{}.gz".into());
        Ok((manifest, ("https://github.com".into(), Instant::now())))
    }

    async fn prepare(
        &self,
        core: ClashCore,
        _mirror: String,
        _artifact: String,
        _tag: CoreTypeMeta,
        progress: UpdaterProgress,
    ) -> Result<PreparedCoreBinary> {
        assert!(
            !consume_panic(&self.prepare_panics),
            "simulated preparation panic"
        );
        let staging = Arc::new(tempfile::TempDir::new()?);
        let source = staging.path().join("prepared");
        std::fs::write(&source, b"core binary")?;
        progress.report(UpdaterState::Replacing, None);
        Ok(PreparedCoreBinary {
            target: core,
            source,
            destination: staging.path().join("installed"),
            staging,
            progress: Arc::new(progress),
        })
    }
}

struct RecordingInstaller {
    installed: mpsc::UnboundedSender<PreparedCoreBinary>,
    error: Option<&'static str>,
}

#[async_trait]
impl CoreUpdateInstaller for RecordingInstaller {
    async fn install(&self, artifact: PreparedCoreBinary) -> Result<()> {
        artifact.progress.restarting();
        self.installed
            .send(artifact)
            .map_err(|_| anyhow!("test installation receiver dropped"))?;
        match self.error {
            Some("installation request timed out") => Err(anyhow::Error::new(
                ports::InstallPending("installation request timed out".into()),
            )),
            Some(error) => Err(anyhow!(error)),
            None => Ok(()),
        }
    }
}

async fn installing_client(
    backend: Arc<ReadyBackend>,
    error: Option<&'static str>,
) -> (UpdaterClient, mpsc::UnboundedReceiver<PreparedCoreBinary>) {
    let (installed, receiver) = mpsc::unbounded_channel();
    let client = UpdaterClient::spawn(backend, Arc::new(RecordingInstaller { installed, error }))
        .await
        .unwrap();
    (client, receiver)
}

#[tokio::test]
async fn successful_install_finishes_and_keeps_staging_owned_by_installer() {
    let (client, mut installed) = installing_client(ReadyBackend::new(0, 0), None).await;
    client.fetch_latest().await.unwrap();
    let id = client.update(ClashCore::Mihomo).await.unwrap();
    assert!(matches!(
        settled_report(&client, id).await.state,
        UpdaterState::Done
    ));
    let prepared = installed.try_recv().unwrap();
    let staging_path = prepared.staging.path().to_path_buf();
    assert_eq!(std::fs::read(&prepared.source).unwrap(), b"core binary");
    drop(prepared);
    assert!(!staging_path.exists());
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn pending_install_reserves_admission_until_authoritative_completion() {
    let (client, mut installed) = installing_client(
        ReadyBackend::new(0, 0),
        Some("installation request timed out"),
    )
    .await;
    client.fetch_latest().await.unwrap();
    let id = client.update(ClashCore::Mihomo).await.unwrap();
    assert!(
        matches!(settled_report(&client, id).await.state, UpdaterState::Pending(reason) if reason.contains("timed out"))
    );
    let prepared = installed.try_recv().unwrap();
    client
        .0
        .0
        .cast(Message::Prune(
            Instant::now() + RETENTION + Duration::from_secs(1),
        ))
        .unwrap();
    assert_eq!(client.update(ClashCore::Mihomo).await.unwrap(), id);
    assert!(matches!(
        client.inspect(id).await.unwrap().state,
        UpdaterState::Pending(_)
    ));
    prepared.progress.restarting();
    client
        .0
        .0
        .cast(Message::Prune(
            Instant::now() + RETENTION + Duration::from_secs(1),
        ))
        .unwrap();
    assert!(matches!(
        client.inspect(id).await.unwrap().state,
        UpdaterState::Restarting
    ));
    assert_eq!(client.update(ClashCore::Mihomo).await.unwrap(), id);
    prepared.progress.finished(None);
    assert!(matches!(
        client.inspect(id).await.unwrap().state,
        UpdaterState::Done
    ));
    prepared.progress.finished(Some("late stale failure"));
    assert!(matches!(
        client.inspect(id).await.unwrap().state,
        UpdaterState::Done
    ));
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn panicking_fetch_and_prepare_release_admission_for_retry() {
    let (client, mut installed) = installing_client(ReadyBackend::new(1, 1), None).await;
    assert!(
        client
            .fetch_latest()
            .await
            .unwrap_err()
            .to_string()
            .contains("panicked")
    );
    client.fetch_latest().await.unwrap();
    let first = client.update(ClashCore::Mihomo).await.unwrap();
    assert!(
        matches!(settled_report(&client, first).await.state, UpdaterState::Failed(reason) if reason.contains("panicked"))
    );
    let second = client.update(ClashCore::Mihomo).await.unwrap();
    assert_ne!(first, second);
    assert!(matches!(
        settled_report(&client, second).await.state,
        UpdaterState::Done
    ));
    assert!(installed.try_recv().is_ok());
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn definitive_install_failure_allows_fresh_admission() {
    let (client, mut installed) =
        installing_client(ReadyBackend::new(0, 0), Some("permission denied")).await;
    client.fetch_latest().await.unwrap();
    let id = client.update(ClashCore::Mihomo).await.unwrap();
    assert!(
        matches!(settled_report(&client, id).await.state, UpdaterState::Failed(reason) if reason.contains("permission denied"))
    );
    let prepared = installed.try_recv().unwrap();
    prepared.progress.finished(Some("permission denied"));
    let retry = client.update(ClashCore::Mihomo).await.unwrap();
    assert_ne!(id, retry);
    client.shutdown().await.unwrap();
}
