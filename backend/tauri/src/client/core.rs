use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::Context as _;
use async_trait::async_trait;
use nyanpasu_config::application::ClashCore;
use nyanpasu_ipc::api::{core::apply::CoreApplyData, status::RevisionIdInfo};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::sync::watch;

use super::{
    ApplicationClient, CoreLifecycleLease, CoreLifecyclePort, RuntimePaths,
    core_bridge::{
        CheckAndPromoteError, CheckAndPromoteFailure, CheckAndPromotePhase, CoreStatusSnapshot,
        RestartFailure, restore_product,
    },
    runtime::CandidateFile,
};
use crate::core::{
    RunType,
    actor::{
        CoreActor, CoreActorArgs, CoreActorMessage, OperationError, OperationId,
        backend::CoreDegradationSink,
        request::CoreRequestFactory,
        runtime::{RuntimeLifecycleState, RuntimeSnapshot},
        types::{
            BackendObservation, CoreActorError, CoreRequest, CoreStatusView, FaithfulLifecycle,
        },
    },
};

const CORE_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) struct CoreClientArgs {
    pub(crate) mode: RunType,
    pub(crate) requests: CoreRequestFactory,
    pub(crate) degradation: Arc<dyn CoreDegradationSink>,
}

struct CoreClientInner {
    actor_ref: ActorRef<CoreActorMessage>,
    next_operation: AtomicU64,
    status_rx: watch::Receiver<CoreStatusView>,
    lifecycle_rx: watch::Receiver<RuntimeLifecycleState>,
    hint_pending: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct CoreClient {
    inner: Arc<CoreClientInner>,
}

pub struct CoreOperationGuard {
    id: OperationId,
    client: CoreClient,
    acquired: bool,
}

impl CoreClient {
    pub(crate) async fn new(args: CoreClientArgs) -> anyhow::Result<Self> {
        #[cfg(test)]
        return Self::spawn(args, None, None, None).await;
        #[cfg(not(test))]
        Self::spawn(args).await
    }

    #[cfg(test)]
    pub(crate) async fn new_with_backend(
        args: CoreClientArgs,
        backend: crate::core::actor::backend::BackendSlot,
    ) -> anyhow::Result<Self> {
        Self::spawn(args, Some(backend), None, None).await
    }

    #[cfg(test)]
    pub(crate) async fn new_with_backend_and_replace_barrier(
        args: CoreClientArgs,
        backend: crate::core::actor::backend::BackendSlot,
        replace_barrier: crate::core::actor::ReplaceBarrier,
    ) -> anyhow::Result<Self> {
        Self::spawn(args, Some(backend), Some(replace_barrier), None).await
    }

    #[cfg(test)]
    pub(crate) async fn new_with_reconciled_backend(
        args: CoreClientArgs,
        backend: crate::core::actor::backend::TestBackend,
    ) -> anyhow::Result<Self> {
        Self::spawn(
            args,
            Some(crate::core::actor::backend::BackendSlot::Ready(
                crate::core::actor::backend::CoreBackend::Test(backend.clone()),
            )),
            None,
            Some(backend),
        )
        .await
    }

    async fn spawn(
        args: CoreClientArgs,
        #[cfg(test)] backend: Option<crate::core::actor::backend::BackendSlot>,
        #[cfg(test)] replace_barrier: Option<crate::core::actor::ReplaceBarrier>,
        #[cfg(test)] replacement_backend: Option<crate::core::actor::backend::TestBackend>,
    ) -> anyhow::Result<Self> {
        let (status_tx, status_rx) = watch::channel(CoreStatusView::initial());
        let (lifecycle_tx, lifecycle_rx) = watch::channel(RuntimeLifecycleState::default());
        let hint_pending = Arc::new(AtomicBool::new(false));
        let actor_ref = Actor::spawn(
            None,
            CoreActor,
            CoreActorArgs {
                mode: args.mode,
                requests: args.requests,
                degradation: args.degradation,
                status_tx,
                lifecycle_tx,
                hint_pending: hint_pending.clone(),
                #[cfg(test)]
                backend,
                #[cfg(test)]
                replace_barrier,
                #[cfg(test)]
                replacement_backend,
            },
        )
        .await
        .context("failed to spawn core actor")?
        .0;
        Ok(Self {
            inner: Arc::new(CoreClientInner {
                actor_ref,
                next_operation: AtomicU64::new(1),
                status_rx,
                lifecycle_rx,
                hint_pending,
            }),
        })
    }

    pub(crate) fn status(&self) -> CoreStatusView {
        self.inner.status_rx.borrow().clone()
    }

    pub(crate) fn lifecycle(&self) -> RuntimeLifecycleState {
        self.inner.lifecycle_rx.borrow().clone()
    }

    #[cfg(test)]
    pub(crate) fn subscribe_status(&self) -> watch::Receiver<CoreStatusView> {
        self.inner.status_rx.clone()
    }

    pub(crate) async fn begin_operation(&self) -> Result<CoreOperationGuard, OperationError> {
        let mut operation = CoreOperationGuard::pending(self.clone(), self.allocate_operation_id());
        operation.acquire().await?;
        Ok(operation)
    }

    pub(crate) async fn refresh_status(
        &self,
        operation: &CoreOperationGuard,
    ) -> Result<BackendObservation, CoreActorError> {
        self.call(|reply| CoreActorMessage::RefreshStatus {
            operation: operation.id(),
            reply,
        })
        .await
    }

    pub(crate) async fn running(
        &self,
        operation: &CoreOperationGuard,
    ) -> Result<(Option<CoreRequest>, FaithfulLifecycle), CoreActorError> {
        self.call(|reply| CoreActorMessage::RunningIdentity {
            operation: operation.id(),
            reply,
        })
        .await
    }

