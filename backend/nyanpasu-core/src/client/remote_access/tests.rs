//! The serial owner's ordering, ownership and truthfulness, with a gated fake listener.

use std::{collections::VecDeque, sync::Arc};

use nyanpasu_config::application::{HttpAccessConfig, ReleaseChannel, RemoteAccessConfig};
use nyanpasu_runtime_values::runtime::{DegradationReason, MutationOutcome};
use parking_lot::Mutex;
use tokio::sync::oneshot;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{RemoteAccessArgs, RemoteAccessClient, SetRemoteAccessError};
use crate::client::{
    application::ApplicationClient,
    http_server::{HttpServerControl, HttpServerControlError, HttpServerFault, HttpServerStatus},
};

const OPEN_PORT: u16 = 40123;

/// One `configure` call that parks until the test releases it.
struct Gate {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

struct Handle {
    entered: oneshot::Receiver<()>,
    release: oneshot::Sender<()>,
}

#[derive(Default)]
struct FakeListener {
    gates: Mutex<VecDeque<Gate>>,
    configured: Mutex<Vec<HttpAccessConfig>>,
    fault: Mutex<Option<HttpServerFault>>,
    fail: Mutex<bool>,
    shutdowns: Mutex<u32>,
}

impl FakeListener {
    /// The next unclaimed `configure` call parks until the handle releases it.
    fn gate_next(&self) -> Handle {
        let (entered_tx, entered) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        self.gates.lock().push_back(Gate {
            entered: entered_tx,
            release: release_rx,
        });
        Handle { entered, release }
    }

    fn configurations(&self) -> Vec<HttpAccessConfig> {
        self.configured.lock().clone()
    }
}

#[async_trait::async_trait]
impl HttpServerControl for FakeListener {
    async fn status(&self) -> anyhow::Result<HttpServerStatus> {
        Ok(HttpServerStatus::default())
    }

    async fn configure(&self, config: &HttpAccessConfig) -> anyhow::Result<HttpServerStatus> {
        self.configured.lock().push(config.clone());
        let gate = self.gates.lock().pop_front();
        if let Some(gate) = gate {
            let _ = gate.entered.send(());
            let _ = gate.release.await;
        }
        anyhow::ensure!(!*self.fail.lock(), "listener failure");
        if !config.enabled {
            return Ok(HttpServerStatus::default());
        }
        if let Some(fault) = *self.fault.lock() {
            return Ok(HttpServerStatus {
                fault: Some(fault),
                ..Default::default()
            });
        }
        let port = config.port.unwrap_or(OPEN_PORT);
        Ok(HttpServerStatus {
            enabled: true,
            url: Some(format!("http://127.0.0.1:{port}/")),
            port: Some(port),
            fault: None,
        })
    }

    async fn launch_url(&self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        *self.shutdowns.lock() += 1;
        Ok(())
    }
}

impl Fixture {
    /// Stops every owner the fixture started, so no test leaks a background actor.
    async fn finish(self) {
        self.stop_keeping_files().await;
    }

    /// Stops the owners and returns the directory holding the config, to reload it.
    async fn stop_keeping_files(self) -> (tempfile::TempDir, camino::Utf8PathBuf) {
        self.shutdown.cancel();
        self.tasks.close();
        self.tasks.wait().await;
        (self.dir, self.config_path)
    }
}

struct Fixture {
    client: RemoteAccessClient,
    listener: Arc<FakeListener>,
    shutdown: CancellationToken,
    tasks: TaskTracker,
    config_path: camino::Utf8PathBuf,
    dir: tempfile::TempDir,
}

async fn fixture(with_listener: bool) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let config_path =
        camino::Utf8PathBuf::from_path_buf(dir.path().join("application.yaml")).unwrap();
    let application = ApplicationClient::new(
        crate::state::mutation::MutationCoordinator::isolated(),
        ReleaseChannel::Stable,
        config_path.clone(),
        shutdown.child_token(),
        &tasks,
    )
    .await
    .unwrap();
    let listener = Arc::new(FakeListener::default());
    let client = RemoteAccessClient::spawn(
        RemoteAccessArgs {
            application: application.clone(),
            http: with_listener.then(|| listener.clone() as Arc<dyn HttpServerControl>),
            shutdown: shutdown.child_token(),
        },
        &tasks,
    )
    .await
    .unwrap();
    Fixture {
        client,
        listener,
        shutdown,
        tasks,
        config_path,
        dir,
    }
}

