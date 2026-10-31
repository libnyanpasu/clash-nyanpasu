//! Independent actor, service-host, and HTTP endpoint fixtures for backend tests.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use axum::Router;
use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
use nyanpasu_ipc::api::{
    core::v2::{CoreApiConnection, OperationInfo, OperationOutputInfo, OperationPhase},
    status::{CoreControllerInfo, CoreStateDetail},
};
use tokio::sync::watch;

use super::{
    CoreClient as CoreClientV2,
    endpoint::{ApiChanges, ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost},
    service_actor::ServiceClient,
};

struct IdleEndpoint;

#[async_trait::async_trait]
impl crate::actor_v2::endpoint::ControlEndpoint for IdleEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Local
    }

    async fn submit(
        &self,
        submission: crate::actor_v2::endpoint::CoreSubmission,
    ) -> std::result::Result<
        nyanpasu_ipc::api::core::v2::OperationInfo,
        nyanpasu_core_manager::CoreError,
    > {
        Ok(successful_reconcile(submission.envelope.operation_id))
    }

    async fn wait_operation(
        &self,
        id: nyanpasu_core_manager::OperationId,
        _timeout: std::time::Duration,
    ) -> Option<nyanpasu_ipc::api::core::v2::OperationInfo> {
        Some(successful_reconcile(id))
    }

    async fn status(
        &self,
    ) -> std::result::Result<
        crate::actor_v2::endpoint::CoreStatusSnapshot,
        nyanpasu_core_manager::CoreError,
    > {
        Ok(crate::actor_v2::endpoint::CoreStatusSnapshot {
            controller: None,
            state: Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
            state_changed_at: 0,
            revision: None,
            source_hash: None,
            healthy: Some(true),
            applied_kind: None,
        })
    }
}

pub struct HostTransitionServiceAdapter {
    pub endpoint: crate::actor_v2::endpoint::EndpointHandle,
    pub calls: Arc<StdMutex<Vec<&'static str>>>,
    pub stopped: std::sync::atomic::AtomicBool,
    /// The config dir the daemon reports it was installed with.
    pub installed_for: PathBuf,
}

#[async_trait::async_trait]
impl crate::actor_v2::service_actor::ServiceHostAdapter for HostTransitionServiceAdapter {
    async fn probe(
        &self,
    ) -> std::result::Result<
        nyanpasu_ipc::types::StatusInfo<'static>,
        crate::service::control::ServiceCommandError,
    > {
        if self.stopped.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(nyanpasu_ipc::types::StatusInfo {
                name: "test-service".into(),
                version: "2.0.2".into(),
                status: nyanpasu_ipc::types::ServiceStatus::Stopped,
                server: None,
            });
        }
        self.calls.lock().unwrap().push("ensure_ready");
        Ok(nyanpasu_ipc::types::StatusInfo {
            name: std::borrow::Cow::Borrowed("test-service"),
            version: std::borrow::Cow::Borrowed("2.0.2"),
            status: nyanpasu_ipc::types::ServiceStatus::Running,
            server: Some(nyanpasu_ipc::api::status::StatusResBody {
                log_query_version: None,
                version: std::borrow::Cow::Borrowed("2.0.2"),
                core_infos: nyanpasu_ipc::api::status::CoreInfos {
                    instance_id: None,
                    r#type: None,
                    state: nyanpasu_ipc::api::status::CoreState::Running,
                    state_changed_at: 0,
                    config_path: None,
                    controller: None,
                    health: None,
                    revision: None,
                    detail: Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped {
                        reason: None,
                    }),
                },
                runtime_infos: nyanpasu_ipc::api::status::RuntimeInfos {
                    service_data_dir: std::borrow::Cow::Owned(Default::default()),
                    service_config_dir: std::borrow::Cow::Owned(Default::default()),
                    nyanpasu_config_dir: std::borrow::Cow::Owned(self.installed_for.clone()),
                    nyanpasu_data_dir: std::borrow::Cow::Owned(Default::default()),
                },
                logs: None,
            }),
        })
    }

    async fn install(
        &self,
    ) -> std::result::Result<(), crate::service::control::ServiceCommandError> {
        self.calls.lock().unwrap().push("install");
        Ok(())
    }

    async fn uninstall(
        &self,
    ) -> std::result::Result<(), crate::service::control::ServiceCommandError> {
        self.calls.lock().unwrap().push("uninstall");
        Ok(())
    }

    async fn start_daemon(
        &self,
    ) -> std::result::Result<(), crate::service::control::ServiceCommandError> {
        self.stopped
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.calls.lock().unwrap().push("start_daemon");
        Ok(())
    }

    async fn stop_daemon(
        &self,
    ) -> std::result::Result<(), crate::service::control::ServiceCommandError> {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.calls.lock().unwrap().push("stop_daemon");
        Ok(())
    }

    async fn update(
        &self,
    ) -> std::result::Result<(), crate::service::control::ServiceCommandError> {
        Ok(())
    }

    fn endpoint(&self) -> crate::actor_v2::endpoint::EndpointHandle {
        self.endpoint.clone()
    }
}

