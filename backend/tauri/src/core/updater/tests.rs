use super::*;
use crate::{
    client::core_lifecycle::ports::PreparedCoreBinary,
    core::download::{DownloadStatus, DownloaderState},
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Semaphore, mpsc};

/// The updater's share of the root shutdown.
struct Shutdown {
    token: CancellationToken,
    tasks: TaskTracker,
}

impl Shutdown {
    fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            tasks: TaskTracker::new(),
        }
    }

    fn request(&self) {
        self.token.cancel();
        self.tasks.close();
    }

    async fn run(&self) {
        self.request();
        tokio::time::timeout(Duration::from_secs(5), self.tasks.wait())
            .await
            .expect("the updater stops");
    }
}

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
        shutdown: &CancellationToken,
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
        // The gate stands for the download, which ends with the shutdown.
        tokio::select! {
            permit = self.prepare_gate.acquire() => permit.unwrap().forget(),
            () = shutdown.cancelled() => anyhow::bail!("updater shut down"),
        }
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
    Shutdown,
) {
    let (events, receiver) = mpsc::unbounded_channel();
    let backend = Arc::new(FakeBackend {
        events,
        fetch_gate: Semaphore::new(0),
        prepare_gate: Semaphore::new(0),
        fetches: AtomicUsize::new(0),
        prepares: AtomicUsize::new(0),
    });
    let shutdown = Shutdown::new();
    let client = UpdaterClient::spawn(
        backend.clone(),
        Arc::new(UnusedInstaller),
        crate::client::jobs::test_client().await,
        shutdown.token.clone(),
        &shutdown.tasks,
    )
    .await
    .unwrap();
    (client, backend, receiver, shutdown)
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
            if matches!(summary.state, UpdaterState::Done | UpdaterState::Failed(_)) {
                return summary;
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn concurrent_fetches_coalesce_and_queries_remain_responsive() {
    let (client, backend, mut events, shutdown) = setup().await;
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
    shutdown.run().await;
}

#[tokio::test]
async fn duplicate_admission_reports_progress_failure_and_retention() {
    let (client, backend, mut events, shutdown) = setup().await;
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
    shutdown.run().await;
}

#[tokio::test]
async fn the_shutdown_ends_a_download_and_refuses_new_work() {
    let (client, backend, mut events, shutdown) = setup().await;
    backend.fetch_gate.add_permits(1);
    client.fetch_latest().await.unwrap();
    assert!(matches!(next_event(&mut events).await, Event::FetchStarted));
    let id = client.update(ClashCore::Mihomo).await.unwrap();
    assert!(matches!(next_event(&mut events).await, Event::Preparing));

    shutdown.request();
    // Sent before this test yields, so it is queued ahead of the drain.
    let refused = client.update(ClashCore::ClashRs).await;
    shutdown.run().await;

    assert!(refused.unwrap_err().to_string().contains("shutting down"));
    assert!(matches!(events.try_recv().unwrap(), Event::PrepareDropped));
    assert!(client.inspect(id).await.is_err(), "the updater has stopped");
}

#[tokio::test]
async fn the_shutdown_completes_a_pending_fetch_with_an_error() {
    let (client, _backend, mut events, shutdown) = setup().await;
    let (fetch, ()) = tokio::join!(client.fetch_latest(), async {
        assert!(matches!(next_event(&mut events).await, Event::FetchStarted));
        shutdown.run().await;
    });
    assert!(fetch.unwrap_err().to_string().contains("shutting down"));
}

/// An installer that holds the install until the test releases it.
struct HeldInstaller {
    installing: tokio::sync::Notify,
    release: tokio::sync::Notify,
    installed: AtomicUsize,
}

#[async_trait]
impl CoreUpdateInstaller for HeldInstaller {
    async fn install(&self, _artifact: PreparedCoreBinary) -> Result<()> {
        self.installing.notify_one();
        self.release.notified().await;
        self.installed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// An install that started is not cut short: the shutdown waits for it.
#[tokio::test]
async fn the_shutdown_waits_for_an_install_in_progress() {
    let installer = Arc::new(HeldInstaller {
        installing: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
        installed: AtomicUsize::new(0),
    });
    let shutdown = Shutdown::new();
    let client = UpdaterClient::spawn(
        Arc::new(ReadyBackend),
        installer.clone(),
        crate::client::jobs::test_client().await,
        shutdown.token.clone(),
        &shutdown.tasks,
    )
    .await
    .unwrap();
    client.fetch_latest().await.unwrap();
    client.update(ClashCore::Mihomo).await.unwrap();
    installer.installing.notified().await;

    shutdown.request();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), shutdown.tasks.wait())
            .await
            .is_err(),
        "the install in progress is awaited"
    );

    installer.release.notify_one();
    shutdown.run().await;
    assert_eq!(installer.installed.load(Ordering::SeqCst), 1);
}

/// A backend whose download is over and whose extraction, which the token
/// cannot cut short, holds until the test releases it.
struct ExtractingBackend {
    extracting: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait]
impl UpdaterBackend for ExtractingBackend {
    async fn fetch_manifest(
        &self,
        mirror: Option<(String, Instant)>,
    ) -> Result<(ManifestVersion, (String, Instant))> {
        ReadyBackend.fetch_manifest(mirror).await
    }

    async fn prepare(
        &self,
        _core: ClashCore,
        _mirror: String,
        _artifact: String,
        _tag: CoreTypeMeta,
        _progress: UpdaterProgress,
        _shutdown: &CancellationToken,
    ) -> Result<PreparedCoreBinary> {
        self.extracting.notify_one();
        self.release.notified().await;
        anyhow::bail!("simulated extraction failure")
    }
}

/// An extraction that started is not cut short: the shutdown waits for it.
#[tokio::test]
async fn the_shutdown_waits_for_an_extraction_in_progress() {
    let backend = Arc::new(ExtractingBackend {
        extracting: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let shutdown = Shutdown::new();
    let client = UpdaterClient::spawn(
        backend.clone(),
        Arc::new(UnusedInstaller),
        crate::client::jobs::test_client().await,
        shutdown.token.clone(),
        &shutdown.tasks,
    )
    .await
    .unwrap();
    client.fetch_latest().await.unwrap();
    client.update(ClashCore::Mihomo).await.unwrap();
    backend.extracting.notified().await;

    shutdown.request();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), shutdown.tasks.wait())
            .await
            .is_err(),
        "the extraction in progress is awaited"
    );

    backend.release.notify_one();
    shutdown.run().await;
}

