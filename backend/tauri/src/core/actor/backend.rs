use std::{borrow::Cow, sync::Arc};

use async_trait::async_trait;
use nyanpasu_core_manager::{
    ApplyOutcome, CoreManager, CoreSpec, CoreState as ManagerCoreState, InstanceOptions,
    InstanceSpec, LocalIpcPolicy, ManagerOptions, RevisionId,
};
use nyanpasu_ipc::{
    SERVICE_PLACEHOLDER,
    api::{
        core::{
            apply::{ApplyOutcomeKind, CoreApplyData, CoreApplyReq},
            check::CoreCheckReq,
            start::CoreStartReq,
        },
        error_kind,
        status::{ConfigRevisionInfo, CoreInfos, CoreState, CoreStateDetail, RevisionIdInfo},
    },
    client::{Client, ClientError},
};
use nyanpasu_utils::core::{ClashCoreType, CoreType};
use tokio::sync::watch;

use super::{
    error_kind::service_error_kind,
    request::{CoreModeReconciler, CoreRequestFactory},
    types::{
        BackendObservation, CoreRequest, CoreStatusView, FaithfulLifecycle, is_recovery_exhausted,
    },
};

pub(crate) struct LocalBackend {
    pub(crate) manager: Arc<CoreManager>,
    status: watch::Receiver<nyanpasu_core_manager::CoreStatus>,
}

pub(crate) struct ServiceBackend {
    pub(crate) client: Client,
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct TestBackend {
    state: Arc<TestBackendState>,
}

#[cfg(test)]
struct TestBackendState {
    observation: std::sync::Mutex<BackendObservation>,
    observe_calls: std::sync::atomic::AtomicUsize,
    check_calls: std::sync::atomic::AtomicUsize,
    apply_calls: std::sync::atomic::AtomicUsize,
    apply_results:
        std::sync::Mutex<std::collections::VecDeque<Result<CoreApplyData, CoreBackendError>>>,
    run_calls: std::sync::atomic::AtomicUsize,
    run_requests: std::sync::Mutex<Vec<CoreRequest>>,
    shutdown_calls: std::sync::atomic::AtomicUsize,
    fail_observe: std::sync::atomic::AtomicBool,
    fail_run: std::sync::atomic::AtomicBool,
    fail_replace: std::sync::atomic::AtomicBool,
    run_barrier: std::sync::Mutex<Option<TestRunBarrier>>,
}

#[cfg(test)]
struct TestRunBarrier {
    started: tokio::sync::oneshot::Sender<()>,
    release: tokio::sync::oneshot::Receiver<()>,
}

pub(crate) enum CoreBackend {
    Local(LocalBackend),
    Service(ServiceBackend),
    #[cfg(test)]
    Test(TestBackend),
}

pub(crate) enum BackendSlot {
    Ready(CoreBackend),
    Failed { error: Arc<CoreBackendError> },
}

impl LocalBackend {
    pub(crate) async fn new(requests: &CoreRequestFactory) -> Result<Self, CoreBackendError> {
        let manager = Arc::new(
            CoreManager::new(local_manager_options(requests.manager_runtime_dir()))
                .await
                .map_err(|error| CoreBackendError::Construct(anyhow::Error::new(error)))?,
        );
        let status = manager.subscribe();
        Ok(Self { manager, status })
    }
}

impl ServiceBackend {
    pub(crate) fn new() -> Result<Self, CoreBackendError> {
        Self::with_placeholder(SERVICE_PLACEHOLDER)
    }

    pub(crate) fn with_placeholder(placeholder: &str) -> Result<Self, CoreBackendError> {
        let client = Client::new(placeholder)
            .map_err(|error| CoreBackendError::Construct(anyhow::Error::new(error)))?;
        Ok(Self { client })
    }
}

#[cfg(test)]
impl TestBackend {
    pub(crate) fn new(observation: BackendObservation) -> Self {
        Self {
            state: Arc::new(TestBackendState {
                observation: std::sync::Mutex::new(observation),
                observe_calls: std::sync::atomic::AtomicUsize::new(0),
                check_calls: std::sync::atomic::AtomicUsize::new(0),
                apply_calls: std::sync::atomic::AtomicUsize::new(0),
                apply_results: std::sync::Mutex::new(std::collections::VecDeque::new()),
                run_calls: std::sync::atomic::AtomicUsize::new(0),
                run_requests: std::sync::Mutex::new(Vec::new()),
                shutdown_calls: std::sync::atomic::AtomicUsize::new(0),
                fail_observe: std::sync::atomic::AtomicBool::new(false),
                fail_run: std::sync::atomic::AtomicBool::new(false),
                fail_replace: std::sync::atomic::AtomicBool::new(false),
                run_barrier: std::sync::Mutex::new(None),
            }),
        }
    }

    pub(crate) fn set_observation(&self, observation: BackendObservation) {
        *self.state.observation.lock().unwrap() = observation;
    }

    pub(crate) fn push_apply_result(&self, result: Result<CoreApplyData, CoreBackendError>) {
        self.state.apply_results.lock().unwrap().push_back(result);
    }

