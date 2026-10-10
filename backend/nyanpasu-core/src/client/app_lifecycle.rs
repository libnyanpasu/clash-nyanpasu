//! The application lifecycle the composition root drives: the startup
//! reconcile, and the shutdown every owner carries out on its own once the root
//! token is cancelled.
use super::{NyanpasuClient, Result, application_workflow::startup::StartupReport};

impl NyanpasuClient {
    /// Proves who owns the runtime and applies the committed configuration,
    /// once per session; a later call returns the first report. Setup blocks
    /// on it until the workflow answers, with no bound (see `TODO(startup)`
    /// in `resolve_setup`); `Unsettled` means the workflow refused the
    /// command or is gone.
    pub async fn startup_reconcile(&self) -> StartupReport {
        self.inner.application_workflow.startup_reconcile().await
    }

    /// Lets the background sources run: scheduled subscription refreshes,
    /// with a catch-up of the overdue ones, the external file watchers and
    /// the journal ticker. Setup calls it once `startup_reconcile` returns,
    /// whatever it reported: anything a source changes queues behind the
    /// startup command (T10 §2.2).
    pub fn start_background_sources(&self) -> Result<()> {
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
}

#[cfg(test)]
mod tests {
    use crate::effects::{plan::ApplicationEffectPlan, ports::ApplicationEffectsPort};
    use std::{
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };

    use nyanpasu_core_manager::{CoreError, OperationId};
    use nyanpasu_ipc::api::core::v2::{OperationInfo, OperationOutputInfo};
    use tempfile::TempDir;
    use tokio::sync::Notify;

    use super::*;
    use crate::{
        client::tests::{TestControlEndpoint, test_client_args_with_endpoint},
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

    /// A booted client with the neutral owners needed by the private shutdown cases.
    struct Graph {
        client: NyanpasuClient,
        core: Arc<RecordingCore>,
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
            let mut args =
                test_client_args_with_endpoint(&dir, core.clone() as EndpointHandle).await;
            args.effects = effects.clone();
            let client = NyanpasuClient::try_new_with_args(args)
                .await
                .unwrap()
                .client;
            Self {
                client,
                core,
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