    pub(crate) async fn publish_promoted(
        &self,
        operation: &CoreOperationGuard,
        snapshot: Arc<RuntimeSnapshot>,
    ) -> Result<(), CoreActorError> {
        self.call(|reply| CoreActorMessage::PublishPromoted {
            operation: operation.id(),
            snapshot,
            reply,
        })
        .await
    }

    pub(crate) async fn publish_applied(
        &self,
        operation: &CoreOperationGuard,
        snapshot: Arc<RuntimeSnapshot>,
    ) -> Result<(), CoreActorError> {
        self.call(|reply| CoreActorMessage::PublishApplied {
            operation: operation.id(),
            snapshot,
            reply,
        })
        .await
    }

    pub(crate) async fn apply_promoted(
        &self,
        operation: &CoreOperationGuard,
        request: &CoreRequest,
        expected: Option<RevisionIdInfo>,
        snapshot: Arc<RuntimeSnapshot>,
    ) -> Result<CoreApplyData, CoreActorError> {
        self.call(|reply| CoreActorMessage::ApplyPromoted {
            operation: operation.id(),
            request: request.clone(),
            expected,
            snapshot,
            reply,
        })
        .await
    }

    pub(crate) async fn check(
        &self,
        operation: &CoreOperationGuard,
        request: &CoreRequest,
    ) -> Result<(), CoreActorError> {
        self.call(|reply| CoreActorMessage::Check {
            operation: operation.id(),
            request: request.clone(),
            reply,
        })
        .await
    }

    pub(crate) async fn run(
        &self,
        operation: &CoreOperationGuard,
        request: &CoreRequest,
    ) -> Result<(), CoreActorError> {
        self.call(|reply| CoreActorMessage::Run {
            operation: operation.id(),
            request: request.clone(),
            reply,
        })
        .await
    }

    pub(crate) async fn stop(&self, operation: &CoreOperationGuard) -> Result<(), CoreActorError> {
        self.call(|reply| CoreActorMessage::Stop {
            operation: operation.id(),
            reply,
        })
        .await
    }

    pub(crate) async fn set_backend(
        &self,
        operation: &CoreOperationGuard,
        mode: RunType,
    ) -> Result<(), CoreActorError> {
        self.call(|reply| CoreActorMessage::SetBackend {
            operation: operation.id(),
            mode,
            reply,
        })
        .await
    }

    pub(crate) async fn shutdown(&self) {
        let _ = self
            .inner
            .actor_ref
            .call(CoreActorMessage::Shutdown, None)
            .await;
    }

    pub fn hint_refresh(&self) {
        if self
            .inner
            .hint_pending
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        if self
            .inner
            .actor_ref
            .cast(CoreActorMessage::RefreshHint)
            .is_err()
        {
            self.inner.hint_pending.store(false, Ordering::Release);
        }
    }

    fn allocate_operation_id(&self) -> OperationId {
        let value = self.inner.next_operation.fetch_add(1, Ordering::Relaxed);
        OperationId::new(value).expect("core operation id space exhausted")
    }

    async fn call<T, F>(&self, make: F) -> Result<T, CoreActorError>
    where
        T: Send + 'static,
        F: FnOnce(RpcReplyPort<Result<T, CoreActorError>>) -> CoreActorMessage,
    {
        match self.inner.actor_ref.call(make, None).await {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::SenderError | CallResult::Timeout) | Err(_) => {
                Err(CoreActorError::ShuttingDown)
            }
        }
    }
}

impl CoreOperationGuard {
    fn pending(client: CoreClient, id: OperationId) -> Self {
        Self {
            id,
            client,
            acquired: true,
        }
    }

    fn id(&self) -> OperationId {
        self.id
    }

    async fn acquire(&mut self) -> Result<(), OperationError> {
        match self
            .client
            .inner
            .actor_ref
            .call(
                |reply| CoreActorMessage::AcquireOperation { id: self.id, reply },
                Some(CORE_ACQUIRE_TIMEOUT),
            )
            .await
        {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::Timeout) => Err(OperationError::AcquireTimeout),
            Ok(CallResult::SenderError) | Err(_) => Err(OperationError::ShuttingDown),
        }
    }

    pub(crate) async fn release(mut self) {
        if self.acquired {
            let _ = self
                .client
                .inner
                .actor_ref
                .cast(CoreActorMessage::ReleaseOperation { id: self.id });
            self.acquired = false;
        }
    }
}

impl Drop for CoreOperationGuard {
    fn drop(&mut self) {
        if self.acquired {
            let _ = self
                .client
                .inner
                .actor_ref
                .cast(CoreActorMessage::ReleaseOperation { id: self.id });
        }
    }
}

impl Drop for CoreClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

pub(crate) struct CoreLifecycleAdapter {
    core: CoreClient,
    application: ApplicationClient,
    requests: CoreRequestFactory,
}

pub(crate) struct CoreLeaseAdapter {
    guard: CoreOperationGuard,
    core: CoreClient,
    application: ApplicationClient,
    requests: CoreRequestFactory,
    runtime_paths: RuntimePaths,
    target_core: Option<ClashCore>,
}

impl CoreLifecycleAdapter {
    pub(crate) fn new(
        core: CoreClient,
        application: ApplicationClient,
        requests: CoreRequestFactory,
    ) -> Self {
        Self {
            core,
            application,
            requests,
        }
    }
}