    pub(crate) fn apply_calls(&self) -> usize {
        self.state
            .apply_calls
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn fail_next_observe(&self) {
        self.state
            .fail_observe
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub(crate) fn fail_next_run(&self) {
        self.state
            .fail_run
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub(crate) fn fail_next_replace(&self) {
        self.state
            .fail_replace
            .store(true, std::sync::atomic::Ordering::Release);
    }

    pub(crate) fn take_replace_failure(&self) -> bool {
        self.state
            .fail_replace
            .swap(false, std::sync::atomic::Ordering::AcqRel)
    }

    pub(crate) fn block_next_run(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        *self.state.run_barrier.lock().unwrap() = Some(TestRunBarrier {
            started: started_tx,
            release: release_rx,
        });
        (started_rx, release_tx)
    }

    pub(crate) fn observe_calls(&self) -> usize {
        self.state
            .observe_calls
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn run_calls(&self) -> usize {
        self.state
            .run_calls
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn check_calls(&self) -> usize {
        self.state
            .check_calls
            .load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn run_requests(&self) -> Vec<CoreRequest> {
        self.state.run_requests.lock().unwrap().clone()
    }

    pub(crate) fn shutdown_calls(&self) -> usize {
        self.state
            .shutdown_calls
            .load(std::sync::atomic::Ordering::Acquire)
    }

    async fn wait_for_run_barrier(&self) {
        let barrier = self.state.run_barrier.lock().unwrap().take();
        if let Some(barrier) = barrier {
            let _ = barrier.started.send(());
            let _ = barrier.release.await;
        }
    }

    fn observation(&self) -> Result<BackendObservation, CoreBackendError> {
        self.state
            .observe_calls
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if self
            .state
            .fail_observe
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            return Err(CoreBackendError::Construct(anyhow::anyhow!(
                "scripted status failure"
            )));
        }
        Ok(self.state.observation.lock().unwrap().clone())
    }
}

impl CoreBackend {
    pub(crate) async fn new(
        mode: crate::core::RunType,
        requests: &CoreRequestFactory,
    ) -> Result<Self, CoreBackendError> {
        match mode {
            crate::core::RunType::Service => Ok(Self::Service(ServiceBackend::new()?)),
            crate::core::RunType::Normal | crate::core::RunType::Elevated => {
                Ok(Self::Local(LocalBackend::new(requests).await?))
            }
        }
    }

    pub(crate) async fn check(&self, request: &CoreRequest) -> Result<(), CoreBackendError> {
        match self {
            Self::Local(local) => local.manager.check_config(&instance_spec(request)?).await?,
            Self::Service(service) => {
                service
                    .client
                    .check_config(&CoreCheckReq {
                        core_type: Cow::Borrowed(&request.core_type),
                        config_file: Cow::Owned(request.config_path.as_std_path().to_path_buf()),
                    })
                    .await?;
            }
            #[cfg(test)]
            Self::Test(test) => {
                test.state
                    .check_calls
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
        }
        Ok(())
    }

    pub(crate) async fn run(
        &self,
        request: &CoreRequest,
    ) -> Result<BackendObservation, CoreBackendError> {
        match self {
            Self::Local(local) => {
                local.manager.switch(instance_spec(request)?).await?;
            }
            Self::Service(service) => {
                let status = service.client.status().await?;
                if service_needs_stop(&status.core_infos)
                    && let Err(error) = service.client.stop_core().await
                    && service_error_kind(&error) != Some(error_kind::NOT_STARTED)
                {
                    return Err(error.into());
                }
                service
                    .client
                    .start_core(&CoreStartReq {
                        core_type: Cow::Borrowed(&request.core_type),
                        config_file: Cow::Owned(request.config_path.as_std_path().to_path_buf()),
                    })
                    .await?;
            }
            #[cfg(test)]
            Self::Test(test) => {
                test.state
                    .run_calls
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                test.state
                    .run_requests
                    .lock()
                    .unwrap()
                    .push(request.clone());
                if test
                    .state
                    .fail_run
                    .swap(false, std::sync::atomic::Ordering::AcqRel)
                {
                    return Err(CoreBackendError::Construct(anyhow::anyhow!(
                        "scripted run failure"
                    )));
                }
                test.wait_for_run_barrier().await;
            }
        }
        self.observe_status().await
    }

    pub(crate) async fn stop(&self) -> Result<BackendObservation, CoreBackendError> {
        match self {
            Self::Local(local) => local.manager.stop().await?,
            Self::Service(service) => service.client.stop_core().await?,
            #[cfg(test)]
            Self::Test(_) => {}
        }
        self.observe_status().await
    }

    pub(crate) async fn recover(&self) -> Result<BackendObservation, CoreBackendError> {
        match self {
            Self::Local(local) => local.manager.recover_quarantine().await?,
            Self::Service(service) => service.client.recover_core().await?,
            #[cfg(test)]
            Self::Test(_) => {}
        }
        self.observe_status().await
    }

    pub(crate) async fn observe_status(&self) -> Result<BackendObservation, CoreBackendError> {
        match self {
            Self::Local(local) => Ok(map_local_status(&local.status.borrow().clone())),
            Self::Service(service) => {
                let status = service.client.status().await?;
                Ok(map_service_status(&status.core_infos))
            }
            #[cfg(test)]
            Self::Test(test) => test.observation(),
        }
    }

    pub(crate) async fn apply(
        &self,
        request: &CoreRequest,
        expected: Option<RevisionIdInfo>,
    ) -> Result<CoreApplyData, CoreBackendError> {
        match self {
            Self::Local(local) => {
                let outcome = local
                    .manager
                    .apply_config(
                        instance_spec(request)?,
                        expected.as_ref().map(map_revision_id),
                    )
                    .await?;
                Ok(map_apply_outcome(&outcome))
            }
            Self::Service(service) => Ok(service
                .client
                .apply_config(&CoreApplyReq {
                    core_type: Cow::Borrowed(&request.core_type),
                    config_file: Cow::Owned(request.config_path.as_std_path().to_path_buf()),
                    expected_revision: expected,
                })
                .await?),
            #[cfg(test)]
            Self::Test(test) => {
                test.state
                    .apply_calls
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
                if let Some(result) = test.state.apply_results.lock().unwrap().pop_front() {
                    result
                } else {
                    let revision = test
                        .state
                        .observation
                        .lock()
                        .unwrap()
                        .view
                        .revision
                        .clone()
                        .unwrap_or(ConfigRevisionInfo {
                            epoch: 0,
                            generation: 0,
                            source_hash: String::new(),
                            effective_hash: String::new(),
                        });
                    Ok(CoreApplyData {
                        outcome: ApplyOutcomeKind::Noop,
                        revision,
                        warning: None,
                        failed_apply: None,
                    })
                }
            }
        }
    }

    pub(crate) async fn shutdown(self) -> Result<(), CoreBackendError> {
        match self {
            Self::Local(local) => local.manager.shutdown().await?,
            Self::Service(service) => {
                if let Err(error) = service.client.stop_core().await
                    && service_error_kind(&error) != Some(error_kind::NOT_STARTED)
                {
                    return Err(error.into());
                }
            }
            #[cfg(test)]
            Self::Test(test) => {
                test.state
                    .shutdown_calls
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
        }
        Ok(())
    }
}

fn local_manager_options(runtime_dir: camino::Utf8PathBuf) -> ManagerOptions {
    ManagerOptions {
        runtime_dir: Some(runtime_dir),
        // The upstream default is already Disable; spelling it out keeps this safety gate auditable here.
        local_ipc_policy: LocalIpcPolicy::Disable,
        ..ManagerOptions::default()
    }
}

fn instance_spec(request: &CoreRequest) -> Result<InstanceSpec, CoreBackendError> {
    Ok(InstanceSpec {
        core: CoreSpec {
            kind: core_kind(&request.core_type)?,
            binary_path: request.binary_path.clone(),
            version: None,
            features: Vec::new(),
        },
        config_path: request.config_path.clone(),
        working_dir: request.working_dir.clone(),
        pid_file: request.pid_path.clone(),
        options: InstanceOptions::default(),
    })
}

fn core_kind(
    core_type: &CoreType,
) -> Result<nyanpasu_core_metadata::ClashCoreKind, CoreBackendError> {
    match core_type {
        CoreType::Clash(ClashCoreType::Mihomo | ClashCoreType::MihomoAlpha) => {
            Ok(nyanpasu_core_metadata::ClashCoreKind::Mihomo)
        }
        CoreType::Clash(ClashCoreType::ClashRust | ClashCoreType::ClashRustAlpha) => {
            Ok(nyanpasu_core_metadata::ClashCoreKind::ClashRust)
        }
        CoreType::Clash(ClashCoreType::ClashPremium) => {
            Ok(nyanpasu_core_metadata::ClashCoreKind::ClashPremium)
        }
        CoreType::Clash(ClashCoreType::Meow) => Ok(nyanpasu_core_metadata::ClashCoreKind::Meow),
        CoreType::SingBox => Err(CoreBackendError::Binary(anyhow::anyhow!(
            "sing-box is not a supported core"
        ))),
    }
}

fn map_local_status(status: &nyanpasu_core_manager::CoreStatus) -> BackendObservation {
    let lifecycle = match &status.state {
        ManagerCoreState::Stopped { reason } => FaithfulLifecycle::Stopped {
            reason: reason.as_ref().map(ToString::to_string),
        },
        ManagerCoreState::Starting { .. } => FaithfulLifecycle::Starting,
        ManagerCoreState::Running { .. } => FaithfulLifecycle::Running,
        ManagerCoreState::Restarting { .. } => FaithfulLifecycle::Restarting,
        ManagerCoreState::Switching { .. } => FaithfulLifecycle::Switching,
        ManagerCoreState::Stopping { .. } => FaithfulLifecycle::Stopping,
        _ => FaithfulLifecycle::Stopped { reason: None },
    };
    let state = match &status.state {
        ManagerCoreState::Running { .. }
        | ManagerCoreState::Switching { .. }
        | ManagerCoreState::Stopping { .. } => CoreState::Running,
        ManagerCoreState::Stopped { reason } => {
            CoreState::Stopped(reason.as_ref().map(ToString::to_string))
        }
        _ => CoreState::Stopped(None),
    };
    observation(
        state,
        status.changed_at,
        status.revision.as_ref().map(map_revision),
        lifecycle,
        crate::core::RunType::Normal,
    )
}

fn map_service_status(infos: &CoreInfos) -> BackendObservation {
    let lifecycle = match infos.detail.as_ref() {
        Some(CoreStateDetail::Stopped { reason }) => FaithfulLifecycle::Stopped {
            reason: reason.clone(),
        },
        Some(CoreStateDetail::Starting { .. }) => FaithfulLifecycle::Starting,
        Some(CoreStateDetail::Running { .. }) => FaithfulLifecycle::Running,
        Some(CoreStateDetail::Restarting { .. }) => FaithfulLifecycle::Restarting,
        Some(CoreStateDetail::Switching { .. }) => FaithfulLifecycle::Switching,
        Some(CoreStateDetail::Stopping { .. }) => FaithfulLifecycle::Stopping,
        None => match &infos.state {
            CoreState::Running => FaithfulLifecycle::Running,
            CoreState::Stopped(reason) => FaithfulLifecycle::Stopped {
                reason: reason.clone(),
            },
        },
    };
    observation(
        infos.state.clone(),
        infos.state_changed_at,
        infos.revision.clone(),
        lifecycle,
        crate::core::RunType::Service,
    )
}

fn observation(
    state: CoreState,
    state_changed_at: i64,
    revision: Option<ConfigRevisionInfo>,
    lifecycle: FaithfulLifecycle,
    run_type: crate::core::RunType,
) -> BackendObservation {
    let recovery_exhausted = match &lifecycle {
        FaithfulLifecycle::Stopped {
            reason: Some(reason),
        } => is_recovery_exhausted(reason),
        _ => false,
    };
    BackendObservation {
        view: CoreStatusView {
            state,
            state_changed_at,
            run_type,
            revision,
            recovery_exhausted,
        },
        lifecycle,
    }
}

fn map_revision(revision: &nyanpasu_core_manager::ConfigRevision) -> ConfigRevisionInfo {
    ConfigRevisionInfo {
        epoch: revision.epoch,
        generation: revision.generation,
        source_hash: revision.source_hash.clone(),
        effective_hash: revision.effective_hash.clone(),
    }
}

fn map_revision_id(info: &RevisionIdInfo) -> RevisionId {
    RevisionId {
        epoch: info.epoch,
        generation: info.generation,
        effective_hash: info.effective_hash.clone(),
    }
}

fn map_apply_outcome(outcome: &ApplyOutcome) -> CoreApplyData {
    let mut warnings = Vec::new();
    let mut current = outcome;
    while let ApplyOutcome::DurabilityUncertain { outcome, warning } = current {
        warnings.push(warning.clone());
        current = outcome;
    }
    let (outcome, revision, failed_apply) = match current {
        ApplyOutcome::Noop { revision } => (ApplyOutcomeKind::Noop, revision, None),
        ApplyOutcome::Patched { revision } => (ApplyOutcomeKind::Patched, revision, None),
        ApplyOutcome::Reloaded { revision } => (ApplyOutcomeKind::Reloaded, revision, None),
        ApplyOutcome::Restarted { revision } => (ApplyOutcomeKind::Restarted, revision, None),
        ApplyOutcome::Switched { revision } => (ApplyOutcomeKind::Switched, revision, None),
        ApplyOutcome::RolledBack {
            revision,
            failed_apply,
        } => (
            ApplyOutcomeKind::RolledBack,
            revision,
            Some(failed_apply.clone()),
        ),
        ApplyOutcome::DurabilityUncertain { .. } => unreachable!("unwrapped above"),
    };
    CoreApplyData {
        outcome,
        revision: map_revision(revision),
        warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
        failed_apply,
    }
}

pub(super) fn service_needs_stop(infos: &CoreInfos) -> bool {
    match infos.detail.as_ref() {
        Some(CoreStateDetail::Stopped { .. }) => false,
        Some(
            CoreStateDetail::Starting { .. }
            | CoreStateDetail::Running { .. }
            | CoreStateDetail::Restarting { .. }
            | CoreStateDetail::Switching { .. }
            | CoreStateDetail::Stopping { .. },
        )
        | None => true,
    }
}

pub trait CoreDegradationSink: Send + Sync + 'static {
    fn publish(&self, degradation: crate::client::runtime::Degradation);
}

#[async_trait]
pub(crate) trait ServiceControlOps: Send + Sync + 'static {
    async fn install(&self, reconciler: CoreModeReconciler) -> anyhow::Result<()>;
    async fn start(&self, reconciler: CoreModeReconciler) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
    async fn restart(&self, reconciler: CoreModeReconciler) -> anyhow::Result<()>;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CoreBackendError {
    #[error("local core manager: {0}")]
    Local(#[from] nyanpasu_core_manager::Error),
    #[error("service ipc: {0}")]
    Service(#[from] ClientError),
    #[error("core binary: {0}")]
    Binary(#[source] anyhow::Error),
    #[error("backend construction: {0}")]
    Construct(#[source] anyhow::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackendFailureClass {
    RevisionConflict,
    NotRunning,
    TransportLost,
    Other,
}

pub(crate) fn classify_apply_backend_failure(error: &CoreBackendError) -> BackendFailureClass {
    match error {
        CoreBackendError::Local(nyanpasu_core_manager::Error::RevisionConflict { .. }) => {
            BackendFailureClass::RevisionConflict
        }
        CoreBackendError::Local(nyanpasu_core_manager::Error::NotStarted) => {
            BackendFailureClass::NotRunning
        }
        CoreBackendError::Local(_) => BackendFailureClass::Other,
        CoreBackendError::Service(ClientError::Server {
            error_kind: Some(kind),
            ..
        }) if kind == error_kind::REVISION_CONFLICT => BackendFailureClass::RevisionConflict,
        CoreBackendError::Service(ClientError::Server {
            error_kind: Some(kind),
            ..
        }) if kind == error_kind::NOT_STARTED => BackendFailureClass::NotRunning,
        CoreBackendError::Service(
            ClientError::BuildClient(_)
            | ClientError::Request { .. }
            | ClientError::WebSocket { .. }
            | ClientError::HttpStatus { .. },
        ) => BackendFailureClass::TransportLost,
        CoreBackendError::Service(_)
        | CoreBackendError::Binary(_)
        | CoreBackendError::Construct(_) => BackendFailureClass::Other,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        collections::VecDeque,
        path::PathBuf,
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
        },
    };

    use axum::{
        Json, Router,
        extract::State,
        http::StatusCode,
        routing::{get, post},
    };
    use camino::Utf8PathBuf;
    use interprocess::local_socket::{
        GenericFilePath, ListenerNonblockingMode, ListenerOptions, ToFsName,
        tokio::{Listener, Stream as IpcStream, prelude::*},
    };
    use nyanpasu_config::application::ClashCore;
    use nyanpasu_core_manager::{ApplyOutcome, ConfigRevision, Error as ManagerError, StopReason};
    use nyanpasu_ipc::api::{
        RBuilder,
        core::{
            apply::{CORE_APPLY_ENDPOINT, CoreApplyReq, CoreApplyRes},
            check::{CORE_CHECK_ENDPOINT, CoreCheckReq, CoreCheckRes},
            recover::{CORE_RECOVER_ENDPOINT, CoreRecoverRes},
            start::{CORE_START_ENDPOINT, CoreStartReq, CoreStartRes},
            stop::{CORE_STOP_ENDPOINT, CoreStopRes},
        },
        status::{
            ConfigRevisionInfo, CoreInfos, CoreState, CoreStateDetail, RuntimeInfos,
            STATUS_ENDPOINT, StatusRes, StatusResBody,
        },
    };
    use tempfile::TempDir;

    use super::*;
    use crate::{
        client::RuntimePaths,
        core::actor::{
            error_kind::local_error_kind,
            request::{CoreBinaryResolver, CoreRequestFactory},
            types::RECOVERY_EXHAUSTED_PREFIX,
        },
        utils::path::PathResolver,
    };

    #[derive(Clone)]
    struct FixedBinary(Utf8PathBuf);

    impl CoreBinaryResolver for FixedBinary {
        fn resolve(&self, _kind: &CoreType) -> anyhow::Result<Utf8PathBuf> {
            Ok(self.0.clone())
        }
    }

    fn test_factory(dir: &TempDir, binary: Utf8PathBuf) -> CoreRequestFactory {
        let config = dir.path().join("config");
        let data = dir.path().join("data");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let paths = PathResolver::with_base_dirs(config.clone(), data);
        let runtime = Utf8PathBuf::from_path_buf(config.join("runtime")).unwrap();
        CoreRequestFactory::new(
            &paths,
            RuntimePaths::new(runtime.join("config.yaml"), runtime.join(".candidates")),
            Arc::new(FixedBinary(binary)),
        )
        .unwrap()
    }

    fn revision(generation: u64) -> ConfigRevision {
        ConfigRevision {
            epoch: 3,
            generation,
            source_hash: format!("source-{generation}"),
            effective_hash: format!("effective-{generation}"),
            runtime_path: Utf8PathBuf::from(format!("runtime-{generation}.yaml")),
        }
    }

    fn free_port() -> u16 {
        std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn write_config(request: &CoreRequest, port: u16) {
        std::fs::create_dir_all(request.config_path.parent().unwrap()).unwrap();
        std::fs::write(
            &request.config_path,
            format!(
                "external-controller: 127.0.0.1:{port}\nproxies: []\nproxy-groups: []\nrules: []\n"
            ),
        )
        .unwrap();
    }

    fn stopped_infos() -> CoreInfos {
        CoreInfos {
            r#type: None,
            state: CoreState::Stopped(None),
            state_changed_at: 1,
            config_path: None,
            controller: None,
            health: None,
            revision: None,
            detail: Some(CoreStateDetail::Stopped { reason: None }),
        }
    }

    #[derive(Clone)]
    struct Harness(Arc<Mutex<HarnessState>>);

    struct HarnessState {
        infos: CoreInfos,
        calls: VecDeque<&'static str>,
        stop_error_kind: Option<&'static str>,
        generation: u64,
        apply_data: CoreApplyData,
    }

    impl Harness {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(HarnessState {
                infos: stopped_infos(),
                calls: VecDeque::new(),
                stop_error_kind: None,
                generation: 0,
                apply_data: map_apply_outcome(&ApplyOutcome::Noop {
                    revision: revision(0),
                }),
            })))
        }

        fn running(&self) {
            let mut state = self.0.lock().unwrap();
            state.infos.state = CoreState::Running;
            state.infos.detail = Some(CoreStateDetail::Running { epoch: 1, pid: 7 });
        }

        fn set_stop_error(&self, kind: &'static str) {
            self.0.lock().unwrap().stop_error_kind = Some(kind);
        }

        fn set_revision(&self, generation: u64) {
            let mut state = self.0.lock().unwrap();
            state.generation = generation;
            state.infos.revision = Some(ConfigRevisionInfo {
                epoch: 1,
                generation,
                source_hash: format!("source-{generation}"),
                effective_hash: format!("effective-{generation}"),
            });
        }

        fn set_apply_data(&self, data: CoreApplyData) {
            self.0.lock().unwrap().apply_data = data;
        }

        fn take_calls(&self) -> Vec<&'static str> {
            self.0.lock().unwrap().calls.drain(..).collect()
        }
    }

    async fn status_handler(State(harness): State<Harness>) -> Json<StatusRes<'static>> {
        let mut state = harness.0.lock().unwrap();
        state.calls.push_back("status");
        Json(RBuilder::success(StatusResBody {
            version: Cow::Borrowed("test"),
            core_infos: state.infos.clone(),
            runtime_infos: RuntimeInfos {
                service_data_dir: Cow::Owned(PathBuf::from("service-data")),
                service_config_dir: Cow::Owned(PathBuf::from("service-config")),
                nyanpasu_config_dir: Cow::Owned(PathBuf::from("config")),
                nyanpasu_data_dir: Cow::Owned(PathBuf::from("data")),
            },
            logs: None,
        }))
    }

    async fn check_handler(
        State(harness): State<Harness>,
        Json(_request): Json<CoreCheckReq<'static>>,
    ) -> Json<CoreCheckRes<'static>> {
        harness.0.lock().unwrap().calls.push_back("check");
        Json(RBuilder::success(()))
    }

    async fn start_handler(
        State(harness): State<Harness>,
        Json(request): Json<CoreStartReq<'static>>,
    ) -> Json<CoreStartRes<'static>> {
        let mut state = harness.0.lock().unwrap();
        state.calls.push_back("start");
        state.generation += 1;
        let generation = state.generation;
        state.infos.r#type = Some(request.core_type.into_owned());
        state.infos.state = CoreState::Running;
        state.infos.detail = Some(CoreStateDetail::Running { epoch: 1, pid: 100 });
        state.infos.revision = Some(ConfigRevisionInfo {
            epoch: 1,
            generation,
            source_hash: "source".to_owned(),
            effective_hash: "effective".to_owned(),
        });
        Json(RBuilder::success(()))
    }

    async fn stop_handler(
        State(harness): State<Harness>,
    ) -> (StatusCode, Json<CoreStopRes<'static>>) {
        let mut state = harness.0.lock().unwrap();
        state.calls.push_back("stop");
        if let Some(kind) = state.stop_error_kind.take() {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RBuilder::other_error_with_kind(
                    Cow::Borrowed("scripted stop error"),
                    Some(Cow::Borrowed(kind)),
                )),
            );
        }
        state.infos.state = CoreState::Stopped(None);
        state.infos.detail = Some(CoreStateDetail::Stopped { reason: None });
        (StatusCode::OK, Json(RBuilder::success(())))
    }

    async fn recover_handler(State(harness): State<Harness>) -> Json<CoreRecoverRes<'static>> {
        harness.0.lock().unwrap().calls.push_back("recover");
        Json(RBuilder::success(()))
    }

