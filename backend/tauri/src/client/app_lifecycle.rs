//! The application lifecycle the composition root drives: the startup
//! reconcile, and the shutdown every owner carries out on its own once the root
//! token is cancelled.
use std::future::Future;

use nyanpasu_core::tasks::track_until_shutdown;
use tokio_util::sync::CancellationToken;

use super::{NyanpasuClient, Result, application_workflow::startup::StartupReport};

impl NyanpasuClient {
    /// Proves who owns the runtime and applies the committed configuration,
    /// once per session; a later call returns the first report. Setup blocks
    /// on it until the workflow answers, with no bound (see `TODO(startup)`
    /// in `resolve_setup`); `Unsettled` means the workflow refused the
    /// command or is gone.
    pub(crate) async fn startup_reconcile(&self) -> StartupReport {
        self.inner.application_workflow.startup_reconcile().await
    }

    /// Lets the background sources run: scheduled subscription refreshes,
    /// with a catch-up of the overdue ones, the external file watchers and
    /// the journal ticker. Setup calls it once `startup_reconcile` returns,
    /// whatever it reported: anything a source changes queues behind the
    /// startup command (T10 §2.2).
    pub(crate) fn start_background_sources(&self) -> Result<()> {
        self.inner.profiles.start_producers()?;
        Ok(())
    }

    /// Starts the shutdown and returns at once: every owner stops admitting
    /// new work and tears itself down on its own. A repeated call changes
    /// nothing.
    pub fn request_shutdown(&self) {
        self.inner.shutdown.cancel();
        self.inner.tasks.close();
    }

    /// Waits until every owner has finished tearing itself down and the core
    /// has stopped. There is no bound: an owner that never finishes keeps this
    /// waiting.
    pub async fn wait_shutdown(&self) {
        self.inner.tasks.wait().await;
        // The workflow stops the core in its `post_stop`, which ractor skips
        // when the workflow panicked. Every owner has finished by now, so
        // nothing drives the core any more, and a stop the workflow already
        // made is only replayed.
        let stop = self
            .inner
            .core_api
            .shutdown()
            .await
            .and_then(|report| report.stop);
        if let Err(error) = stop {
            tracing::warn!(%error, "the core was not proven stopped");
        }
    }

    /// A child of the root shutdown token, for Tauri-boundary background
    /// work that is not one of the client's own owners (e.g. per-webview
    /// connection-detail forwarding) but must still end when the root token
    /// does, and needs its own narrower cancellation besides (e.g. one child
    /// per subscription).
    pub(crate) fn shutdown_child_token(&self) -> CancellationToken {
        self.inner.shutdown.child_token()
    }