pub fn successful_reconcile(
    id: nyanpasu_core_manager::OperationId,
) -> nyanpasu_ipc::api::core::v2::OperationInfo {
    nyanpasu_ipc::api::core::v2::OperationInfo {
        id: id.to_string(),
        phase: nyanpasu_ipc::api::core::v2::OperationPhase::Succeeded,
        output: Some(
            nyanpasu_ipc::api::core::v2::OperationOutputInfo::Reconciled(
                nyanpasu_ipc::api::core::v2::ReconcileOutcomeInfo {
                    outcome: nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Started,
                    revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                        epoch: 1,
                        generation: 1,
                        source_hash: "source".into(),
                        effective_hash: "effective".into(),
                    },
                    warning: None,
                    failed_apply: None,
                },
            ),
        ),
        error: None,
    }
}

pub struct IdleServiceAdapter;

#[async_trait::async_trait]
impl crate::actor_v2::service_actor::ServiceHostAdapter for IdleServiceAdapter {
    async fn probe(
        &self,
    ) -> std::result::Result<
        nyanpasu_ipc::types::StatusInfo<'static>,
        crate::service::control::ServiceCommandError,
    > {
        Ok(nyanpasu_ipc::types::StatusInfo {
            name: std::borrow::Cow::Borrowed("test-service"),
            version: std::borrow::Cow::Borrowed("test"),
            status: nyanpasu_ipc::types::ServiceStatus::NotInstalled,
            server: None,
        })
    }

    async fn install(
        &self,
    ) -> std::result::Result<(), crate::service::control::ServiceCommandError> {
        Ok(())
    }

    async fn uninstall(
        &self,
    ) -> std::result::Result<(), crate::service::control::ServiceCommandError> {
        Ok(())
    }

    async fn start_daemon(
        &self,
    ) -> std::result::Result<(), crate::service::control::ServiceCommandError> {
        Ok(())
    }

    async fn stop_daemon(
        &self,
    ) -> std::result::Result<(), crate::service::control::ServiceCommandError> {
        Ok(())
    }

    async fn update(
        &self,
    ) -> std::result::Result<(), crate::service::control::ServiceCommandError> {
        Ok(())
    }

    fn endpoint(&self) -> crate::actor_v2::endpoint::EndpointHandle {
        Arc::new(IdleEndpoint)
    }
}

pub async fn test_v2_clients() -> (CoreClientV2, ServiceClient) {
    test_v2_clients_with_endpoint(test_idle_endpoint()).await
}

/// A core that answers nothing, for tests whose subject is the facade
/// rather than the running core.
pub fn test_idle_endpoint() -> crate::actor_v2::endpoint::EndpointHandle {
    Arc::new(IdleEndpoint)
}

pub async fn test_v2_clients_with_endpoint(
    endpoint: crate::actor_v2::endpoint::EndpointHandle,
) -> (CoreClientV2, ServiceClient) {
    let core = CoreClientV2::spawn(endpoint).await.unwrap();
    let service = ServiceClient::spawn(Arc::new(IdleServiceAdapter), 0)
        .await
        .unwrap();
    (core, service)
}

pub struct Endpoint {
    pub(super) host: ExecutionHost,
    pub binding: watch::Sender<Option<CoreApiConnection>>,
}

#[async_trait::async_trait]
impl ControlEndpoint for Endpoint {
    fn host(&self) -> ExecutionHost {
        self.host
    }

    async fn api_connection(&self) -> Result<Option<CoreApiConnection>, CoreError> {
        Ok(self.binding.borrow().clone())
    }

    async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
        Ok(Some(Box::pin(futures::stream::unfold(
            self.binding.subscribe(),
            |mut rx| async move {
                rx.changed().await.ok()?;
                Some((Ok(()), rx))
            },
        ))))
    }

    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        Ok(CoreStatusSnapshot {
            controller: None,
            state: Some(if self.binding.borrow().is_some() {
                CoreStateDetail::Running { epoch: 1, pid: 7 }
            } else {
                CoreStateDetail::Stopped { reason: None }
            }),
            state_changed_at: 0,
            revision: None,
            source_hash: None,
            healthy: Some(true),
            applied_kind: None,
        })
    }

    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        if !matches!(
            submission.envelope.command,
            nyanpasu_core_manager::CoreCommand::Stop
        ) {
            return Err(CoreError::new(
                CoreErrorKind::Internal,
                "test accepts only stop",
                false,
            ));
        }
        self.binding.send_replace(None);
        Ok(OperationInfo {
            id: submission.envelope.operation_id.to_string(),
            phase: OperationPhase::Succeeded,
            output: Some(OperationOutputInfo::Stopped),
            error: None,
        })
    }

    async fn wait_operation(&self, _: OperationId, _: Duration) -> Option<OperationInfo> {
        None
    }
}

pub fn endpoint(url: String) -> Arc<Endpoint> {
    let (binding, _) = watch::channel(Some(CoreApiConnection {
        instance_id: "first-process".into(),
        controller: CoreControllerInfo::Http(url),
        secret: None,
    }));
    Arc::new(Endpoint {
        binding,
        host: ExecutionHost::Local,
    })
}

pub async fn server(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (url, task)
}