#[async_trait]
impl CoreLifecyclePort for CoreLifecycleAdapter {
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
        let guard = self.core.begin_operation().await?;
        Ok(Box::new(CoreLeaseAdapter {
            guard,
            core: self.core.clone(),
            application: self.application.clone(),
            requests: self.requests.clone(),
            runtime_paths: self.requests.runtime_paths().clone(),
            target_core: None,
        }))
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let status = self.core.status();
        Ok(CoreStatusSnapshot {
            state: status.state,
            state_changed_at: status.state_changed_at,
            run_type: status.run_type,
        })
    }

    async fn on_profile_change(&self) {
        // TODO(actor-migration): connection interruption still reads Config::verge().
        // Reason: break_when_* and clash API client migration is PR-6 scope.
        // Remove when: interruption reads typed ClashConfig.break_connection.
        let _ =
            crate::core::connection_interruption::ConnectionInterruptionService::on_profile_change(
            )
            .await;
    }
}

#[async_trait]
impl CoreLifecycleLease for CoreLeaseAdapter {
    async fn check_and_promote(
        &mut self,
        candidate: &CandidateFile,
        target_core: ClashCore,
        product: &camino::Utf8Path,
    ) -> Result<[u8; 32], CheckAndPromoteFailure> {
        use sha2::Digest as _;

        self.target_core = Some(target_core);
        if product != self.runtime_paths.product() {
            return Err(check_and_promote_operation_error(
                CheckAndPromotePhase::Promote,
                anyhow::anyhow!("product path must match the lifecycle adapter runtime product"),
            ));
        }
        let bytes = tokio::fs::read(candidate.path()).await.map_err(|error| {
            check_and_promote_operation_error(CheckAndPromotePhase::Promote, error.into())
        })?;
        let mut request = self.requests.for_product(target_core).map_err(|error| {
            check_and_promote_operation_error(
                CheckAndPromotePhase::Check,
                anyhow::Error::new(error),
            )
        })?;
        request.config_path = candidate.path().to_owned();
        self.core
            .check(&self.guard, &request)
            .await
            .map_err(CheckAndPromoteFailure::Actor)?;

        let after = tokio::fs::read(candidate.path()).await.map_err(|error| {
            check_and_promote_operation_error(CheckAndPromotePhase::Promote, error.into())
        })?;
        if after != bytes {
            return Err(check_and_promote_operation_error(
                CheckAndPromotePhase::Promote,
                anyhow::anyhow!("candidate config changed between check and promote"),
            ));
        }
        let candidate_hash: [u8; 32] = sha2::Sha256::digest(&bytes).into();
        if candidate_hash != candidate.bytes_sha256() {
            return Err(check_and_promote_operation_error(
                CheckAndPromotePhase::Promote,
                anyhow::anyhow!("candidate config hash changed before promotion"),
            ));
        }

        restore_product(product.as_std_path(), &bytes)
            .await
            .map_err(|error| {
                check_and_promote_operation_error(CheckAndPromotePhase::Promote, error)
            })?;
        let promoted = tokio::fs::read(product).await.map_err(|error| {
            check_and_promote_operation_error(CheckAndPromotePhase::Promote, error.into())
        })?;
        let promoted_hash: [u8; 32] = sha2::Sha256::digest(&promoted).into();
        if promoted_hash != candidate.bytes_sha256() {
            return Err(check_and_promote_operation_error(
                CheckAndPromotePhase::Promote,
                anyhow::anyhow!("promoted runtime product hash does not match candidate"),
            ));
        }
        Ok(promoted_hash)
    }

    async fn publish_promoted(
        &mut self,
        snapshot: Arc<RuntimeSnapshot>,
    ) -> Result<(), CoreActorError> {
        self.core.publish_promoted(&self.guard, snapshot).await
    }

    async fn publish_applied(
        &mut self,
        snapshot: Arc<RuntimeSnapshot>,
    ) -> Result<(), CoreActorError> {
        self.core.publish_applied(&self.guard, snapshot).await
    }

    async fn running_identity(
        &mut self,
    ) -> Result<(Option<CoreRequest>, FaithfulLifecycle), CoreActorError> {
        self.core.running(&self.guard).await
    }