    /// Runs `producer` as tracked background work, the same way the
    /// client's own owners do (see `track_until_shutdown`): it ends when
    /// `token` is cancelled, and `wait_shutdown` waits for it. `token` must
    /// be `shutdown_child_token()` or one of its descendants, so the root
    /// shutdown still ends it.
    pub(crate) fn spawn_tracked(
        &self,
        token: &CancellationToken,
        producer: impl Future<Output = ()> + Send + 'static,
    ) {
        tauri::async_runtime::spawn(track_until_shutdown(&self.inner.tasks, token, producer));
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use nyanpasu_config::{
        clash::config::overrides::{ClashGuardOverridesPatch, Mode},
        state::window::WindowState,
    };
    use nyanpasu_core_manager::{CoreError, OperationId};
    use nyanpasu_ipc::api::core::v2::{OperationInfo, OperationOutputInfo};
    use tempfile::TempDir;
    use tokio::sync::Notify;

    use super::*;
    use crate::client::{
        effects::{plan::ApplicationEffectPlan, ports::ApplicationEffectsPort},
        hotkey::{
            HotkeyArgs, HotkeyClient,
            ports::{
                HotkeyAction, HotkeyActionSink, HotkeyParseError, MockHotkeyActionSink,
                ShortcutError, ShortcutRegistrar,
            },
        },
        tests::{TestControlEndpoint, test_client_args_with_endpoint},
    };
    use nyanpasu_core::{
        control::endpoint::{
            CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot, CoreSubmission,
            EndpointHandle, ExecutionHost,
        },
        effects::status::{EffectHealth, EffectRevision, EffectStatus},
    };

    // -- the owners' shutdown through the client (V14, V16, V21) -----------

    type Log = Arc<StdMutex<Vec<&'static str>>>;

    fn record(log: &Log, event: &'static str) {
        log.lock().unwrap().push(event);
    }

    fn geometry(width: u32) -> WindowState {
        WindowState {
            width,
            height: 600,
            ..WindowState::default()
        }
    }

    /// The test core. It records each stop and can hold the next applied
    /// reconcile until the test releases it.
    struct RecordingCore {
        delegate: Arc<TestControlEndpoint>,
        log: Log,
        hold: AtomicBool,
        held: Notify,
        release: Notify,
    }

    #[async_trait::async_trait]
    impl ControlEndpoint for RecordingCore {
        fn host(&self) -> ExecutionHost {
            self.delegate.host()
        }

        async fn check_config(&self, submission: CheckSubmission) -> CheckSupport {
            self.delegate.check_config(submission).await
        }

        async fn submit(
            &self,
            submission: CoreSubmission,
        ) -> std::result::Result<OperationInfo, CoreError> {
            if matches!(
                submission.envelope.command,
                nyanpasu_core_manager::CoreCommand::Stop
            ) {
                record(&self.log, "core stopped");
            }
            self.delegate.submit(submission).await
        }

        async fn wait_operation(
            &self,
            id: OperationId,
            timeout: Duration,
        ) -> Option<OperationInfo> {
            let result = self.delegate.wait_operation(id, timeout).await;
            if matches!(
                result.as_ref().and_then(|info| info.output.as_ref()),
                Some(OperationOutputInfo::Reconciled(_))
            ) && self.hold.swap(false, Ordering::SeqCst)
            {
                self.held.notify_one();
                self.release.notified().await;
            }
            result
        }

        async fn status(&self) -> std::result::Result<CoreStatusSnapshot, CoreError> {
            self.delegate.status().await
        }
    }

    /// Effects that apply at once, or park the next plan until released.
    #[derive(Default)]
    struct HeldEffects {
        hold: AtomicBool,
        applying: Notify,
        release: Notify,
    }

    #[async_trait::async_trait]
    impl ApplicationEffectsPort for HeldEffects {
        async fn apply(
            &self,
            revision: EffectRevision,
            plan: ApplicationEffectPlan,
        ) -> Vec<EffectStatus> {
            if self.hold.swap(false, Ordering::SeqCst) {
                self.applying.notify_one();
                self.release.notified().await;
            }
            plan.effects()
                .iter()
                .map(|effect| EffectStatus {
                    kind: effect.kind(),
                    desired_revision: revision,
                    applied_revision: revision,
                    health: EffectHealth::Healthy,
                })
                .collect()
        }
    }

    /// Counts the releases the hotkey owner makes on its way out.
    #[derive(Default)]
    struct CountingRegistrar {
        releases: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ShortcutRegistrar for CountingRegistrar {
        fn validate(&self, _: &str) -> std::result::Result<(), HotkeyParseError> {
            Ok(())
        }

        async fn register(
            &self,
            _: &str,
            _: HotkeyAction,
            _: Arc<dyn HotkeyActionSink>,
        ) -> std::result::Result<(), ShortcutError> {
            Ok(())
        }

        async fn unregister(&self, _: &str) -> std::result::Result<(), ShortcutError> {
            Ok(())
        }

        async fn unregister_all(&self) -> std::result::Result<(), ShortcutError> {
            self.releases.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// A booted client, with a hotkey owner beside it on the same root
    /// token, the way the composition root wires one.
    struct Graph {
        client: NyanpasuClient,
        core: Arc<RecordingCore>,
        effects: Arc<HeldEffects>,
        registrar: Arc<CountingRegistrar>,
        _hotkeys: HotkeyClient,
        log: Log,
        _dir: TempDir,
    }

    impl Graph {
        async fn new() -> Self {
            let graph = Self::unbooted().await;
            graph.core.delegate.prime(&graph.client).await;
            graph
        }

        /// The same graph before StartupReconcile ran, with a host whose core
        /// is stopped, as a fresh one is.
        async fn unbooted() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let log = Log::default();
            let core = Arc::new(RecordingCore {
                delegate: TestControlEndpoint::succeeding(),
                log: log.clone(),
                hold: AtomicBool::new(false),
                held: Notify::new(),
                release: Notify::new(),
            });
            core.delegate.set_status(
                Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
                None,
            );
            let effects = Arc::new(HeldEffects::default());
            let mut args = test_client_args_with_endpoint(&dir, core.clone() as EndpointHandle);
            args.effects = effects.clone();
            let registrar = Arc::new(CountingRegistrar::default());
            let hotkeys = HotkeyClient::spawn(
                HotkeyArgs {
                    registrar: registrar.clone(),
                    sink: Arc::new(MockHotkeyActionSink::new()),
                    shutdown: args.shutdown.child_token(),
                },
                &args.tasks,
            )
            .await
            .unwrap();
            let client =
                tokio::task::spawn_blocking(move || NyanpasuClient::try_new_with_args(args))
                    .await
                    .unwrap()
                    .unwrap();
            Self {
                client,
                core,
                effects,
                registrar,
                _hotkeys: hotkeys,
                log,
                _dir: dir,
            }
        }

        fn events(&self) -> Vec<&'static str> {
            self.log.lock().unwrap().clone()
        }

        /// Whether every owner has finished within `wait`.
        async fn stopped_within(&self, wait: Duration) -> bool {
            tokio::time::timeout(wait, self.client.wait_shutdown())
                .await
                .is_ok()
        }
    }

    const SETTLE: Duration = Duration::from_secs(10);
    const HELD: Duration = Duration::from_millis(100);

    fn mode(mode: Mode) -> ClashGuardOverridesPatch {
        ClashGuardOverridesPatch {
            mode: Some(mode),
            ..Default::default()
        }
    }

    /// V14: concurrent and repeated requests are one signal: every owner
    /// tears itself down once.
    #[tokio::test(flavor = "multi_thread")]
    async fn repeated_and_concurrent_requests_tear_each_owner_down_once() {
        let g = Graph::new().await;

        let (first, second) = (g.client.clone(), g.client.clone());
        tokio::join!(async move { first.request_shutdown() }, async move {
            second.request_shutdown()
        },);
        g.client.request_shutdown();
        assert!(g.stopped_within(SETTLE).await);
        g.client.request_shutdown();
        g.client.wait_shutdown().await;

        assert_eq!(g.events(), ["core stopped"]);
        assert_eq!(g.registrar.releases.load(Ordering::SeqCst), 1);
    }

    /// V16 (X2): a Try in flight when the shutdown begins runs to its end;
    /// the core stops only after it, through the workflow.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_try_in_flight_finishes_before_the_core_stops() {
        let g = Graph::new().await;
        g.core.hold.store(true, Ordering::SeqCst);
        let mutation = tokio::spawn({
            let client = g.client.clone();
            async move { client.patch_runtime_overrides(mode(Mode::Global)).await }
        });
        g.core.held.notified().await;

        g.client.request_shutdown();
        assert!(!g.stopped_within(HELD).await, "the Try still runs");
        assert!(g.events().is_empty(), "{:?}", g.events());

        record(&g.log, "try released");
        g.core.release.notify_one();
        mutation.await.unwrap().unwrap();
        assert!(g.stopped_within(SETTLE).await);

        assert_eq!(g.events(), ["try released", "core stopped"]);
        assert_eq!(
            g.client.get_clash_config().await.unwrap().overrides.mode(),
            Mode::Global
        );
    }

    /// V16 (X12): a startup still running when the shutdown begins is waited
    /// for, and the core is stopped after it, never behind its back.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_startup_in_flight_finishes_before_the_core_stops() {
        let g = Graph::unbooted().await;
        g.core.hold.store(true, Ordering::SeqCst);
        let startup = tokio::spawn({
            let client = g.client.clone();
            async move { client.startup_reconcile().await }
        });
        g.core.held.notified().await;

        g.client.request_shutdown();
        assert!(!g.stopped_within(HELD).await, "the startup still runs");
        assert!(g.events().is_empty(), "{:?}", g.events());

        record(&g.log, "startup released");
        g.core.release.notify_one();
        startup.await.unwrap();
        assert!(g.stopped_within(SETTLE).await);

        assert_eq!(g.events(), ["startup released", "core stopped"]);
    }

