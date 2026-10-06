use std::sync::Arc;

use clash_api::{Delay, DelayQuery, ProviderName, ProxyName, Version};
use nyanpasu_core_manager::{CoreError, OperationId};
use nyanpasu_ipc::api::{
    core::v2::{CoreApiConnection, OperationInfo, OperationOutputInfo, OperationPhase},
    status::{CoreControllerInfo, CoreStateDetail},
};
use tokio::sync::{Notify, watch};

use super::{
    super::endpoint::{
        ApiChanges, ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
    },
    ApiError, ApiResponseStream, ApiResult, InstanceApiPort,
};
use crate::core::{CoreClient, EndpointConnectivity};

struct FakeApiPort;

fn unavailable<T>() -> ApiResult<T> {
    Err(ApiError::Unavailable(
        "test API is intentionally unused".into(),
    ))
}

#[async_trait::async_trait]
impl InstanceApiPort for FakeApiPort {
    async fn connections_ws(&self) -> ApiResult<ApiResponseStream<clash_api::ConnectionsSnapshot>> {
        unavailable()
    }
    async fn logs_ws(
        &self,
        _: clash_api::LogLevel,
    ) -> ApiResult<ApiResponseStream<clash_api::LogEntry>> {
        unavailable()
    }
    async fn traffic_ws(&self) -> ApiResult<ApiResponseStream<clash_api::Traffic>> {
        unavailable()
    }
    async fn memory_ws(&self) -> ApiResult<ApiResponseStream<clash_api::Memory>> {
        unavailable()
    }
    async fn proxies(&self) -> ApiResult<clash_api::IndexMap<ProxyName, clash_api::Proxy>> {
        unavailable()
    }
    async fn proxy_providers(
        &self,
    ) -> ApiResult<clash_api::IndexMap<ProviderName, clash_api::ProxyProvider>> {
        unavailable()
    }
    async fn groups(&self) -> ApiResult<clash_api::IndexMap<ProxyName, clash_api::Proxy>> {
        unavailable()
    }
    async fn select_proxy(&self, _: &ProxyName, _: &ProxyName) -> ApiResult<()> {
        unavailable()
    }
    async fn update_proxy_provider(&self, _: &ProviderName) -> ApiResult<()> {
        unavailable()
    }
    async fn configs(&self) -> ApiResult<clash_api::RuntimeConfig> {
        unavailable()
    }
    async fn rule_providers(
        &self,
    ) -> ApiResult<indexmap::IndexMap<clash_api::RuleProviderName, clash_api::RuleProvider>> {
        unavailable()
    }
    async fn rules(&self) -> ApiResult<Vec<clash_api::Rule>> {
        unavailable()
    }
    async fn update_rule_provider(&self, _: &clash_api::RuleProviderName) -> ApiResult<()> {
        unavailable()
    }
    async fn version(&self) -> ApiResult<Version> {
        unavailable()
    }
    async fn proxy_delay(
        &self,
        _: &ProxyName,
        _: Option<&ProviderName>,
        _: &DelayQuery,
    ) -> ApiResult<Delay> {
        unavailable()
    }
    async fn group_delay(
        &self,
        _: &ProxyName,
        _: &DelayQuery,
    ) -> ApiResult<indexmap::IndexMap<ProxyName, u16>> {
        unavailable()
    }
    async fn connections(&self) -> ApiResult<clash_api::ConnectionsSnapshot> {
        unavailable()
    }
    async fn close_connection(&self, _: uuid::Uuid) -> ApiResult<()> {
        unavailable()
    }
    async fn close_all_connections(&self) -> ApiResult<()> {
        unavailable()
    }
}

struct FakeEndpoint {
    binding: watch::Sender<Option<CoreApiConnection>>,
    gate_stop: std::sync::atomic::AtomicBool,
    stop_submitted: Notify,
    release_stop: Notify,
}

#[async_trait::async_trait]
impl ControlEndpoint for FakeEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Local
    }

    async fn api_connection(&self) -> Result<Option<CoreApiConnection>, CoreError> {
        Ok(self.binding.borrow().clone())
    }

    fn api_backend(&self, _: &CoreApiConnection) -> Result<Arc<dyn InstanceApiPort>, ApiError> {
        Ok(Arc::new(FakeApiPort))
    }

    async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
        Ok(Some(Box::pin(futures::stream::unfold(
            self.binding.subscribe(),
            |mut receiver| async move {
                receiver.changed().await.ok()?;
                Some((Ok(()), receiver))
            },
        ))))
    }

    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        if matches!(
            submission.envelope.command,
            nyanpasu_core_manager::CoreCommand::Stop
        ) && self.gate_stop.load(std::sync::atomic::Ordering::SeqCst)
        {
            self.stop_submitted.notify_one();
            self.release_stop.notified().await;
        }
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
            state: Some(CoreStateDetail::Stopped { reason: None }),
            state_changed_at: 0,
            revision: None,
            source_hash: None,
            healthy: Some(true),
            applied_kind: None,
        })
    }
}

fn fake_endpoint() -> Arc<FakeEndpoint> {
    let (binding, _) = watch::channel(Some(CoreApiConnection {
        instance_id: "instance-a".into(),
        controller: CoreControllerInfo::Http("http://127.0.0.1/".into()),
        secret: None,
    }));
    Arc::new(FakeEndpoint {
        binding,
        gate_stop: std::sync::atomic::AtomicBool::new(false),
        stop_submitted: Notify::new(),
        release_stop: Notify::new(),
    })
}

#[tokio::test]
async fn instance_replacement_revokes_every_api_client_clone() {
    let endpoint = fake_endpoint();
    let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
    let api = core.api_client().await.unwrap();
    let clone = api.clone();

    endpoint
        .binding
        .send_modify(|binding| binding.as_mut().unwrap().instance_id = "instance-b".into());
    assert!(matches!(api.version().await, Err(ApiError::Stale)));
    assert!(matches!(clone.version().await, Err(ApiError::Stale)));
    assert!(api.is_revoked());
    core.shutdown().await.unwrap();
}

#[tokio::test]
async fn dropping_the_shutdown_waiter_keeps_owner_cleanup_running() {
    let endpoint = fake_endpoint();
    endpoint
        .gate_stop
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
    let waiter = {
        let core = core.clone();
        tokio::spawn(async move { core.shutdown().await })
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        endpoint.stop_submitted.notified(),
    )
    .await
    .expect("the actor must submit its stop before dropping the waiter");
    waiter.abort();
    endpoint.release_stop.notify_one();

    assert_eq!(
        core.shutdown().await.unwrap().stop.unwrap().unwrap().phase,
        OperationPhase::Succeeded
    );
    assert_eq!(core.status().connectivity, EndpointConnectivity::ShutDown);
}
