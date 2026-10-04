use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use anyhow::Result;
use async_trait::async_trait;
use nyanpasu_config::application::{ReleaseChannel, UpdateSource};
use tokio::sync::{Notify, watch};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::*;

struct Events(watch::Sender<Option<AppUpdateSnapshot>>);
impl AppUpdateEventSink for Events {
    fn publish(&self, snapshot: AppUpdateSnapshot) {
        self.0.send_replace(Some(snapshot));
    }
}

struct FakeBackend {
    hold_download: bool,
    fallback_progress: bool,
    hold_install: bool,
    fallback_source: Notify,
    continue_fallback: Notify,
    install_started: Notify,
    continue_install: Notify,
    install_finished: Notify,
    download_count: AtomicUsize,
    install_count: AtomicUsize,
    download_started: Notify,
    cancellation_seen: Notify,
    allow_cleanup: Notify,
    fail_next_check: AtomicBool,
}

#[async_trait]
impl AppUpdateBackend for FakeBackend {
    async fn check(
        &self,
        _settings: &AppUpdateSettings,
        cancellation: CancellationToken,
    ) -> Result<Option<PreparedAppUpdate>> {
        if self.fail_next_check.swap(false, Ordering::SeqCst) {
            anyhow::bail!("temporary check network failure")
        }
        if cancellation.is_cancelled() {
            anyhow::bail!("check cancelled")
        }
        Ok(Some(PreparedAppUpdate {
            release: AppUpdateRelease {
                version: "2.1.0".into(),
                date: None,
                body: None,
            },
            identity: "stable|x86_64|2.1.0|signature".into(),
            context: Arc::new(()),
        }))
    }

    async fn download(
        &self,
        _update: PreparedAppUpdate,
        cancellation: CancellationToken,
        progress: Arc<dyn Fn(AppUpdateDownloadProgress) + Send + Sync>,
    ) -> Result<VerifiedAppUpdate> {
        self.download_count.fetch_add(1, Ordering::SeqCst);
        progress(AppUpdateDownloadProgress::Source {
            source: UpdateSource::Github,
            content_length: None,
        });
        if self.hold_download {
            self.download_started.notify_one();
            cancellation.cancelled().await;
            self.cancellation_seen.notify_one();
            self.allow_cleanup.notified().await;
            anyhow::bail!("cancelled after cleanup")
        }
        progress(AppUpdateDownloadProgress::Verifying);
        if self.fallback_progress {
            progress(AppUpdateDownloadProgress::Source {
                source: UpdateSource::Nyanpasu,
                content_length: Some(3),
            });
            self.fallback_source.notify_one();
            self.continue_fallback.notified().await;
            progress(AppUpdateDownloadProgress::Chunk {
                downloaded: 2,
                total: Some(3),
            });
        }
        progress(AppUpdateDownloadProgress::Verifying);
        Ok(VerifiedAppUpdate {
            bytes: Arc::new(vec![1, 2, 3]),
            source: UpdateSource::Github,
        })
    }

    async fn install(&self, _update: PreparedAppUpdate, _package: VerifiedAppUpdate) -> Result<()> {
        self.install_count.fetch_add(1, Ordering::SeqCst);
        if self.hold_install {
            self.install_started.notify_one();
            self.continue_install.notified().await;
            self.install_finished.notify_one();
        }
        anyhow::bail!("not used by this test")
    }
}

async fn wait_for_phase(
    client: &AppUpdateClient,
    events: &mut watch::Receiver<Option<AppUpdateSnapshot>>,
    phase: AppUpdatePhase,
) -> AppUpdateSnapshot {
    loop {
        let snapshot = client.state().await.unwrap();
        if snapshot.phase == phase {
            return snapshot;
        }
        events.changed().await.unwrap();
    }
}

async fn wait_for_source(
    client: &AppUpdateClient,
    events: &mut watch::Receiver<Option<AppUpdateSnapshot>>,
    source: UpdateSource,
) -> AppUpdateSnapshot {
    loop {
        let snapshot = client.state().await.unwrap();
        if snapshot.source == Some(source) {
            return snapshot;
        }
        events.changed().await.unwrap();
    }
}