    /// V18 at the client: an owner whose work is stuck holds back no other
    /// owner, and the shutdown keeps waiting for it rather than giving up.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_stuck_owner_holds_back_no_other_and_the_wait_keeps_waiting() {
        let g = Graph::new().await;
        g.effects.hold.store(true, Ordering::SeqCst);
        let mut patch = <nyanpasu_config::application::NyanpasuAppConfig as struct_patch::Patch<
            _,
        >>::new_empty_patch();
        patch.language = Some(nyanpasu_config::application::I18nLanguage::Korean);
        g.client.patch_app_config(patch).await.unwrap();
        g.effects.applying.notified().await;

        g.client.request_shutdown();

        tokio::time::timeout(SETTLE, async {
            while g.events().is_empty() || g.registrar.releases.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the core stopped and the shortcuts were released");
        assert!(
            !g.stopped_within(HELD).await,
            "the effects group still runs"
        );

        g.effects.release.notify_one();
        assert!(g.stopped_within(SETTLE).await);
    }

    /// V21: a geometry save queued before the cancel is written before the
    /// session owner stops; one sent after it finds the owner gone.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_final_geometry_queued_before_the_cancel_is_written() {
        let g = Graph::new().await;

        g.client
            .queue_main_window_geometry_save(geometry(1234))
            .unwrap();
        g.client.request_shutdown();
        assert!(g.stopped_within(SETTLE).await);

        assert_eq!(g.client.main_window_geometry(), Some(geometry(1234)));
        assert!(
            g.client
                .queue_main_window_geometry_save(geometry(4321))
                .is_err(),
            "the session owner has drained"
        );
        assert_eq!(g.client.main_window_geometry(), Some(geometry(1234)));
    }

