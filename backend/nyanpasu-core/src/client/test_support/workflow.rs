//! Scripted runtime replies and installer ownership fixtures shared by tests.

use crate::{
    actor_v2::endpoint::ExecutionHost,
    client::{
        NyanpasuClient,
        core_lifecycle::ports::{
            BinaryInstallProgress, BinaryInstaller, InstallCoreBinaryError, PreparedCoreBinary,
        },
        runtime_error::RuntimeError,
        test_support::{TestControlEndpoint, test_client_args_with_endpoint},
    },
};
use nyanpasu_config::application::ClashCore;
use nyanpasu_core_manager::{CoreError, OperationId};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use struct_patch::Patch;
use tokio::sync::Notify;
/// What became of the runtime after the aborted commit.
pub fn aftermath(
    aborted: &crate::state::mutation::CommitAborted,
) -> &crate::state::mutation::RuntimeAftermath {
    use crate::state::mutation::CommitAborted::*;
    match aborted {
        WriteConfig { runtime, .. }
        | RecoverAfterWriteFailure { runtime, .. }
        | RuntimeRefused { runtime, .. }
        | RuntimeFailed { runtime, .. } => runtime,
        ValidateState { .. } => panic!("{aborted:?} has no runtime aftermath"),
    }
}

pub async fn disable_mode_interruption(client: &NyanpasuClient) {
    let mut config = client.get_clash_config().await.unwrap();
    config.break_connection.on_mode_change = false;
    let mut patch = nyanpasu_config::clash::config::ClashConfig::new_empty_patch();
    patch.break_connection = config.break_connection.into_patch();
    client.patch_clash_config(patch).await.unwrap();
}

pub fn override_patch(
    value: serde_json::Value,
) -> nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch {
    serde_json::from_value(value).unwrap()
}

pub async fn start_replacement(
    f: &Fixture,
) -> (
    tokio::task::JoinHandle<Result<(), RuntimeError>>,
    std::path::PathBuf,
) {
    let target = f.client.get_app_config().await.unwrap().core;
    let (artifact, _) = f.artifact(target);
    let staging_path = artifact.staging.path().to_owned();
    let client = f.client.clone();
    let task = tokio::spawn(async move {
        client
            .inner
            .application_workflow
            .replace_binary(artifact)
            .await
    });
    f.installer.entered.notified().await;
    (task, staging_path)
}

/// How [`ScriptedWaitEndpoint`] answers the wait for one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitScript {
    /// The real terminal result.
    Deliver,
    /// The registry answers nothing: an admitted operation whose result is
    /// lost.
    Missing,
    /// The operation is admitted and still running.
    Running,
    /// The real terminal result, held until the test releases it.
    Held,
}

/// A control endpoint whose operation waits are scripted per operation.
///
/// Each submission takes the next queued script (`Deliver` once the queue is
/// empty) and keeps it until a test rescripts that operation, so the one
/// operation a test cares about can be lost or left running, and later
/// delivered, while everything else is real.
pub struct ScriptedWaitEndpoint {
    pub(crate) delegate: Arc<TestControlEndpoint>,
    pub(crate) queued: std::sync::Mutex<std::collections::VecDeque<WaitScript>>,
    pub(crate) scripts: std::sync::Mutex<Vec<(OperationId, WaitScript)>>,
    pub(crate) held: Notify,
    pub(crate) release: Notify,
}

impl ScriptedWaitEndpoint {
    pub fn new(delegate: Arc<TestControlEndpoint>) -> Arc<Self> {
        Arc::new(Self {
            delegate,
            queued: std::sync::Mutex::new(std::collections::VecDeque::new()),
            scripts: std::sync::Mutex::new(Vec::new()),
            held: Notify::new(),
            release: Notify::new(),
        })
    }

    /// Resolves once a `Held` wait is holding its result.
    #[cfg(test)]
    pub(crate) async fn held(&self) {
        self.held.notified().await;
    }

    /// Lets the held wait deliver.
    #[cfg(test)]
    pub(crate) fn release(&self) {
        self.release.notify_one();
    }

    /// The script the next submission takes.
    pub fn queue(&self, script: WaitScript) {
        self.queued.lock().unwrap().push_back(script);
    }

    #[cfg(test)]
    pub(crate) fn rescript(&self, operation: OperationId, script: WaitScript) {
        let mut scripts = self.scripts.lock().unwrap();
        let entry = scripts
            .iter_mut()
            .find(|(id, _)| *id == operation)
            .expect("only a submitted operation is rescripted");
        entry.1 = script;
    }

    #[cfg(test)]
    pub(crate) fn submitted(&self) -> usize {
        self.scripts.lock().unwrap().len()
    }

    /// Every submitted operation, in order.
    #[cfg(test)]
    pub(crate) fn operations(&self) -> Vec<OperationId> {
        self.scripts
            .lock()
            .unwrap()
            .iter()
            .map(|(operation, _)| *operation)
            .collect()
    }
}