async fn test_client(
    hold_download: bool,
    fallback_progress: bool,
    hold_install: bool,
) -> (
    AppUpdateClient,
    watch::Receiver<Option<AppUpdateSnapshot>>,
    Arc<FakeBackend>,
    CancellationToken,
    TaskTracker,
) {
    let (events_tx, events) = watch::channel(None);
    let backend = Arc::new(FakeBackend {
        hold_download,
        fallback_progress,
        hold_install,
        fallback_source: Notify::new(),
        continue_fallback: Notify::new(),
        install_started: Notify::new(),
        continue_install: Notify::new(),
        install_finished: Notify::new(),
        download_count: AtomicUsize::new(0),
        install_count: AtomicUsize::new(0),
        download_started: Notify::new(),
        cancellation_seen: Notify::new(),
        allow_cleanup: Notify::new(),
        fail_next_check: AtomicBool::new(false),
    });
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client = AppUpdateClient::spawn(
        AppUpdateArgs {
            backend: backend.clone(),
            events: Arc::new(Events(events_tx)),
            settings: AppUpdateSettings {
                channel: ReleaseChannel::Stable,
                sources: vec![UpdateSource::Github],
                auto_check: false,
                auto_download: false,
            },
            supported: true,
            endpoints: Vec::new(),
            shutdown: shutdown.clone(),
        },
        &tasks,
    )
    .await
    .unwrap();
    (client, events, backend, shutdown, tasks)
}

#[tokio::test]
async fn cancellation_ack_is_separate_from_download_cleanup_completion() {
    let (client, mut events, backend, _, _) = test_client(true, false, false).await;
    assert_eq!(
        client.check().await.unwrap().phase,
        AppUpdatePhase::Checking
    );
    wait_for_phase(&client, &mut events, AppUpdatePhase::Available).await;

    assert_eq!(
        client.download().await.unwrap().phase,
        AppUpdatePhase::Downloading
    );
    backend.download_started.notified().await;
    assert_eq!(
        client.state().await.unwrap().phase,
        AppUpdatePhase::Downloading
    );

    assert_eq!(
        client.cancel_download().await.unwrap().phase,
        AppUpdatePhase::Cancelling
    );
    backend.cancellation_seen.notified().await;
    assert_eq!(
        client.state().await.unwrap().phase,
        AppUpdatePhase::Cancelling
    );

    backend.allow_cleanup.notify_one();
    let cancelled = wait_for_phase(&client, &mut events, AppUpdatePhase::Cancelled).await;
    assert_eq!(cancelled.release.unwrap().version, "2.1.0");
}