    /// A workflow that died without its `post_stop`, as a panicking one does,
    /// still leaves the core stopped once every owner has finished.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_core_stops_even_when_the_workflow_died_without_its_cleanup() {
        let g = Graph::new().await;
        g.client.inner.application_workflow.kill().await;

        g.client.request_shutdown();
        assert!(g.stopped_within(SETTLE).await);

        assert_eq!(g.events(), ["core stopped"]);
    }

    /// The owners with nothing to persist stop on the token too, so a client
    /// that outlives the shutdown (the IPC cleanup) polls the core no more.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_shutdown_stops_the_streams_proxies_and_log_owners() {
        let g = Graph::new().await;
        g.client.inner.streams.start().await.unwrap();

        g.client.request_shutdown();
        assert!(g.stopped_within(SETTLE).await);

        let streams = g.client.inner.streams.snapshot().await.unwrap_err();
        assert_eq!(streams.to_string(), "Clash stream actor unavailable");
        let proxies = g.client.inner.proxies.get(false).await.unwrap_err();
        assert_eq!(proxies.to_string(), "proxy actor is unavailable");
        assert!(matches!(
            g.client
                .inner
                .app_logs
                .close("owner".into(), "session".into())
                .await,
            Err(nyanpasu_logging::LogError::Unavailable)
        ));
    }
}
