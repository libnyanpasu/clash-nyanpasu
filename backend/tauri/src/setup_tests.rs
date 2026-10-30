//! Exercises partial composition through the production setup failure boundary.
use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

use nyanpasu_core_manager::{CoreError, OperationId};
use nyanpasu_ipc::{
    api::core::v2::{OperationInfo, OperationOutputInfo, OperationPhase},
    types::{ServiceStatus, StatusInfo},
};

use super::{StartupFailureHost, StartupOwners};
use crate::{
    core::{
        actor_v2::{
            endpoint::{
                ControlEndpoint, CoreStatusSnapshot, CoreSubmission, EndpointHandle, ExecutionHost,
            },
            service_actor::ServiceHostAdapter,
        },
        service::control::ServiceCommandError,
    },
    startup_failure::{FailureKind, StartupFailure},
};

struct Endpoint(Arc<Mutex<Vec<&'static str>>>);

#[async_trait::async_trait]
impl ControlEndpoint for Endpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Local
    }
    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        self.0.lock().unwrap().push("core stop");
        Ok(OperationInfo {
            id: submission.envelope.operation_id.to_string(),
            phase: OperationPhase::Succeeded,
            output: Some(OperationOutputInfo::Stopped),
            error: None,
        })
    }
    async fn wait_operation(
        &self,
        id: OperationId,
        _: std::time::Duration,
    ) -> Option<OperationInfo> {
        Some(OperationInfo {
            id: id.to_string(),
            phase: OperationPhase::Succeeded,
            output: Some(OperationOutputInfo::Stopped),
            error: None,
        })
    }
    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        Ok(CoreStatusSnapshot {
            controller: None,
            state: Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
            state_changed_at: 0,
            revision: None,
            source_hash: None,
            healthy: None,
            applied_kind: None,
        })
    }
}

struct Service {
    endpoint: EndpointHandle,
    cleanup: Arc<Mutex<Vec<&'static str>>>,
}

impl Drop for Service {
    fn drop(&mut self) {
        self.cleanup.lock().unwrap().push("service released");
    }
}

#[async_trait::async_trait]
impl ServiceHostAdapter for Service {
    async fn probe(&self) -> Result<StatusInfo<'static>, ServiceCommandError> {
        Ok(StatusInfo {
            name: "test".into(),
            version: "test".into(),
            status: ServiceStatus::NotInstalled,
            server: None,
        })
    }
    async fn install(&self) -> Result<(), ServiceCommandError> {
        unreachable!()
    }
    async fn uninstall(&self) -> Result<(), ServiceCommandError> {
        unreachable!()
    }
    async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
        unreachable!()
    }
    async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
        unreachable!()
    }
    async fn update(&self) -> Result<(), ServiceCommandError> {
        unreachable!()
    }
    fn endpoint(&self) -> EndpointHandle {
        self.endpoint.clone()
    }
}

#[derive(Default)]
struct Host {
    events: RefCell<Vec<&'static str>>,
    failure: RefCell<Option<StartupFailure>>,
    owners: RefCell<Option<StartupOwners>>,
}

impl StartupFailureHost for Host {
    fn show(&self, failure: StartupFailure) {
        self.events.borrow_mut().push("dialog");
        self.failure.replace(Some(failure));
    }
    fn retain(&self, owners: StartupOwners) {
        assert!(
            !owners.shutdown.is_cancelled(),
            "setup must not drain on the main thread"
        );
        self.events.borrow_mut().push("retain");
        self.owners.replace(Some(owners));
    }
    fn exit(&self, code: i32) {
        assert_eq!(code, 1);
        assert!(self.owners.borrow().is_some());
        self.events.borrow_mut().push("exit");
    }
}

#[test]
fn jobs_failure_after_core_and_service_start_returns_ok_then_drains_owners() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _entered = runtime.enter();
    let dir = tempfile::tempdir().unwrap();
    let paths =
        crate::client::tests::test_paths(dir.path().join("config"), dir.path().join("data"));
    paths.create_base_dirs().unwrap();
    // A real redb open failure: another handle holds the jobs database.
    let _held = redb::Database::create(paths.jobs_path()).unwrap();
    let cleanup = Arc::new(Mutex::new(Vec::new()));
    let endpoint: EndpointHandle = Arc::new(Endpoint(cleanup.clone()));
    let host = Host::default();
    let mut core_watch = None;
    let (main_thread_request, request_received) = tokio::sync::oneshot::channel();
    let (main_thread_reply, reply_received) = tokio::sync::oneshot::channel();
    let result = super::run_setup(
        StartupOwners::default(),
        |owners| {
            runtime.block_on(async {
                // Models cleanup that requires the event loop to service a main-thread request.
                owners.tasks.spawn({
                    let token = owners.shutdown.clone();
                    let cleanup = cleanup.clone();
                    async move {
                        token.cancelled().await;
                        main_thread_request.send(()).unwrap();
                        reply_received.await.unwrap();
                        cleanup.lock().unwrap().push("owner acknowledged");
                    }
                });
                let error = super::start_runtime_owners(
                    owners,
                    endpoint.clone(),
                    Arc::new(Service {
                        endpoint,
                        cleanup: cleanup.clone(),
                    }),
                    &paths,
                    crate::client::jobs::capture(),
                )
                .await
                .err()
                .expect("a held jobs database cannot be opened");
                assert!(matches!(error, super::Error::StartJobs { .. }));
                core_watch = Some(owners.core.as_ref().unwrap().subscribe());
                Err(StartupFailure::from_setup(&error, &paths))
            })
        },
        &host,
    );
    assert!(result.is_ok());
    assert_eq!(*host.events.borrow(), ["dialog", "retain", "exit"]);
    assert_eq!(
        host.failure.borrow().as_ref().unwrap().kind,
        FailureKind::Busy
    );
    let owners = host.owners.take().unwrap();
    runtime.block_on(async {
        let drain = crate::utils::exit::drain_shutdown_owners(None, Some(owners.clone()));
        assert!(
            owners.shutdown.is_cancelled(),
            "the exit boundary selects partial startup owners"
        );
        let drain = tokio::spawn(drain);
        request_received.await.unwrap();
        assert!(!drain.is_finished());
        main_thread_reply.send(()).unwrap();
        drain.await.unwrap();
    });
    let cleanup = cleanup.lock().unwrap();
    assert_eq!(cleanup.len(), 3);
    assert!(cleanup[..2].contains(&"owner acknowledged"));
    assert!(cleanup[..2].contains(&"service released"));
    assert_eq!(cleanup[2], "core stop");
    assert!(
        core_watch.unwrap().has_changed().is_err(),
        "core actor and pump must release their senders"
    );
}

#[test]
fn rpc_failure_uses_the_same_dialog_and_exit_boundary() {
    let host = Host::default();
    let result = super::run_setup(
        StartupOwners::default(),
        |_| {
            Err(StartupFailure::from_rpc(
                &super::UnifiedRpcError::RegisterState,
            ))
        },
        &host,
    );
    assert!(result.is_ok());
    assert_eq!(*host.events.borrow(), ["dialog", "retain", "exit"]);
    assert_eq!(
        host.failure.borrow().as_ref().unwrap().kind,
        FailureKind::Internal
    );
}