#[tokio::test]
async fn manual_cancel_suppresses_automatic_redownload_of_the_same_release() {
    let (client, mut events, backend, _, _) = test_client(true, false, false).await;
    client.check().await.unwrap();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Available).await;
    client.download().await.unwrap();
    backend.download_started.notified().await;
    client.cancel_download().await.unwrap();
    backend.cancellation_seen.notified().await;
    backend.allow_cleanup.notify_one();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Cancelled).await;

    client.configure(AppUpdateSettings {
        channel: ReleaseChannel::Stable,
        sources: vec![UpdateSource::Github],
        auto_check: true,
        auto_download: true,
    });
    wait_for_phase(&client, &mut events, AppUpdatePhase::Available).await;
    assert_eq!(backend.download_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn changing_channel_during_download_cancels_stale_result_then_checks_new_settings() {
    let (client, mut events, backend, _, _) = test_client(true, false, false).await;
    client.check().await.unwrap();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Available).await;
    client.download().await.unwrap();
    backend.download_started.notified().await;

    client.configure(AppUpdateSettings {
        channel: ReleaseChannel::Beta,
        sources: vec![UpdateSource::Github],
        auto_check: true,
        auto_download: false,
    });
    backend.cancellation_seen.notified().await;
    backend.allow_cleanup.notify_one();
    let checked = wait_for_phase(&client, &mut events, AppUpdatePhase::Available).await;
    assert_eq!(checked.phase, AppUpdatePhase::Available);
    assert_eq!(backend.download_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn checking_the_same_release_keeps_the_verified_package() {
    let (client, mut events, backend, _, _) = test_client(false, false, false).await;
    client.check().await.unwrap();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Available).await;
    client.download().await.unwrap();
    let ready = wait_for_phase(&client, &mut events, AppUpdatePhase::Ready).await;
    assert_eq!(ready.downloaded, 3);

    client.check().await.unwrap();
    let ready = wait_for_phase(&client, &mut events, AppUpdatePhase::Ready).await;
    assert_eq!(ready.downloaded, 3);
    assert_eq!(backend.download_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failed_recheck_keeps_a_verified_package_ready_to_install() {
    let (client, mut events, backend, _, _) = test_client(false, false, false).await;
    client.check().await.unwrap();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Available).await;
    client.download().await.unwrap();
    let ready = wait_for_phase(&client, &mut events, AppUpdatePhase::Ready).await;
    assert_eq!(ready.downloaded, 3);

    backend.fail_next_check.store(true, Ordering::SeqCst);
    client.check().await.unwrap();
    let ready = wait_for_phase(&client, &mut events, AppUpdatePhase::Ready).await;
    assert_eq!(ready.release.unwrap().version, "2.1.0");
    assert_eq!(ready.downloaded, 3);
    assert_eq!(
        ready.error.as_deref(),
        Some("temporary check network failure")
    );
}

#[tokio::test]
async fn fallback_source_after_verification_restarts_download_progress() {
    let (client, mut events, backend, _, _) = test_client(false, true, false).await;
    client.check().await.unwrap();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Available).await;
    client.download().await.unwrap();
    backend.fallback_source.notified().await;
    let fallback = wait_for_source(&client, &mut events, UpdateSource::Nyanpasu).await;
    assert_eq!(fallback.phase, AppUpdatePhase::Downloading);
    assert_eq!(fallback.source, Some(UpdateSource::Nyanpasu));
    assert_eq!(fallback.downloaded, 0);
    backend.continue_fallback.notify_one();
    let ready = wait_for_phase(&client, &mut events, AppUpdatePhase::Ready).await;
    assert_eq!(ready.downloaded, 3);
}

#[tokio::test]
async fn failed_install_keeps_the_verified_package_ready_for_retry() {
    let (client, mut events, backend, _, _) = test_client(false, false, false).await;
    client.check().await.unwrap();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Available).await;
    client.download().await.unwrap();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Ready).await;

    client.install().await.unwrap();
    let ready = wait_for_phase(&client, &mut events, AppUpdatePhase::Ready).await;
    assert_eq!(ready.error.as_deref(), Some("not used by this test"));
    client.install().await.unwrap();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Ready).await;
    assert_eq!(backend.install_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn shutdown_waits_for_download_cleanup_before_draining_the_actor() {
    let (client, mut events, backend, shutdown, tasks) = test_client(true, false, false).await;
    client.check().await.unwrap();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Available).await;
    client.download().await.unwrap();
    backend.download_started.notified().await;

    shutdown.cancel();
    backend.cancellation_seen.notified().await;
    backend.allow_cleanup.notify_one();
    tasks.close();
    tasks.wait().await;
    assert!(client.state().await.is_err());
}

#[tokio::test]
async fn shutdown_hands_off_started_install_without_waiting_on_its_child() {
    let (client, mut events, backend, shutdown, tasks) = test_client(false, false, true).await;
    client.check().await.unwrap();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Available).await;
    client.download().await.unwrap();
    wait_for_phase(&client, &mut events, AppUpdatePhase::Ready).await;
    client.install().await.unwrap();
    backend.install_started.notified().await;

    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
    backend.continue_install.notify_one();
    backend.install_finished.notified().await;
}
