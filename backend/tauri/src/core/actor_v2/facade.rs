//! Application-facing orchestration over the core and service actors.

use std::time::Duration;

use futures_util::future::{BoxFuture, FutureExt, Shared};
use nyanpasu_config::application::ClashCore;
use nyanpasu_core_manager::{
    ConfigInput, CoreCommand, CoreCommandEnvelope, CoreError, CoreErrorKind, CoreSpec,
    InstanceOptions, OperationId, ReconcileRequest, RevisionId,
};
use nyanpasu_ipc::api::core::v2::{OperationInfo, OperationOutputInfo, OperationPhase};
use tokio::sync::OnceCell;

use super::{
    CoreClient, CoreStatusProjection, HandoffReport, ShutdownReport,
    endpoint::{CoreSubmission, ExecutionHost},
    intent::RuntimeIntentBuilder,
    service_actor::{ServiceClient, ServiceHostStatus},
};

const OPERATION_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq)]
pub struct ReconcileReport {
    pub output: OperationOutputInfo,
    pub status: CoreStatusProjection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StopReport {
    pub output: OperationOutputInfo,
    pub status: CoreStatusProjection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecoverReport {
    pub output: OperationOutputInfo,
    pub status: CoreStatusProjection,
}

type SharedShutdown = Shared<BoxFuture<'static, ShutdownReport>>;

pub struct CoreFacade {
    core: CoreClient,
    service: ServiceClient,
    shutdown: OnceCell<SharedShutdown>,
}

impl CoreFacade {
    pub fn new(core: CoreClient, service: ServiceClient) -> Self {
        Self {
            core,
            service,
            shutdown: OnceCell::new(),
        }
    }

    pub async fn reconcile(
        &self,
        core: ClashCore,
        document: &serde_yaml::Mapping,
        core_spec: CoreSpec,
    ) -> Result<ReconcileReport, CoreError> {
        let expected_applied = self
            .core
            .status()
            .snapshot
            .and_then(|snapshot| snapshot.revision);
        let core_type: nyanpasu_utils::core::CoreType = (&core).into();
        let intent = RuntimeIntentBuilder::build(core_type.clone(), document, expected_applied)
            .map_err(|error| {
                CoreError::new(
                    CoreErrorKind::InvalidConfig,
                    format!("failed to serialize runtime config: {error}"),
                    false,
                )
            })?;
        let expected_applied = intent.expected_applied.map(|revision| RevisionId {
            epoch: revision.epoch,
            generation: revision.generation,
            effective_hash: revision.effective_hash,
        });
        let submission = CoreSubmission {
            envelope: CoreCommandEnvelope {
                operation_id: OperationId::generate(),
                command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                    core: core_spec,
                    config: ConfigInput::Inline {
                        bytes: intent.config_text.into_bytes(),
                        expected_digest: Some(intent.digest),
                    },
                    options: InstanceOptions::default(),
                    expected_applied,
                })),
            },
            core_type: Some(core_type),
        };
        let output = self.submit_and_wait(submission).await?;
        if !matches!(output, OperationOutputInfo::Reconciled(_)) {
            return Err(unexpected_output("reconcile", &output));
        }
        Ok(ReconcileReport {
            output,
            status: self.core.status(),
        })
    }

    pub async fn stop(&self) -> Result<StopReport, CoreError> {
        let output = self.command(CoreCommand::Stop).await?;
        if output != OperationOutputInfo::Stopped {
            return Err(unexpected_output("stop", &output));
        }
        Ok(StopReport {
            output,
            status: self.core.status(),
        })
    }

    pub async fn recover(&self) -> Result<RecoverReport, CoreError> {
        let output = self.command(CoreCommand::Recover).await?;
        if output != OperationOutputInfo::Recovered {
            return Err(unexpected_output("recover", &output));
        }
        Ok(RecoverReport {
            output,
            status: self.core.status(),
        })
    }

    pub async fn change_execution_host(
        &self,
        host: ExecutionHost,
    ) -> Result<HandoffReport, CoreError> {
        let target = match host {
            ExecutionHost::Local => self.core.initial_endpoint(),
            ExecutionHost::Service => self.service.ensure_ready().await?,
        };
        self.core.change_host(target).await
    }

    pub fn core_status(&self) -> CoreStatusProjection {
        self.core.status()
    }

    pub fn subscribe_core_events(&self) -> tokio::sync::broadcast::Receiver<CoreStatusProjection> {
        self.core.subscribe_events()
    }

    pub fn service_status(&self) -> ServiceHostStatus {
        self.service.status()
    }

    pub async fn uninstall_service(&self) -> Result<(), CoreError> {
        self.service.uninstall().await
    }

    pub async fn shutdown(&self) -> ShutdownReport {
        self.shutdown
            .get_or_init(|| {
                let core = self.core.clone();
                std::future::ready(
                    async move {
                        match core.shutdown().await {
                            Ok(report) => report,
                            Err(error) => ShutdownReport {
                                stop: Err(error),
                                final_status: core.status().snapshot,
                            },
                        }
                    }
                    .boxed()
                    .shared(),
                )
            })
            .await
            .clone()
            .await
    }

    async fn command(&self, command: CoreCommand) -> Result<OperationOutputInfo, CoreError> {
        self.submit_and_wait(CoreSubmission {
            envelope: CoreCommandEnvelope {
                operation_id: OperationId::generate(),
                command,
            },
            core_type: None,
        })
        .await
    }

    async fn submit_and_wait(
        &self,
        submission: CoreSubmission,
    ) -> Result<OperationOutputInfo, CoreError> {
        let ticket = self.core.submit(submission).await?;
        let info = ticket
            .endpoint
            .wait_operation(ticket.id, OPERATION_WAIT)
            .await
            .ok_or_else(|| {
                CoreError::new(
                    CoreErrorKind::BackendUnavailable,
                    "the admitted core operation disappeared before reaching a terminal state",
                    true,
                )
                .with_operation(ticket.id)
            })?;
        terminal_output(info, ticket.id)
    }
}