    async fn apply_promoted(
        &mut self,
        snapshot: Arc<RuntimeSnapshot>,
    ) -> Result<CoreApplyData, CoreActorError> {
        let target_core = self
            .target_core
            .expect("apply_promoted requires check_and_promote on the same lease");
        let request = self
            .requests
            .for_product(target_core)
            .map_err(|error| CoreActorError::Backend(Arc::new(error)))?;
        let expected = self
            .core
            .status()
            .revision
            .as_ref()
            .map(|revision| revision.id());
        for attempt in 0..5 {
            match self
                .core
                .apply_promoted(&self.guard, &request, expected.clone(), snapshot.clone())
                .await
            {
                Ok(data) => return Ok(data),
                Err(CoreActorError::Backend(error))
                    if attempt < 4 && is_apply_transport_failure(error.as_ref()) =>
                {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("bounded apply retry loop always returns")
    }

    async fn restart(&mut self) -> Result<(), RestartFailure> {
        let core = match self.target_core.take() {
            Some(core) => core,
            None => {
                self.application
                    .get()
                    .await
                    .map_err(RestartFailure::Operation)?
                    .state
                    .core
            }
        };
        let request = self
            .requests
            .for_product(core)
            .map_err(|error| RestartFailure::Operation(error.into()))?;
        self.core
            .run(&self.guard, &request)
            .await
            .map_err(RestartFailure::Actor)?;
        Ok(())
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        self.core.stop(&self.guard).await?;
        Ok(())
    }
}

fn check_and_promote_operation_error(
    phase: CheckAndPromotePhase,
    source: anyhow::Error,
) -> CheckAndPromoteFailure {
    CheckAndPromoteFailure::Operation(CheckAndPromoteError { phase, source })
}

fn is_apply_transport_failure(error: &crate::core::actor::backend::CoreBackendError) -> bool {
    matches!(
        error,
        crate::core::actor::backend::CoreBackendError::Service(
            nyanpasu_ipc::client::ClientError::BuildClient(_)
                | nyanpasu_ipc::client::ClientError::Request { .. }
                | nyanpasu_ipc::client::ClientError::WebSocket { .. }
                | nyanpasu_ipc::client::ClientError::HttpStatus { .. }
        )
    )
}

#[cfg(test)]
mod tests {
    use std::{
        future::{Future, poll_fn},
        pin::Pin,
        sync::Mutex,
        task::Poll,
    };

    use camino::Utf8PathBuf;
    use nyanpasu_ipc::api::{
        core::apply::ApplyOutcomeKind,
        status::{ConfigRevisionInfo, CoreState},
    };
    use nyanpasu_utils::core::{ClashCoreType, CoreType};
    use tempfile::TempDir;

    use super::*;
    use crate::{
        client::runtime::{Degradation, DegradationPhase},
        core::actor::{
            ReplaceBarrier,
            backend::{BackendSlot, CoreBackend, LocalBackend, TestBackend},
            request::CoreBinaryResolver,
            runtime::{RuntimeRevision, RuntimeSnapshot, RuntimeSnapshotData},
            types::{FaithfulLifecycle, LifecycleInvariantKind},
        },
        enhance::PostProcessingOutput,
        utils::path::PathResolver,
    };

    const RECOVERY_EXHAUSTED_PREFIX: &str = "core kept crashing; restart budget exhausted";

    #[derive(Clone)]
    struct FixedBinary(Utf8PathBuf);

    impl CoreBinaryResolver for FixedBinary {
        fn resolve(&self, _kind: &CoreType) -> anyhow::Result<Utf8PathBuf> {
            Ok(self.0.clone())
        }
    }

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<Degradation>>);

    impl CoreDegradationSink for RecordingSink {
        fn publish(&self, degradation: Degradation) {
            self.0.lock().unwrap().push(degradation);
        }
    }

    struct TestClient {
        client: CoreClient,
        backend: TestBackend,
        requests: CoreRequestFactory,
        sink: Arc<RecordingSink>,
        _dir: TempDir,
    }

    async fn running_request(client: &CoreClient) -> CoreRequest {
        let operation = client.begin_operation().await.unwrap();
        client.running(&operation).await.unwrap().0.unwrap()
    }

    async fn poll_once_pending<F: Future>(mut future: Pin<&mut F>) {
        poll_fn(|context| {
            assert!(future.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
    }

    fn observation(
        state: CoreState,
        lifecycle: FaithfulLifecycle,
        generation: Option<u64>,
    ) -> BackendObservation {
        BackendObservation {
            view: CoreStatusView {
                state,
                state_changed_at: generation.unwrap_or_default() as i64 + 1,
                run_type: RunType::Normal,
                revision: generation.map(|generation| ConfigRevisionInfo {
                    epoch: 1,
                    generation,
                    source_hash: format!("source-{generation}"),
                    effective_hash: format!("effective-{generation}"),
                }),
                recovery_exhausted: matches!(
                    &lifecycle,
                    FaithfulLifecycle::Stopped { reason: Some(reason) }
                        if reason.starts_with(RECOVERY_EXHAUSTED_PREFIX)
                ),
            },
            lifecycle,
        }
    }

    fn running(generation: u64) -> BackendObservation {
        observation(
            CoreState::Running,
            FaithfulLifecycle::Running,
            Some(generation),
        )
    }

    fn stopped(reason: Option<String>) -> BackendObservation {
        observation(
            CoreState::Stopped(reason.clone()),
            FaithfulLifecycle::Stopped { reason },
            None,
        )
    }

    fn exhausted(reason_suffix: &str) -> BackendObservation {
        let reason = format!("{RECOVERY_EXHAUSTED_PREFIX}\n{reason_suffix}");
        stopped(Some(reason))
    }

    async fn test_client(initial: BackendObservation) -> TestClient {
        test_client_with_options(initial, None, false).await
    }

    async fn test_client_with_replace_barrier(
        initial: BackendObservation,
        replace_barrier: Option<ReplaceBarrier>,
    ) -> TestClient {
        test_client_with_options(initial, replace_barrier, false).await
    }

    async fn test_client_with_scripted_replacement(initial: BackendObservation) -> TestClient {
        test_client_with_options(initial, None, true).await
    }

    async fn test_client_with_options(
        initial: BackendObservation,
        replace_barrier: Option<ReplaceBarrier>,
        scripted_replacement: bool,
    ) -> TestClient {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let data = dir.path().join("data");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let paths = PathResolver::with_base_dirs(config.clone(), data);
        let runtime = Utf8PathBuf::from_path_buf(config.join("runtime")).unwrap();
        let requests = CoreRequestFactory::new(
            &paths,
            RuntimePaths::new(runtime.join("config.yaml"), runtime.join(".candidates")),
            Arc::new(FixedBinary(Utf8PathBuf::from("core"))),
        )
        .unwrap();
        let backend = TestBackend::new(initial);
        let sink = Arc::new(RecordingSink::default());
        let args = CoreClientArgs {
            mode: RunType::Normal,
            requests: requests.clone(),
            degradation: sink.clone(),
        };
        let client = if scripted_replacement {
            CoreClient::new_with_reconciled_backend(args, backend.clone()).await
        } else {
            let slot = BackendSlot::Ready(CoreBackend::Test(backend.clone()));
            match replace_barrier {
                Some(barrier) => {
                    CoreClient::new_with_backend_and_replace_barrier(args, slot, barrier).await
                }
                None => CoreClient::new_with_backend(args, slot).await,
            }
        }
        .unwrap();
        TestClient {
            client,
            backend,
            requests,
            sink,
            _dir: dir,
        }
    }

    fn request(factory: &CoreRequestFactory, core: ClashCore) -> CoreRequest {
        factory.for_product(core).unwrap()
    }

    fn runtime_snapshot(revision: u64, core: ClashCore) -> Arc<RuntimeSnapshot> {
        Arc::new(RuntimeSnapshot::from_data(
            RuntimeRevision(revision),
            core,
            Arc::from([]),
            RuntimeSnapshotData {
                config: Default::default(),
                exists_keys: Vec::new(),
                postprocessing_output: PostProcessingOutput::default(),
            },
        ))
    }

    #[tokio::test]
    async fn promoted_revision_must_advance_monotonically() {
        let test = test_client(stopped(None)).await;
        let operation = test.client.begin_operation().await.unwrap();
        let snapshot = runtime_snapshot(1, ClashCore::Mihomo);
        test.client
            .publish_promoted(&operation, snapshot.clone())
            .await
            .unwrap();
        let error = test
            .client
            .publish_promoted(&operation, snapshot.clone())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            CoreActorError::LifecycleInvariant(LifecycleInvariantKind::PromotedRegression)
        ));
        assert!(Arc::ptr_eq(
            test.client.lifecycle().promoted.as_ref().unwrap(),
            &snapshot
        ));
    }

    #[tokio::test]
    async fn applied_requires_the_matching_promoted_snapshot() {
        let test = test_client(stopped(None)).await;
        let operation = test.client.begin_operation().await.unwrap();
        let promoted = runtime_snapshot(1, ClashCore::Mihomo);
        let other = runtime_snapshot(2, ClashCore::Mihomo);

        let missing = test
            .client
            .publish_applied(&operation, promoted.clone())
            .await
            .unwrap_err();
        assert!(matches!(
            missing,
            CoreActorError::LifecycleInvariant(LifecycleInvariantKind::AppliedWithoutPromoted)
        ));

        test.client
            .publish_promoted(&operation, promoted.clone())
            .await
            .unwrap();
        let mismatched = test
            .client
            .publish_applied(&operation, other)
            .await
            .unwrap_err();
        assert!(matches!(
            mismatched,
            CoreActorError::LifecycleInvariant(LifecycleInvariantKind::AppliedWithoutPromoted)
        ));

        test.client
            .publish_applied(&operation, promoted.clone())
            .await
            .unwrap();
        assert!(Arc::ptr_eq(
            test.client.lifecycle().applied.as_ref().unwrap(),
            &promoted
        ));
    }

    #[tokio::test]
    async fn actor_apply_advances_applied_for_all_outcomes_except_rollback() {
        let outcomes = [
            ApplyOutcomeKind::Noop,
            ApplyOutcomeKind::Patched,
            ApplyOutcomeKind::Reloaded,
            ApplyOutcomeKind::Restarted,
            ApplyOutcomeKind::Switched,
            ApplyOutcomeKind::RolledBack,
        ];
        for outcome in outcomes {
            for warning in [None, Some("durability uncertain".to_owned())] {
                let test = test_client(running(5)).await;
                let operation = test.client.begin_operation().await.unwrap();
                let promoted = runtime_snapshot(10, ClashCore::Mihomo);
                test.client
                    .publish_promoted(&operation, promoted.clone())
                    .await
                    .unwrap();
                test.backend.push_apply_result(Ok(CoreApplyData {
                    outcome,
                    revision: ConfigRevisionInfo {
                        epoch: 1,
                        generation: 6,
                        source_hash: "source-6".into(),
                        effective_hash: "effective-6".into(),
                    },
                    warning,
                    failed_apply: (outcome == ApplyOutcomeKind::RolledBack)
                        .then(|| "rejected".into()),
                }));
                let request = request(&test.requests, ClashCore::Mihomo);
                let data = test
                    .client
                    .apply_promoted(
                        &operation,
                        &request,
                        test.client
                            .status()
                            .revision
                            .as_ref()
                            .map(|revision| revision.id()),
                        promoted.clone(),
                    )
                    .await
                    .unwrap();
                assert_eq!(data.outcome, outcome);
                assert_eq!(test.backend.apply_calls(), 1);
                let applied = test.client.lifecycle().applied;
                if outcome == ApplyOutcomeKind::RolledBack {
                    assert!(applied.is_none());
                } else {
                    assert!(Arc::ptr_eq(applied.as_ref().unwrap(), &promoted));
                }
            }
        }
    }

    #[tokio::test]
    async fn lifecycle_read_remains_live_while_run_is_blocked() {
        let test = test_client(stopped(None)).await;
        let operation = test.client.begin_operation().await.unwrap();
        let snapshot = runtime_snapshot(1, ClashCore::Mihomo);
        test.client
            .publish_promoted(&operation, snapshot.clone())
            .await
            .unwrap();
        let (started, release) = test.backend.block_next_run();
        let client = test.client.clone();
        let request = request(&test.requests, ClashCore::Mihomo);
        let run = tokio::spawn(async move {
            let result = client.run(&operation, &request).await;
            drop(operation);
            result
        });
        started.await.unwrap();
        assert!(Arc::ptr_eq(
            test.client.lifecycle().promoted.as_ref().unwrap(),
            &snapshot
        ));
        release.send(()).unwrap();
        run.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn mutation_with_wrong_id_returns_stale() {
        let test = test_client(stopped(None)).await;
        let active = test.client.begin_operation().await.unwrap();
        let wrong = OperationId::new(active.id().get() + 1).unwrap();
        let result = test
            .client
            .call(|reply| CoreActorMessage::Run {
                operation: wrong,
                request: request(&test.requests, ClashCore::Mihomo),
                reply,
            })
            .await;
        assert!(matches!(result, Err(CoreActorError::StaleOperation)));
        assert_eq!(test.backend.run_calls(), 0);
    }

    #[tokio::test]
    async fn shutdown_drains_waiters_and_stops_backend() {
        let test = test_client(stopped(None)).await;
        let active = test.client.begin_operation().await.unwrap();
        let first_client = test.client.clone();
        let second_client = test.client.clone();
        let first = tokio::spawn(async move { first_client.begin_operation().await });
        let second = tokio::spawn(async move { second_client.begin_operation().await });
        tokio::task::yield_now().await;
        test.client.shutdown().await;
        assert!(matches!(
            first.await.unwrap(),
            Err(OperationError::ShuttingDown)
        ));
        assert!(matches!(
            second.await.unwrap(),
            Err(OperationError::ShuttingDown)
        ));
        assert_eq!(test.backend.shutdown_calls(), 1);
        drop(active);
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_times_out_and_releases_the_waiter() {
        let test = test_client(stopped(None)).await;
        let active = test.client.begin_operation().await.unwrap();
        let waiting_client = test.client.clone();
        let waiting = tokio::spawn(async move { waiting_client.begin_operation().await });
        tokio::task::yield_now().await;
        tokio::time::advance(CORE_ACQUIRE_TIMEOUT).await;
        assert!(matches!(
            waiting.await.unwrap(),
            Err(OperationError::AcquireTimeout)
        ));
        tokio::task::yield_now().await;
        active.release().await;
        let third = test.client.begin_operation().await.unwrap();
        third.release().await;
    }

    #[tokio::test]
    async fn dropping_a_waiting_begin_operation_allows_the_next_waiter() {
        let test = test_client(stopped(None)).await;
        let first = test.client.begin_operation().await.unwrap();
        let mut second = Box::pin(test.client.begin_operation());
        poll_once_pending(second.as_mut()).await;
        let mut third = Box::pin(test.client.begin_operation());
        poll_once_pending(third.as_mut()).await;
        test.client.refresh_status(&first).await.unwrap();

        drop(second);
        drop(first);
        let third = third.await.unwrap();
        third.release().await;
    }

    #[tokio::test]
    async fn dropping_a_just_granted_operation_guard_releases_the_next_waiter() {
        let test = test_client(stopped(None)).await;
        let first = test.client.begin_operation().await.unwrap();
        let mut second = Box::pin(test.client.begin_operation());
        poll_once_pending(second.as_mut()).await;
        let mut third = Box::pin(test.client.begin_operation());
        poll_once_pending(third.as_mut()).await;
        test.client.refresh_status(&first).await.unwrap();

        drop(first);
        let second = second.await.unwrap();
        drop(second);
        let third = third.await.unwrap();
        third.release().await;
    }

    #[tokio::test]
    async fn operation_and_refresh_both_update_revision() {
        let test = test_client(stopped(None)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.set_observation(running(1));
        test.client
            .run(&operation, &request(&test.requests, ClashCore::Mihomo))
            .await
            .unwrap();
        assert_eq!(test.client.status().revision.unwrap().generation, 1);
        test.backend.set_observation(running(2));
        test.client.refresh_status(&operation).await.unwrap();
        assert_eq!(test.client.status().revision.unwrap().generation, 2);
    }

    #[tokio::test]
    async fn guarded_and_hint_refreshes_obey_the_gate() {
        let test = test_client(stopped(None)).await;
        let baseline = test.backend.observe_calls();
        let operation = test.client.begin_operation().await.unwrap();
        let wrong = OperationId::new(operation.id().get() + 1).unwrap();
        let wrong_result = test
            .client
            .call(|reply| CoreActorMessage::RefreshStatus {
                operation: wrong,
                reply,
            })
            .await;
        assert!(matches!(wrong_result, Err(CoreActorError::StaleOperation)));
        assert_eq!(test.backend.observe_calls(), baseline);

        test.client.hint_refresh();
        tokio::task::yield_now().await;
        assert_eq!(test.backend.observe_calls(), baseline);

        test.backend.set_observation(running(3));
        test.client.refresh_status(&operation).await.unwrap();
        assert_eq!(test.client.status().revision.unwrap().generation, 3);
        operation.release().await;

        let mut status_rx = test.client.inner.status_rx.clone();
        status_rx.borrow_and_update();
        test.backend.set_observation(running(4));
        test.client.hint_refresh();
        status_rx.changed().await.unwrap();
        assert_eq!(test.client.status().revision.unwrap().generation, 4);
    }

    #[tokio::test]
    async fn operation_result_is_committed_before_reply() {
        let test = test_client(stopped(None)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.set_observation(running(5));
        test.client
            .run(&operation, &request(&test.requests, ClashCore::Mihomo))
            .await
            .unwrap();
        assert_eq!(test.client.status().revision.unwrap().generation, 5);
    }

    #[tokio::test]
    async fn failed_refresh_does_not_pollute_the_cache() {
        let test = test_client(running(6)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.fail_next_observe();
        assert!(test.client.refresh_status(&operation).await.is_err());
        assert_eq!(test.client.status().revision.unwrap().generation, 6);
    }

    #[tokio::test]
    async fn ui_hint_eventually_observes_exhaustion_once() {
        let test = test_client(running(1)).await;
        let mut status_rx = test.client.inner.status_rx.clone();
        status_rx.borrow_and_update();
        test.backend.set_observation(exhausted("diagnostic"));
        let before = test.client.status();
        assert!(!before.recovery_exhausted);
        test.client.hint_refresh();
        status_rx.changed().await.unwrap();
        assert!(test.client.status().recovery_exhausted);
        assert_eq!(test.sink.0.lock().unwrap().len(), 1);
        test.client.hint_refresh();
        status_rx.changed().await.unwrap();
        assert_eq!(test.sink.0.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn status_read_remains_live_while_run_is_blocked() {
        let test = test_client(stopped(None)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.set_observation(running(7));
        let (started, release) = test.backend.block_next_run();
        let client = test.client.clone();
        let request = request(&test.requests, ClashCore::Mihomo);
        let run = tokio::spawn(async move {
            let result = client.run(&operation, &request).await;
            drop(operation);
            result
        });
        started.await.unwrap();
        assert!(matches!(test.client.status().state, CoreState::Stopped(_)));
        release.send(()).unwrap();
        run.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn shutdown_publishes_a_distinct_terminal_snapshot() {
        let test = test_client(running(8)).await;
        assert!(matches!(test.client.status().state, CoreState::Running));
        let mut receiver = test.client.inner.status_rx.clone();
        test.client.shutdown().await;
        receiver.changed().await.unwrap();
        assert!(matches!(receiver.borrow().state, CoreState::Stopped(_)));
    }

    #[tokio::test]
    async fn external_apply_stays_stale_until_an_idle_hint() {
        let test = test_client(running(1)).await;
        test.backend.set_observation(running(9));
        assert_eq!(test.client.status().revision.unwrap().generation, 1);
        let mut receiver = test.client.inner.status_rx.clone();
        receiver.borrow_and_update();
        test.client.hint_refresh();
        receiver.changed().await.unwrap();
        assert_eq!(test.client.status().revision.unwrap().generation, 9);
    }

    #[tokio::test]
    async fn refresh_hints_are_coalesced() {
        let test = test_client(running(1)).await;
        let baseline = test.backend.observe_calls();
        for _ in 0..10 {
            test.client.hint_refresh();
        }
        for _ in 0..10 {
            if test.backend.observe_calls() > baseline {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(test.backend.observe_calls() - baseline <= 2);
    }

    #[tokio::test]
    async fn running_identity_is_actor_owned_not_typed_desired() {
        let test = test_client(stopped(None)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.set_observation(running(1));
        let request_b = request(&test.requests, ClashCore::ClashRs);
        test.client.run(&operation, &request_b).await.unwrap();
        let actual = test.client.running(&operation).await.unwrap().0.unwrap();
        assert_eq!(actual.core_type, CoreType::Clash(ClashCoreType::ClashRust));
    }

    #[test]
    fn initial_watch_snapshot_matches_legacy_empty_status() {
        let view = CoreStatusView::initial();
        assert!(matches!(view.state, CoreState::Stopped(None)));
        assert_eq!(view.state_changed_at, 0);
        assert_eq!(view.run_type, RunType::default());
        assert!(view.revision.is_none());
        assert!(!view.recovery_exhausted);
    }

    #[tokio::test]
    async fn recovery_exhaustion_is_published_once_even_after_recover() {
        let test = test_client(running(1)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.set_observation(exhausted("first"));
        test.client.refresh_status(&operation).await.unwrap();
        test.client.refresh_status(&operation).await.unwrap();
        test.client
            .call(|reply| CoreActorMessage::Recover {
                operation: operation.id(),
                reply,
            })
            .await
            .unwrap();
        assert_eq!(test.sink.0.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn active_lifecycle_resets_the_recovery_latch() {
        let test = test_client(running(1)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.set_observation(exhausted("first"));
        test.client.refresh_status(&operation).await.unwrap();
        test.backend.set_observation(running(2));
        test.client.refresh_status(&operation).await.unwrap();
        test.backend.set_observation(exhausted("second"));
        test.client.refresh_status(&operation).await.unwrap();
        assert_eq!(test.sink.0.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn latch_uses_faithful_lifecycle_before_lossy_projection() {
        let test = test_client(running(1)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.set_observation(exhausted("first"));
        test.client.refresh_status(&operation).await.unwrap();
        test.backend.set_observation(observation(
            CoreState::Stopped(None),
            FaithfulLifecycle::Starting,
            None,
        ));
        test.client.refresh_status(&operation).await.unwrap();
        test.backend.set_observation(exhausted("second"));
        test.client.refresh_status(&operation).await.unwrap();
        assert_eq!(test.sink.0.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn recovery_degradation_dto_is_bounded_and_complete() {
        let test = test_client(running(1)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.set_observation(exhausted(&"界".repeat(400)));
        test.client.refresh_status(&operation).await.unwrap();
        let degradations = test.sink.0.lock().unwrap();
        let degradation = &degradations[0];
        assert_eq!(degradation.phase, DegradationPhase::CoreLifecycle);
        assert_eq!(degradation.code, "core_recovery_exhausted");
        assert!(degradation.retryable);
        assert!(degradation.message.contains(RECOVERY_EXHAUSTED_PREFIX));
        assert!(degradation.message.len() <= 512);
        assert!(
            degradation
                .message
                .is_char_boundary(degradation.message.len())
        );
    }

    #[tokio::test]
    async fn set_backend_clears_running_identity() {
        let test = test_client(stopped(None)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.set_observation(running(1));
        test.client
            .run(&operation, &request(&test.requests, ClashCore::Mihomo))
            .await
            .unwrap();
        assert!(test.client.running(&operation).await.unwrap().0.is_some());
        let _ = test.client.set_backend(&operation, RunType::Service).await;
        assert!(test.client.running(&operation).await.unwrap().0.is_none());
    }

    #[tokio::test]
    async fn backend_switch_publishes_transient_stopped() {
        let (barrier, entered, release) = ReplaceBarrier::new();
        let test = test_client_with_replace_barrier(running(1), Some(barrier)).await;
        let operation = test.client.begin_operation().await.unwrap();
        let mut receiver = test.client.inner.status_rx.clone();
        receiver.borrow_and_update();
        let client = test.client.clone();
        let switching = tokio::spawn(async move {
            let result = client.set_backend(&operation, RunType::Normal).await;
            (result, operation)
        });
        entered.await.unwrap();
        receiver.changed().await.unwrap();
        assert!(matches!(receiver.borrow().state, CoreState::Stopped(None)));
        release.send(()).unwrap();
        let (result, operation) = switching.await.unwrap();
        result.unwrap();
        operation.release().await;
        test.client.shutdown().await;
    }

    #[tokio::test]
    async fn failed_backend_slot_is_queryable_and_recoverable() {
        let test = test_client(stopped(None)).await;
        let holding = LocalBackend::new(&test.requests).await.unwrap();
        let operation = test.client.begin_operation().await.unwrap();
        let first_error = match test.client.set_backend(&operation, RunType::Normal).await {
            Err(CoreActorError::NoBackend { last_error }) => last_error,
            other => panic!("expected failed backend slot, got {other:?}"),
        };
        assert!(matches!(
            test.client.status().state,
            CoreState::Stopped(Some(_))
        ));
        let queried_error = match test.client.running(&operation).await {
            Err(CoreActorError::NoBackend { last_error }) => last_error,
            _ => panic!("expected failed backend query"),
        };
        assert!(Arc::ptr_eq(&first_error, &queried_error));

        CoreBackend::Local(holding).shutdown().await.unwrap();
        test.client
            .set_backend(&operation, RunType::Normal)
            .await
            .unwrap();
        assert!(test.client.running(&operation).await.unwrap().0.is_none());
        operation.release().await;
        test.client.shutdown().await;
    }

    #[tokio::test]
    async fn failed_replacement_preserves_run_type_until_a_real_backend_commits() {
        let test = test_client_with_scripted_replacement(running(1)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.fail_next_replace();

        assert!(
            test.client
                .set_backend(&operation, RunType::Service)
                .await
                .is_err()
        );
        assert_eq!(test.client.status().run_type, RunType::Normal);

        test.client
            .set_backend(&operation, RunType::Service)
            .await
            .unwrap();
        assert_eq!(test.client.status().run_type, RunType::Service);
    }

    #[tokio::test]
    async fn terminal_observation_clears_running_identity() {
        let test = test_client(stopped(None)).await;
        let operation = test.client.begin_operation().await.unwrap();
        test.backend.set_observation(running(1));
        test.client
            .run(&operation, &request(&test.requests, ClashCore::Mihomo))
            .await
            .unwrap();
        test.backend
            .set_observation(stopped(Some("crashed".to_owned())));
        test.client.refresh_status(&operation).await.unwrap();
        assert!(test.client.running(&operation).await.unwrap().0.is_none());
    }

    #[tokio::test]
    async fn running_identity_without_the_active_guard_is_rejected() {
        let test = test_client(running(1)).await;
        let operation = test.client.begin_operation().await.unwrap();
        let wrong = OperationId::new(operation.id().get() + 1).unwrap();
        let result = test
            .client
            .call(|reply| CoreActorMessage::RunningIdentity {
                operation: wrong,
                reply,
            })
            .await;
        assert!(matches!(result, Err(CoreActorError::StaleOperation)));
    }

    #[tokio::test]
    async fn promoted_restart_uses_the_transaction_target_core() {
        let test = test_client(stopped(None)).await;
        let application =
            crate::client::tests::test_application_client(&test._dir, ClashCore::Mihomo).await;
        let adapter =
            CoreLifecycleAdapter::new(test.client.clone(), application, test.requests.clone());
        let candidate = test
            .requests
            .runtime_paths()
            .create_candidate(b"mode: rule\n")
            .await
            .unwrap();
        let mut lease = adapter.begin().await.unwrap();
        lease
            .check_and_promote(
                &candidate,
                ClashCore::ClashRs,
                test.requests.runtime_paths().product(),
            )
            .await
            .unwrap();
        test.backend.set_observation(running(1));
        lease.restart().await.unwrap();
        drop(lease);

        assert_eq!(
            running_request(&test.client).await.core_type,
            CoreType::Clash(ClashCoreType::ClashRust)
        );
    }

    #[tokio::test]
    async fn restart_consumes_transaction_target_once_then_uses_typed_core() {
        let test = test_client(stopped(None)).await;
        let application =
            crate::client::tests::test_application_client(&test._dir, ClashCore::Mihomo).await;
        let adapter =
            CoreLifecycleAdapter::new(test.client.clone(), application, test.requests.clone());
        let candidate = test
            .requests
            .runtime_paths()
            .create_candidate(b"mode: rule\n")
            .await
            .unwrap();
        let mut lease = adapter.begin().await.unwrap();
        lease
            .check_and_promote(
                &candidate,
                ClashCore::ClashRs,
                test.requests.runtime_paths().product(),
            )
            .await
            .unwrap();
        lease.restart().await.unwrap();
        lease.restart().await.unwrap();
        drop(lease);

        let requests = test.backend.run_requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].core_type,
            CoreType::Clash(ClashCoreType::ClashRust)
        );
        assert_eq!(
            requests[1].core_type,
            CoreType::Clash(ClashCoreType::Mihomo)
        );
    }

    #[tokio::test]
    async fn pure_restart_uses_the_typed_snapshot_core() {
        let test = test_client(stopped(None)).await;
        let application =
            crate::client::tests::test_application_client(&test._dir, ClashCore::Meow).await;
        let adapter =
            CoreLifecycleAdapter::new(test.client.clone(), application, test.requests.clone());
        let mut lease = adapter.begin().await.unwrap();
        test.backend.set_observation(running(3));
        lease.restart().await.unwrap();
        drop(lease);

        assert_eq!(
            running_request(&test.client).await.core_type,
            CoreType::Clash(ClashCoreType::Meow)
        );
    }
}
