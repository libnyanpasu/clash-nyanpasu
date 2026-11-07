//! Follows the running core's database mode for the country index owner.
use std::time::Duration;

use nyanpasu_observers::geo::{GeodataMode, ModeReporter, ModeSource};
use tokio::task::JoinHandle;

use crate::actor_v2::CoreClient;

/// How long to wait before asking a core that did not answer again.
const RETRY: Duration = Duration::from_secs(1);

pub(super) struct CoreModeSource {
    core: CoreClient,
}

impl CoreModeSource {
    pub(super) fn new(core: CoreClient) -> Self {
        Self { core }
    }
}

impl ModeSource for CoreModeSource {
    fn start(&self, reporter: ModeReporter) -> JoinHandle<()> {
        tokio::spawn(follow_core(reporter, self.core.clone()))
    }
}

/// Reads `geodata-mode` from every core instance as it binds. Cores without the field (clash-rs,
/// Clash Premium) read the mmdb.
async fn follow_core(reporter: ModeReporter, core: CoreClient) {
    loop {
        let Ok(api) = core.api_client().await else {
            tokio::time::sleep(RETRY).await;
            continue;
        };
        loop {
            match api.configs().await {
                Ok(config) => {
                    let mode = match config.geodata_mode {
                        Some(true) => GeodataMode::Dat,
                        _ => GeodataMode::Mmdb,
                    };
                    if !reporter.report(mode) {
                        return;
                    }
                    break;
                }
                Err(_) if api.is_revoked() => break,
                Err(error) => {
                    tracing::debug!(%error, "the core did not report its geodata mode");
                    tokio::time::sleep(RETRY).await;
                }
            }
        }
        api.cancelled().await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{Json, Router, routing::get};
    use nyanpasu_core_manager::{CoreError, OperationId};
    use nyanpasu_ipc::api::core::v2::{CoreApiConnection, OperationInfo};
    use tokio::{sync::mpsc, time::Instant};

    use super::*;
    use crate::actor_v2::{
        endpoint::{ApiChanges, ControlEndpoint, CoreStatusSnapshot, CoreSubmission},
        test_support::{self, Endpoint},
    };
    use nyanpasu_runtime_values::execution_host::ExecutionHost;

    struct ObservedEndpoint {
        inner: Arc<Endpoint>,
        acquisitions: mpsc::UnboundedSender<Instant>,
        leases: Option<mpsc::UnboundedSender<String>>,
    }

    #[async_trait::async_trait]
    impl ControlEndpoint for ObservedEndpoint {
        fn host(&self) -> ExecutionHost {
            self.inner.host()
        }

        async fn api_connection(&self) -> Result<Option<CoreApiConnection>, CoreError> {
            self.acquisitions.send(Instant::now()).unwrap();
            self.inner.api_connection().await
        }

        async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
            if let Some(leases) = &self.leases {
                leases
                    .send(
                        self.inner
                            .binding
                            .borrow()
                            .as_ref()
                            .unwrap()
                            .instance_id
                            .clone(),
                    )
                    .unwrap();
            }
            self.inner.api_changes().await
        }

        async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
            self.inner.status().await
        }

        async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
            self.inner.submit(submission).await
        }

        async fn wait_operation(
            &self,
            id: OperationId,
            timeout: Duration,
        ) -> Option<OperationInfo> {
            self.inner.wait_operation(id, timeout).await
        }
    }

    #[tokio::test(start_paused = true)]
    async fn retries_missing_leases_at_one_second_intervals() {
        let endpoint = test_support::endpoint("http://127.0.0.1:1/".into());
        endpoint.binding.send_replace(None);
        let (acquisitions, mut attempts) = mpsc::unbounded_channel();
        let (core, _service) =
            test_support::test_v2_clients_with_endpoint(Arc::new(ObservedEndpoint {
                inner: endpoint,
                acquisitions,
                leases: None,
            }))
            .await;
        let follower =
            CoreModeSource::new(core.clone()).start(ModeReporter::new_for_test(|mode| {
                panic!("no lease can report a mode: {mode:?}")
            }));

        let first = attempts.recv().await.unwrap();
        let second = attempts.recv().await.unwrap();
        let third = attempts.recv().await.unwrap();
        assert_eq!(second - first, Duration::from_secs(1));
        assert_eq!(third - second, Duration::from_secs(1));

        follower.abort();
        assert!(follower.await.unwrap_err().is_cancelled());
        core.finish_shutdown().await;
    }

    #[tokio::test(start_paused = true)]
    async fn reacquires_and_reports_once_per_revoked_lease() {
        use nyanpasu_ipc::api::status::CoreControllerInfo;

        let (requests, mut received) = mpsc::unbounded_channel();
        let mut servers = Vec::new();
        let mut bindings = Vec::new();
        for (instance, dat) in [("first-process", true), ("second-process", false)] {
            let requests = requests.clone();
            let (url, server) = test_support::server(Router::new().route(
                "/configs",
                get(move || {
                    requests.send(instance).unwrap();
                    async move { Json(serde_json::json!({"geodata-mode": dat})) }
                }),
            ))
            .await;
            bindings.push(CoreApiConnection {
                instance_id: instance.into(),
                controller: CoreControllerInfo::Http(url),
                secret: None,
            });
            servers.push(server);
        }
        let endpoint = test_support::endpoint(String::new());
        endpoint.binding.send_replace(Some(bindings.remove(0)));
        let (acquisitions, _attempts) = mpsc::unbounded_channel();
        let (leases, mut acquired) = mpsc::unbounded_channel();
        let (core, _service) =
            test_support::test_v2_clients_with_endpoint(Arc::new(ObservedEndpoint {
                inner: endpoint.clone(),
                acquisitions,
                leases: Some(leases),
            }))
            .await;
        let (reports, mut modes) = mpsc::unbounded_channel();
        // Poll the follower ourselves: each checkpoint leaves it suspended at the lease wait,
        // rather than racing a spawned task when checking for duplicate requests or reports.
        let mut follower = Box::pin(follow_core(
            ModeReporter::new_for_test(move |mode| reports.send(mode).is_ok()),
            core.clone(),
        ));

        for (instance, mode) in [
            ("first-process", GeodataMode::Dat),
            ("second-process", GeodataMode::Mmdb),
        ] {
            tokio::select! {
                biased;
                () = &mut follower => panic!("follower stopped before reporting {instance}"),
                report = modes.recv() => assert_eq!(report, Some(mode)),
            }
            assert_eq!(acquired.recv().await.as_deref(), Some(instance));
            assert_eq!(received.recv().await, Some(instance));

            // Cross several retry intervals with the binding unchanged. A repeated acquisition
            // or report must be visible after the follower has been polled past those timers.
            for _ in 0..3 {
                tokio::time::advance(RETRY).await;
                assert!(futures::poll!(&mut follower).is_pending());
                assert!(acquired.try_recv().is_err());
                assert!(received.try_recv().is_err());
                assert!(modes.try_recv().is_err());
            }
            if instance == "first-process" {
                endpoint.binding.send_replace(Some(bindings.remove(0)));
            }
        }

        drop(follower);
        core.finish_shutdown().await;
        for server in servers {
            server.abort();
            assert!(server.await.unwrap_err().is_cancelled());
        }
    }

    #[tokio::test]
    async fn defaults_to_mmdb_and_stops_when_report_is_rejected() {
        let (url, server) = test_support::server(
            Router::new().route("/configs", get(|| async { Json(serde_json::json!({})) })),
        )
        .await;
        let (core, _service) =
            test_support::test_v2_clients_with_endpoint(test_support::endpoint(url)).await;
        let (reports, mut modes) = mpsc::unbounded_channel();
        let follower =
            CoreModeSource::new(core.clone()).start(ModeReporter::new_for_test(move |mode| {
                reports.send(mode).unwrap();
                false
            }));

        follower.await.unwrap();
        assert_eq!(modes.recv().await, Some(GeodataMode::Mmdb));
        assert_eq!(modes.recv().await, None);

        core.finish_shutdown().await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}