pub(crate) struct ReadyBackend;

#[async_trait]
impl UpdaterBackend for ReadyBackend {
    async fn fetch_manifest(
        &self,
        _mirror: Option<(String, Instant)>,
    ) -> Result<(ManifestVersion, (String, Instant))> {
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
        _shutdown: &CancellationToken,
    ) -> Result<PreparedCoreBinary> {
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
            Some(error) => Err(anyhow!(error)),
            None => Ok(()),
        }
    }
}

async fn installing_client(
    error: Option<&'static str>,
) -> (UpdaterClient, mpsc::UnboundedReceiver<PreparedCoreBinary>) {
    let (installed, receiver) = mpsc::unbounded_channel();
    let client = UpdaterClient::spawn(
        Arc::new(ReadyBackend),
        Arc::new(RecordingInstaller { installed, error }),
        crate::client::jobs::test_client().await,
        CancellationToken::new(),
        &TaskTracker::new(),
    )
    .await
    .unwrap();
    (client, receiver)
}

#[tokio::test]
async fn successful_install_finishes_and_keeps_staging_owned_by_installer() {
    let (client, mut installed) = installing_client(None).await;
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
}

#[tokio::test]
async fn definitive_install_failure_allows_fresh_admission() {
    let (client, mut installed) = installing_client(Some("permission denied")).await;
    client.fetch_latest().await.unwrap();
    let id = client.update(ClashCore::Mihomo).await.unwrap();
    assert!(
        matches!(settled_report(&client, id).await.state, UpdaterState::Failed(reason) if reason.contains("permission denied"))
    );
    let prepared = installed.try_recv().unwrap();
    prepared.progress.finished(Some("permission denied"));
    let retry = client.update(ClashCore::Mihomo).await.unwrap();
    assert_ne!(id, retry);
}