    async fn apply_handler(
        State(harness): State<Harness>,
        Json(_request): Json<CoreApplyReq<'static>>,
    ) -> Json<CoreApplyRes<'static>> {
        let mut state = harness.0.lock().unwrap();
        state.calls.push_back("apply");
        Json(RBuilder::success(state.apply_data.clone()))
    }

    struct IpcListener(Listener, String);

    impl axum::serve::Listener for IpcListener {
        type Io = IpcStream;
        type Addr = String;

        async fn accept(&mut self) -> (Self::Io, Self::Addr) {
            loop {
                if let Ok(stream) = self.0.accept().await {
                    return (stream, self.1.clone());
                }
                tokio::task::yield_now().await;
            }
        }

        fn local_addr(&self) -> tokio::io::Result<Self::Addr> {
            Ok(self.1.clone())
        }
    }

    fn socket_path(placeholder: &str) -> String {
        if cfg!(windows) {
            format!("\\\\.\\pipe\\{placeholder}")
        } else {
            format!("/var/run/{placeholder}.sock")
        }
    }

    #[cfg(windows)]
    fn transport_available() -> bool {
        true
    }

    #[cfg(unix)]
    fn transport_available() -> bool {
        static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            let probe = format!("/var/run/.nyanpasu-ipc-probe-{}", std::process::id());
            match std::fs::File::create(&probe) {
                Ok(_) => {
                    let _ = std::fs::remove_file(&probe);
                    true
                }
                Err(error) => {
                    eprintln!(
                        "skipping core actor unix socket tests: /var/run is not writable \
                         ({error}); run as root for unix socket coverage"
                    );
                    false
                }
            }
        })
    }

    fn spawn_harness(harness: Harness) -> (String, tokio::sync::oneshot::Sender<()>) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let placeholder = format!(
            "nyanpasu_core_actor_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = socket_path(&placeholder);
        #[cfg(unix)]
        let _ = std::fs::remove_file(&path);
        let name = path.as_str().to_fs_name::<GenericFilePath>().unwrap();
        let listener = ListenerOptions::new()
            .name(name)
            .nonblocking(ListenerNonblockingMode::Both)
            .create_tokio()
            .unwrap();
        let router = Router::new()
            .route(STATUS_ENDPOINT, get(status_handler))
            .route(CORE_CHECK_ENDPOINT, post(check_handler))
            .route(CORE_START_ENDPOINT, post(start_handler))
            .route(CORE_STOP_ENDPOINT, post(stop_handler))
            .route(CORE_RECOVER_ENDPOINT, post(recover_handler))
            .route(CORE_APPLY_ENDPOINT, post(apply_handler))
            .with_state(harness);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            axum::serve(IpcListener(listener, path.clone()), router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
            #[cfg(unix)]
            let _ = std::fs::remove_file(path);
        });
        (placeholder, shutdown_tx)
    }

    #[test]
    fn local_backend_explicitly_disables_local_ipc() {
        let options = local_manager_options(Utf8PathBuf::from("runtime"));
        assert_eq!(options.local_ipc_policy, LocalIpcPolicy::Disable);
    }

    #[test]
    fn apply_outcome_mapping_preserves_all_outcomes_and_nested_warnings() {
        let cases = [
            (
                ApplyOutcome::Noop {
                    revision: revision(1),
                },
                ApplyOutcomeKind::Noop,
            ),
            (
                ApplyOutcome::Patched {
                    revision: revision(2),
                },
                ApplyOutcomeKind::Patched,
            ),
            (
                ApplyOutcome::Reloaded {
                    revision: revision(3),
                },
                ApplyOutcomeKind::Reloaded,
            ),
            (
                ApplyOutcome::Restarted {
                    revision: revision(4),
                },
                ApplyOutcomeKind::Restarted,
            ),
            (
                ApplyOutcome::Switched {
                    revision: revision(5),
                },
                ApplyOutcomeKind::Switched,
            ),
            (
                ApplyOutcome::RolledBack {
                    revision: revision(6),
                    failed_apply: "failed".to_owned(),
                },
                ApplyOutcomeKind::RolledBack,
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(map_apply_outcome(&input).outcome, expected);
        }
        let nested = ApplyOutcome::DurabilityUncertain {
            outcome: Box::new(ApplyOutcome::DurabilityUncertain {
                outcome: Box::new(ApplyOutcome::Noop {
                    revision: revision(7),
                }),
                warning: "inner".to_owned(),
            }),
            warning: "outer".to_owned(),
        };
        let mapped = map_apply_outcome(&nested);
        assert_eq!(mapped.outcome, ApplyOutcomeKind::Noop);
        assert_eq!(mapped.warning.as_deref(), Some("outer; inner"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_and_service_apply_conversion_preserve_identical_data() {
        if !transport_available() {
            return;
        }
        let local_data = map_apply_outcome(&ApplyOutcome::DurabilityUncertain {
            outcome: Box::new(ApplyOutcome::RolledBack {
                revision: revision(13),
                failed_apply: "scripted apply failure".to_owned(),
            }),
            warning: "directory sync uncertain".to_owned(),
        });
        let harness = Harness::new();
        harness.set_apply_data(local_data.clone());
        let (placeholder, shutdown) = spawn_harness(harness.clone());
        let service = CoreBackend::Service(ServiceBackend::with_placeholder(&placeholder).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let factory = test_factory(&dir, Utf8PathBuf::from("unused"));
        let request = factory.for_product(ClashCore::Mihomo).unwrap();

        let service_data = service.apply(&request, None).await.unwrap();

        assert_eq!(service_data, local_data);
        assert_eq!(harness.take_calls(), ["apply"]);
        let _ = shutdown.send(());
    }

    fn request_client_error() -> ClientError {
        let client = nyanpasu_ipc::client::Client::new("classifier-test").unwrap();
        let source = client.http_client().get("::::").build().unwrap_err();
        ClientError::Request {
            operation: "apply",
            source,
        }
    }

    fn build_client_error() -> ClientError {
        let client = nyanpasu_ipc::client::Client::new("classifier-test").unwrap();
        let source = client.http_client().get("::::").build().unwrap_err();
        ClientError::BuildClient(source)
    }

    fn service_server_error(error_kind: Option<&str>) -> CoreBackendError {
        CoreBackendError::Service(ClientError::Server {
            operation: "apply",
            code: nyanpasu_ipc::api::ResponseCode::OtherError,
            msg: "scripted".into(),
            error_kind: error_kind.map(str::to_owned),
        })
    }

    #[test]
    fn apply_backend_classifier_recognizes_revision_conflicts() {
        let revision = RevisionId {
            epoch: 1,
            generation: 2,
            effective_hash: "hash".into(),
        };
        let local = CoreBackendError::Local(ManagerError::RevisionConflict {
            expected: revision,
            actual: None,
        });
        let service = service_server_error(Some(error_kind::REVISION_CONFLICT));
        assert_eq!(
            classify_apply_backend_failure(&local),
            BackendFailureClass::RevisionConflict
        );
        assert_eq!(
            classify_apply_backend_failure(&service),
            BackendFailureClass::RevisionConflict
        );
    }

    #[test]
    fn apply_backend_classifier_recognizes_not_running() {
        let local = CoreBackendError::Local(ManagerError::NotStarted);
        let service = service_server_error(Some(error_kind::NOT_STARTED));
        assert_eq!(
            classify_apply_backend_failure(&local),
            BackendFailureClass::NotRunning
        );
        assert_eq!(
            classify_apply_backend_failure(&service),
            BackendFailureClass::NotRunning
        );
    }

    #[test]
    fn apply_backend_classifier_recognizes_transport_failures() {
        let cases = [
            CoreBackendError::Service(request_client_error()),
            CoreBackendError::Service(ClientError::HttpStatus {
                operation: "apply",
                status: reqwest::StatusCode::BAD_GATEWAY,
                body: None,
            }),
            CoreBackendError::Service(build_client_error()),
        ];
        for error in cases {
            assert_eq!(
                classify_apply_backend_failure(&error),
                BackendFailureClass::TransportLost
            );
        }
    }

    #[test]
    fn apply_backend_classifier_keeps_decoded_and_unclassified_failures_other() {
        let cases = [
            service_server_error(None),
            CoreBackendError::Service(ClientError::Decode {
                operation: "apply",
                source: serde_json::from_str::<()>("!").unwrap_err(),
            }),
            CoreBackendError::Service(ClientError::EmptyData { operation: "apply" }),
            CoreBackendError::Binary(anyhow::anyhow!("binary")),
            CoreBackendError::Construct(anyhow::anyhow!("construct")),
        ];
        for error in cases {
            assert_eq!(
                classify_apply_backend_failure(&error),
                BackendFailureClass::Other
            );
        }
    }

    #[test]
    fn local_error_kind_maps_all_twelve_protocol_constants() {
        let expected = RevisionId {
            epoch: 1,
            generation: 1,
            effective_hash: "hash".to_owned(),
        };
        let cases = [
            (ManagerError::NotStarted, error_kind::NOT_STARTED),
            (ManagerError::AlreadyRunning, error_kind::ALREADY_RUNNING),
            (
                ManagerError::RevisionConflict {
                    expected,
                    actual: None,
                },
                error_kind::REVISION_CONFLICT,
            ),
            (
                ManagerError::ManagerQuarantined {
                    epoch: 1,
                    reason: "reason".to_owned(),
                },
                error_kind::QUARANTINED,
            ),
            (
                ManagerError::ConfigCheckFailed("bad".to_owned()),
                error_kind::CONFIG_CHECK_FAILED,
            ),
            (
                ManagerError::ConfigNotFound(Utf8PathBuf::from("missing")),
                error_kind::CONFIG_NOT_FOUND,
            ),
            (
                ManagerError::BinaryNotFound(Utf8PathBuf::from("missing")),
                error_kind::BINARY_NOT_FOUND,
            ),
            (
                ManagerError::InvalidConfig("bad".to_owned()),
                error_kind::INVALID_CONFIG,
            ),
            (
                ManagerError::ControllerMissing,
                error_kind::CONTROLLER_MISSING,
            ),
            (
                ManagerError::ApplyFailed("bad".to_owned()),
                error_kind::APPLY_FAILED,
            ),
            (
                ManagerError::ApplyRollbackFailed {
                    apply: "bad".to_owned(),
                    rollback: "worse".to_owned(),
                },
                error_kind::APPLY_ROLLBACK_FAILED,
            ),
            (
                ManagerError::StopUnconfirmed("unknown".to_owned()),
                error_kind::STOP_UNCONFIRMED,
            ),
        ];
        for (error, kind) in cases {
            assert_eq!(local_error_kind(&error), Some(kind));
        }
    }

    #[test]
    fn recovery_exhausted_matches_only_the_upstream_prefix() {
        assert!(is_recovery_exhausted(&format!(
            "{RECOVERY_EXHAUSTED_PREFIX}\ndiagnostic"
        )));
        assert!(!is_recovery_exhausted("core stopped normally"));
    }

    #[test]
    fn config_revision_maps_without_runtime_path() {
        let mapped = map_revision(&revision(9));
        assert_eq!(mapped.epoch, 3);
        assert_eq!(mapped.generation, 9);
        assert_eq!(mapped.source_hash, "source-9");
        assert_eq!(mapped.effective_hash, "effective-9");
    }

    #[test]
    fn revision_id_maps_all_three_fields() {
        let mapped = map_revision_id(&RevisionIdInfo {
            epoch: 4,
            generation: 8,
            effective_hash: "hash".to_owned(),
        });
        assert_eq!(mapped.epoch, 4);
        assert_eq!(mapped.generation, 8);
        assert_eq!(mapped.effective_hash, "hash");
    }

    #[test]
    fn service_needs_stop_covers_every_faithful_state_and_missing_detail() {
        let cases = [
            (Some(CoreStateDetail::Stopped { reason: None }), false),
            (Some(CoreStateDetail::Starting { epoch: 1 }), true),
            (Some(CoreStateDetail::Running { epoch: 1, pid: 2 }), true),
            (
                Some(CoreStateDetail::Restarting {
                    epoch: 1,
                    attempt: 2,
                }),
                true,
            ),
            (
                Some(CoreStateDetail::Switching {
                    from: Some(1),
                    to: 2,
                }),
                true,
            ),
            (Some(CoreStateDetail::Stopping { epoch: 1 }), true),
            (None, true),
        ];
        for (detail, expected) in cases {
            let mut infos = stopped_infos();
            infos.detail = detail;
            assert_eq!(service_needs_stop(&infos), expected);
        }
    }

    #[tokio::test]
    async fn local_backend_releases_runtime_directory_lock_when_consumed() {
        let dir = tempfile::tempdir().unwrap();
        let factory = test_factory(&dir, Utf8PathBuf::from("unused"));
        let first = LocalBackend::new(&factory).await.unwrap();
        CoreBackend::Local(first).shutdown().await.unwrap();
        let second = LocalBackend::new(&factory).await.unwrap();
        CoreBackend::Local(second).shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn local_and_service_backends_have_real_lifecycle_parity() {
        if !transport_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let probe =
            Utf8PathBuf::from_path_buf(fake_core::require_probe_bin_path().unwrap()).unwrap();
        let factory = test_factory(&dir, probe);
        let request = factory.for_product(ClashCore::Mihomo).unwrap();
        write_config(&request, free_port());
        let local = CoreBackend::Local(LocalBackend::new(&factory).await.unwrap());

        let harness = Harness::new();
        let (placeholder, shutdown) = spawn_harness(harness);
        let service = CoreBackend::Service(ServiceBackend::with_placeholder(&placeholder).unwrap());

        local.check(&request).await.unwrap();
        service.check(&request).await.unwrap();
        let local_running = local.run(&request).await.unwrap();
        let service_running = service.run(&request).await.unwrap();
        assert!(matches!(local_running.view.state, CoreState::Running));
        assert!(matches!(service_running.view.state, CoreState::Running));
        assert!(local_running.view.revision.is_some());
        assert!(service_running.view.revision.is_some());

        let local_stopped = local.stop().await.unwrap();
        let service_stopped = service.stop().await.unwrap();
        assert!(matches!(local_stopped.view.state, CoreState::Stopped(_)));
        assert!(matches!(service_stopped.view.state, CoreState::Stopped(_)));

        let local_recovered = local.recover().await.unwrap();
        let service_recovered = service.recover().await.unwrap();
        assert!(matches!(local_recovered.view.state, CoreState::Stopped(_)));
        assert!(matches!(
            service_recovered.view.state,
            CoreState::Stopped(_)
        ));

        local.shutdown().await.unwrap();
        service.shutdown().await.unwrap();
        let _ = shutdown.send(());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn service_run_suppresses_only_not_started_stop_races() {
        if !transport_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let factory = test_factory(&dir, Utf8PathBuf::from("unused"));
        let request = factory.for_product(ClashCore::Mihomo).unwrap();
        let harness = Harness::new();
        let (placeholder, shutdown) = spawn_harness(harness.clone());
        let service = CoreBackend::Service(ServiceBackend::with_placeholder(&placeholder).unwrap());

        harness.running();
        harness.set_stop_error(error_kind::NOT_STARTED);
        service.run(&request).await.unwrap();
        assert_eq!(harness.take_calls(), ["status", "stop", "start", "status"]);

        harness.running();
        harness.set_stop_error(error_kind::QUARANTINED);
        assert!(service.run(&request).await.is_err());
        assert_eq!(harness.take_calls(), ["status", "stop"]);

        let _ = shutdown.send(());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn service_run_and_refresh_relearn_revision() {
        if !transport_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let factory = test_factory(&dir, Utf8PathBuf::from("unused"));
        let request = factory.for_product(ClashCore::Mihomo).unwrap();
        let harness = Harness::new();
        let (placeholder, shutdown) = spawn_harness(harness.clone());
        let service = CoreBackend::Service(ServiceBackend::with_placeholder(&placeholder).unwrap());

        let running = service.run(&request).await.unwrap();
        assert_eq!(running.view.revision.unwrap().generation, 1);
        harness.set_revision(9);
        let refreshed = service.observe_status().await.unwrap();
        assert_eq!(refreshed.view.revision.unwrap().generation, 9);

        service.shutdown().await.unwrap();
        let _ = shutdown.send(());
    }

    #[test]
    fn stop_reason_display_is_the_recovery_latch_input() {
        let reason = StopReason::Error(format!("{RECOVERY_EXHAUSTED_PREFIX}\ntrace"));
        assert!(is_recovery_exhausted(&reason.to_string()));
    }
}