fn terminal_output(info: OperationInfo, id: OperationId) -> Result<OperationOutputInfo, CoreError> {
    match info.phase {
        OperationPhase::Succeeded => info.output.ok_or_else(|| {
            CoreError::new(
                CoreErrorKind::Internal,
                "a successful core operation had no output",
                false,
            )
            .with_operation(id)
        }),
        OperationPhase::Failed => {
            let error =
                info.error
                    .unwrap_or_else(|| nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                        kind: None,
                        message: "the core operation failed without an error payload".into(),
                        retryable: false,
                    });
            Err(CoreError {
                kind: error.kind.as_deref().and_then(CoreErrorKind::from_wire),
                message: error.message,
                retryable: error.retryable,
                operation_id: Some(id),
            })
        }
        OperationPhase::Queued | OperationPhase::Running => Err(CoreError::new(
            CoreErrorKind::BackendUnavailable,
            "the core operation did not reach a terminal state before the wait elapsed",
            true,
        )
        .with_operation(id)),
    }
}

fn unexpected_output(command: &str, output: &OperationOutputInfo) -> CoreError {
    CoreError::new(
        CoreErrorKind::Internal,
        format!("{command} returned an unexpected operation output: {output:?}"),
        false,
    )
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use camino::Utf8PathBuf;
    use nyanpasu_core_manager::{CoreKind, CoreSpec};
    use nyanpasu_ipc::{
        api::{
            core::v2::{
                OperationInfo, OperationOutputInfo, OperationPhase, ReconcileOutcomeInfo,
                ReconcileOutcomeKind,
            },
            status::{
                ConfigRevisionInfo, CoreInfos, CoreState, CoreStateDetail, RevisionIdInfo,
                RuntimeInfos, StatusResBody,
            },
        },
        types::{ServiceStatus, StatusInfo},
    };

    use super::*;
    use crate::core::actor_v2::{
        endpoint::{BoxFuture, ControlEndpoint, CoreStatusSnapshot},
        service_actor::ServiceHostAdapter,
    };

    struct RecordingEndpoint {
        host: ExecutionHost,
        status: CoreStatusSnapshot,
        submissions: Mutex<Vec<CoreSubmission>>,
        stops: AtomicUsize,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl RecordingEndpoint {
        fn new(host: ExecutionHost, revision: Option<RevisionIdInfo>) -> Arc<Self> {
            Arc::new(Self {
                host,
                status: CoreStatusSnapshot {
                    state: Some(CoreStateDetail::Running { pid: 7, epoch: 1 }),
                    state_changed_at: 1,
                    revision,
                    healthy: Some(true),
                },
                submissions: Mutex::new(Vec::new()),
                stops: AtomicUsize::new(0),
                calls: Arc::new(Mutex::new(Vec::new())),
            })
        }

        fn operation(submission: &CoreSubmission) -> OperationInfo {
            let output = match submission.envelope.command {
                CoreCommand::Reconcile(_) => {
                    OperationOutputInfo::Reconciled(ReconcileOutcomeInfo {
                        outcome: ReconcileOutcomeKind::Noop,
                        revision: ConfigRevisionInfo {
                            epoch: 1,
                            generation: 2,
                            source_hash: "source".into(),
                            effective_hash: "effective".into(),
                        },
                        warning: None,
                        failed_apply: None,
                    })
                }
                CoreCommand::Stop => OperationOutputInfo::Stopped,
                CoreCommand::Recover => OperationOutputInfo::Recovered,
                CoreCommand::Shutdown => OperationOutputInfo::ShutDown,
            };
            OperationInfo {
                id: submission.envelope.operation_id.to_string(),
                phase: OperationPhase::Succeeded,
                output: Some(output),
                error: None,
            }
        }
    }

    impl ControlEndpoint for RecordingEndpoint {
        fn host(&self) -> ExecutionHost {
            self.host
        }

        fn submit<'a>(
            &'a self,
            submission: CoreSubmission,
        ) -> BoxFuture<'a, Result<OperationInfo, CoreError>> {
            Box::pin(async move {
                if matches!(submission.envelope.command, CoreCommand::Stop) {
                    self.stops.fetch_add(1, Ordering::SeqCst);
                }
                let info = Self::operation(&submission);
                self.submissions.lock().unwrap().push(submission);
                Ok(info)
            })
        }

        fn wait_operation<'a>(
            &'a self,
            id: OperationId,
            _timeout: Duration,
        ) -> BoxFuture<'a, Option<OperationInfo>> {
            Box::pin(async move {
                self.submissions
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|submission| submission.envelope.operation_id == id)
                    .map(Self::operation)
            })
        }

        fn status<'a>(&'a self) -> BoxFuture<'a, Result<CoreStatusSnapshot, CoreError>> {
            Box::pin(async move {
                if self.host == ExecutionHost::Service {
                    self.calls.lock().unwrap().push("change_host");
                }
                Ok(self.status.clone())
            })
        }
    }

    struct ReadyService {
        endpoint: Arc<RecordingEndpoint>,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl ServiceHostAdapter for ReadyService {
        fn probe(&self) -> BoxFuture<'_, Result<StatusInfo<'static>, String>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push("ensure_ready");
                Ok(StatusInfo {
                    name: Cow::Borrowed("nyanpasu-service"),
                    version: Cow::Borrowed("test"),
                    status: ServiceStatus::Running,
                    server: Some(StatusResBody {
                        version: Cow::Borrowed("2.0.0"),
                        core_infos: CoreInfos {
                            r#type: None,
                            state: CoreState::Running,
                            state_changed_at: 1,
                            config_path: None,
                            controller: None,
                            health: None,
                            revision: None,
                            detail: Some(CoreStateDetail::Stopped { reason: None }),
                        },
                        runtime_infos: RuntimeInfos {
                            service_data_dir: Cow::Owned(Default::default()),
                            service_config_dir: Cow::Owned(Default::default()),
                            nyanpasu_config_dir: Cow::Owned(Default::default()),
                            nyanpasu_data_dir: Cow::Owned(Default::default()),
                        },
                        logs: None,
                    }),
                })
            })
        }

        fn install(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
        fn uninstall(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
        fn start_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
        fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
        fn update(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
        fn endpoint(&self) -> crate::core::actor_v2::endpoint::EndpointHandle {
            self.endpoint.clone()
        }
    }

    async fn facade(local: Arc<RecordingEndpoint>) -> (CoreFacade, Arc<Mutex<Vec<&'static str>>>) {
        let core = CoreClient::spawn(local).await.unwrap();
        let service_endpoint = RecordingEndpoint::new(ExecutionHost::Service, None);
        let calls = service_endpoint.calls.clone();
        let service = ServiceClient::spawn(
            Arc::new(ReadyService {
                calls: calls.clone(),
                endpoint: service_endpoint,
            }),
            1,
        )
        .await
        .unwrap();
        calls.lock().unwrap().clear();
        (CoreFacade::new(core, service), calls)
    }

    async fn wait_for_snapshot(core: &CoreClient) {
        let mut status = core.subscribe();
        while status.borrow().snapshot.is_none() {
            status.changed().await.unwrap();
        }
    }

    #[tokio::test]
    async fn reconcile_builds_an_inline_intent_with_the_status_revision_as_cas_token() {
        let expected = RevisionIdInfo {
            epoch: 4,
            generation: 9,
            effective_hash: "old-effective".into(),
        };
        let local = RecordingEndpoint::new(ExecutionHost::Local, Some(expected.clone()));
        let (facade, _) = facade(local.clone()).await;
        wait_for_snapshot(&facade.core).await;
        let document = serde_yaml::from_str("mode: rule\n").unwrap();
        let spec = CoreSpec {
            kind: CoreKind::Mihomo,
            binary_path: Utf8PathBuf::from("fake-mihomo"),
            version: None,
            features: vec![],
        };

        facade
            .reconcile(ClashCore::Mihomo, &document, spec)
            .await
            .unwrap();

        let submissions = local.submissions.lock().unwrap();
        let CoreCommand::Reconcile(request) = &submissions[0].envelope.command else {
            panic!("expected reconcile");
        };
        let ConfigInput::Inline {
            bytes,
            expected_digest,
        } = &request.config;
        let digest = nyanpasu_core_manager::payload_digest(bytes);
        assert_eq!(expected_digest.as_deref(), Some(digest.as_str()));
        assert_eq!(
            request.expected_applied,
            Some(RevisionId {
                epoch: expected.epoch,
                generation: expected.generation,
                effective_hash: expected.effective_hash,
            })
        );
        assert_eq!(submissions[0].core_type, Some((&ClashCore::Mihomo).into()));
    }

    #[tokio::test]
    async fn a_second_shutdown_awaits_the_same_future() {
        let local = RecordingEndpoint::new(ExecutionHost::Local, None);
        let (facade, _) = facade(local.clone()).await;
        let facade = Arc::new(facade);

        let (first, second) = tokio::join!(facade.shutdown(), facade.shutdown());

        assert_eq!(first.stop, second.stop);
        assert_eq!(local.stops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn change_execution_host_to_service_ensures_ready_first() {
        let local = RecordingEndpoint::new(ExecutionHost::Local, None);
        let (facade, calls) = facade(local).await;

        facade
            .change_execution_host(ExecutionHost::Service)
            .await
            .unwrap();

        let calls = calls.lock().unwrap();
        let ensure = calls
            .iter()
            .position(|call| *call == "ensure_ready")
            .unwrap();
        let handoff = calls
            .iter()
            .position(|call| *call == "change_host")
            .unwrap();
        assert!(ensure < handoff);
    }
}