fn enabled(port: Option<u16>, serve_webui: bool) -> RemoteAccessConfig {
    RemoteAccessConfig {
        http: HttpAccessConfig {
            enabled: true,
            port,
            serve_webui,
            ..Default::default()
        },
    }
}

fn is_degraded_by_listener(outcome: &MutationOutcome<()>) -> bool {
    matches!(
        outcome.degradations(),
        [degradation] if matches!(degradation.reason, DegradationReason::HttpServerUnavailable)
    )
}

#[tokio::test]
async fn conflicting_edits_configure_the_listener_in_commit_order() {
    let fixture = fixture(true).await;
    let first_gate = fixture.listener.gate_next();
    let (a, b) = (enabled(Some(1111), true), enabled(Some(2222), false));

    let first = tokio::spawn({
        let (client, a) = (fixture.client.clone(), a.clone());
        async move { client.set(a).await }
    });
    first_gate.entered.await.unwrap();

    // The first poll submits the change to the owner's mailbox and parks on its reply. The
    // owner is still inside the first change, so nothing of the second may have happened.
    let mut second = Box::pin(fixture.client.set(b.clone()));
    assert!(futures::poll!(second.as_mut()).is_pending());
    assert_eq!(fixture.listener.configurations(), vec![a.http.clone()]);
    assert_eq!(fixture.client.config(), a);

    first_gate.release.send(()).unwrap();
    first.await.unwrap().unwrap();
    second.await.unwrap();

    assert_eq!(
        fixture.listener.configurations(),
        vec![a.http, b.http.clone()]
    );
    assert_eq!(fixture.client.config(), b);
}

#[tokio::test]
async fn recording_the_first_port_cannot_overwrite_a_newer_edit() {
    let fixture = fixture(true).await;
    let gate = fixture.listener.gate_next();
    let newer = enabled(Some(2222), false);

    let first = tokio::spawn({
        let client = fixture.client.clone();
        async move { client.set(enabled(None, true)).await }
    });
    gate.entered.await.unwrap();
    let mut second = Box::pin(fixture.client.set(newer.clone()));
    assert!(futures::poll!(second.as_mut()).is_pending());
    gate.release.send(()).unwrap();
    first.await.unwrap().unwrap();
    second.await.unwrap();

    assert_eq!(
        fixture.client.config(),
        newer,
        "the later edit wins in full"
    );
    let seen = fixture.listener.configurations();
    assert_eq!(
        seen.len(),
        2,
        "recording the port does not reconfigure the listener"
    );
    assert_eq!(seen[1], newer.http);
    fixture.finish().await;
}

#[tokio::test]
async fn a_caller_that_leaves_does_not_cancel_the_admitted_change() {
    let fixture = fixture(true).await;
    let gate = fixture.listener.gate_next();

    let caller = tokio::spawn({
        let client = fixture.client.clone();
        async move { client.set(enabled(None, true)).await }
    });
    gate.entered.await.unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    gate.release.send(()).unwrap();

    // Queued behind the abandoned change, so it observes the finished result.
    fixture.client.reconcile().await.unwrap();
    let config = fixture.client.config();
    assert!(config.http.enabled);
    assert_eq!(
        config.http.port,
        Some(OPEN_PORT),
        "the bound port was recorded"
    );
    assert_eq!(fixture.listener.configurations().len(), 2);
    fixture.finish().await;
}

#[tokio::test]
async fn a_listener_failure_after_the_commit_is_a_degraded_outcome() {
    let fixture = fixture(true).await;
    *fixture.listener.fail.lock() = true;

    let outcome = fixture.client.set(enabled(Some(8765), true)).await.unwrap();

    assert!(is_degraded_by_listener(&outcome));
    assert_eq!(fixture.client.config(), enabled(Some(8765), true));
    fixture.finish().await;
}

#[tokio::test]
async fn a_taken_port_degrades_the_commit_and_is_never_replaced() {
    let fixture = fixture(true).await;
    *fixture.listener.fault.lock() = Some(HttpServerFault::PortInUse);

    let outcome = fixture.client.set(enabled(Some(8765), true)).await.unwrap();

    assert!(is_degraded_by_listener(&outcome));
    assert_eq!(fixture.client.config().http.port, Some(8765));
    assert_eq!(fixture.listener.configurations().len(), 1);
    fixture.finish().await;
}