#[async_trait::async_trait]
impl crate::actor_v2::endpoint::ControlEndpoint for ScriptedWaitEndpoint {
    async fn effective_config(
        &self,
    ) -> Result<Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>, CoreError> {
        self.delegate.effective_config().await
    }
    async fn api_connection(
        &self,
    ) -> Result<Option<nyanpasu_ipc::api::core::v2::CoreApiConnection>, CoreError> {
        self.delegate.api_connection().await
    }
    async fn api_changes(
        &self,
    ) -> Result<Option<crate::actor_v2::endpoint::ApiChanges>, CoreError> {
        self.delegate.api_changes().await
    }
    fn host(&self) -> ExecutionHost {
        self.delegate.host()
    }
    async fn check_config(
        &self,
        submission: crate::actor_v2::endpoint::CheckSubmission,
    ) -> crate::actor_v2::endpoint::CheckSupport {
        self.delegate.check_config(submission).await
    }
    async fn submit(
        &self,
        submission: crate::actor_v2::endpoint::CoreSubmission,
    ) -> Result<nyanpasu_ipc::api::core::v2::OperationInfo, CoreError> {
        let operation = submission.envelope.operation_id;
        let script = self
            .queued
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(WaitScript::Deliver);
        self.scripts.lock().unwrap().push((operation, script));
        self.delegate.submit(submission).await
    }
    async fn wait_operation(
        &self,
        id: OperationId,
        timeout: Duration,
    ) -> Option<nyanpasu_ipc::api::core::v2::OperationInfo> {
        let script = self
            .scripts
            .lock()
            .unwrap()
            .iter()
            .find(|(operation, _)| *operation == id)
            .map_or(WaitScript::Deliver, |(_, script)| *script);
        match script {
            WaitScript::Deliver => self.delegate.wait_operation(id, timeout).await,
            WaitScript::Missing => None,
            WaitScript::Running => Some(nyanpasu_ipc::api::core::v2::OperationInfo {
                id: id.to_string(),
                phase: nyanpasu_ipc::api::core::v2::OperationPhase::Running,
                output: None,
                error: None,
            }),
            WaitScript::Held => {
                self.held.notify_one();
                self.release.notified().await;
                self.delegate.wait_operation(id, timeout).await
            }
        }
    }
    async fn status(&self) -> Result<crate::actor_v2::endpoint::CoreStatusSnapshot, CoreError> {
        self.delegate.status().await
    }
}

#[derive(Default)]
pub(crate) struct Progress(pub(crate) AtomicBool, pub(crate) AtomicBool);

impl BinaryInstallProgress for Progress {
    fn restarting(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    fn finished(&self, _error: Option<&str>) {
        self.1.store(true, Ordering::SeqCst);
    }
}

pub struct Installer {
    pub(crate) endpoint: Arc<TestControlEndpoint>,
    pub(crate) entered: Notify,
    pub release: Notify,
    pub(crate) park: bool,
    pub(crate) fail: bool,
    pub(crate) calls: AtomicUsize,
    pub(crate) submissions_at_copy: AtomicUsize,
}

#[async_trait::async_trait]
impl BinaryInstaller for Installer {
    async fn install(&self, artifact: &PreparedCoreBinary) -> Result<(), InstallCoreBinaryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.submissions_at_copy
            .store(self.endpoint.submissions(), Ordering::SeqCst);
        self.entered.notify_one();
        if self.park {
            self.release.notified().await;
        }
        if self.fail {
            return Err(InstallCoreBinaryError::ElevatedCopyFailed {
                core: artifact.target,
                destination: (&artifact.destination).into(),
                exit_code: Some(1),
            });
        }
        tokio::fs::copy(&artifact.source, &artifact.destination)
            .await
            .unwrap();
        Ok(())
    }
}

pub struct Fixture {
    pub client: NyanpasuClient,
    pub endpoint: Arc<TestControlEndpoint>,
    pub installer: Arc<Installer>,
    pub(crate) dir: tempfile::TempDir,
}

impl Fixture {
    #[cfg(test)]
    pub(crate) async fn new(park: bool, fail: bool) -> Self {
        Self::with_adapters(
            park,
            fail,
            crate::client::test_support::profile_test_adapters,
        )
        .await
    }

    pub async fn with_adapters(
        park: bool,
        fail: bool,
        adapters: impl FnOnce(&tempfile::TempDir) -> crate::client::host_adapters::ClientHostAdapters,
    ) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = TestControlEndpoint::succeeding();
        let installer = Arc::new(Installer {
            endpoint: endpoint.clone(),
            entered: Notify::new(),
            release: Notify::new(),
            park,
            fail,
            calls: AtomicUsize::new(0),
            submissions_at_copy: AtomicUsize::new(0),
        });
        let mut args = test_client_args_with_endpoint(&dir, endpoint.clone(), adapters(&dir)).await;
        args.binary_installer = installer.clone();
        let client = NyanpasuClient::try_new(args).await.unwrap();
        Self {
            client,
            endpoint,
            installer,
            dir,
        }
    }

    pub(crate) fn artifact(&self, target: ClashCore) -> (PreparedCoreBinary, Arc<Progress>) {
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let source = staging.path().join("new-core");
        std::fs::write(&source, b"new binary").unwrap();
        let progress = Arc::new(Progress::default());
        (
            PreparedCoreBinary {
                target,
                source,
                destination: self.dir.path().join("installed-core"),
                staging,
                progress: progress.clone(),
                companion: None,
            },
            progress,
        )
    }
}