#[tokio::test]
async fn the_first_start_persists_the_port_and_later_starts_reuse_it() {
    let fixture = fixture(true).await;

    let outcome = fixture.client.set(enabled(None, true)).await.unwrap();
    assert!(outcome.degradations().is_empty());
    assert_eq!(fixture.client.config().http.port, Some(OPEN_PORT));
    fixture.client.reconcile().await.unwrap();
    let seen = fixture.listener.configurations();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].port, Some(OPEN_PORT));

    // The port is durable: a fresh application owner loading the same file after a restart
    // sees it, and a fresh remote-access owner binds that port again.
    let (_dir, path) = fixture.stop_keeping_files().await;
    let tasks = TaskTracker::new();
    let shutdown = CancellationToken::new();
    let reloaded = ApplicationClient::new(
        crate::state::mutation::MutationCoordinator::isolated(),
        ReleaseChannel::Stable,
        path,
        shutdown.child_token(),
        &tasks,
    )
    .await
    .unwrap();
    let persisted = reloaded.snapshot().state.remote_access;
    assert_eq!(persisted.http.port, Some(OPEN_PORT));
    assert!(persisted.http.enabled);
    let listener = Arc::new(FakeListener::default());
    let restarted = RemoteAccessClient::spawn(
        RemoteAccessArgs {
            application: reloaded,
            http: Some(listener.clone() as Arc<dyn HttpServerControl>),
            shutdown: shutdown.child_token(),
        },
        &tasks,
    )
    .await
    .unwrap();
    restarted.reconcile().await.unwrap();
    assert_eq!(listener.configurations().len(), 1);
    assert_eq!(listener.configurations()[0].port, Some(OPEN_PORT));
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[tokio::test]
async fn invalid_persisted_settings_never_reach_the_listener() {
    let fixture = fixture(true).await;
    fixture
        .client
        .set(enabled(Some(OPEN_PORT), true))
        .await
        .unwrap();
    let (_dir, path) = fixture.stop_keeping_files().await;
    let document = std::fs::read_to_string(&path).unwrap();
    let valid_port = format!("port: {OPEN_PORT}");
    assert_eq!(document.matches(&valid_port).count(), 1);
    let invalid_document = document.replace(&valid_port, "port: 0");
    std::fs::write(&path, &invalid_document).unwrap();

    let tasks = TaskTracker::new();
    let shutdown = CancellationToken::new();
    let application = ApplicationClient::new(
        crate::state::mutation::MutationCoordinator::isolated(),
        ReleaseChannel::Stable,
        path.clone(),
        shutdown.child_token(),
        &tasks,
    )
    .await
    .unwrap();
    assert_eq!(
        application.snapshot().state.remote_access.http.port,
        Some(0)
    );
    let listener = Arc::new(FakeListener::default());
    let restarted = RemoteAccessClient::spawn(
        RemoteAccessArgs {
            application,
            http: Some(listener.clone() as Arc<dyn HttpServerControl>),
            shutdown: shutdown.child_token(),
        },
        &tasks,
    )
    .await
    .unwrap();

    let outcome = restarted.reconcile().await.unwrap();
    assert!(is_degraded_by_listener(&outcome));
    assert!(listener.configurations().is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid_document);
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[tokio::test]
async fn nothing_is_committed_without_a_listener_or_with_an_invalid_value() {
    let without = fixture(false).await;
    let error = without
        .client
        .set(enabled(Some(8765), true))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SetRemoteAccessError::Server {
            source: HttpServerControlError::ChangeUnsupported
        }
    ));
    assert_eq!(without.client.config(), RemoteAccessConfig::default());
    without.finish().await;

    let with = fixture(true).await;
    let error = with.client.set(enabled(Some(0), true)).await.unwrap_err();
    assert!(matches!(error, SetRemoteAccessError::Client { .. }));
    assert_eq!(with.client.config(), RemoteAccessConfig::default());
    assert!(with.listener.configurations().is_empty());
    with.finish().await;
}

#[tokio::test]
async fn shutdown_refuses_new_changes_and_stops_the_listener() {
    let fixture = fixture(true).await;
    fixture.client.set(enabled(Some(8765), true)).await.unwrap();

    fixture.shutdown.cancel();
    fixture.tasks.close();
    fixture.tasks.wait().await;

    assert!(matches!(
        fixture.client.set(enabled(Some(9876), true)).await,
        Err(SetRemoteAccessError::OwnerStopped)
    ));
    assert_eq!(*fixture.listener.shutdowns.lock(), 1);
    assert_eq!(fixture.client.config().http.port, Some(8765));
}
