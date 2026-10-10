//! Private fixture copies for host-owned consumers of the public core facade.
use super::{
    logs_test_support as logs,
    platform_test_support::{MockOsProxyPort, NoopSystemDnsCache},
};
use camino::Utf8PathBuf;
use nyanpasu_config::{
    application::ReleaseChannel,
    clash::config::{ClashConfig, clash_strategy::PortStrategy},
    profile::{
        ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
        ProfileDefinition, ProfileMetadata, ProfileSource,
    },
};
use nyanpasu_core::{
    ClientSetupArgs, NyanpasuClient,
    client::{RuntimePaths, application_workflow, core_lifecycle},
    control::{CoreClient as CoreClientV2, endpoint::ExecutionHost},
    device::DeviceInfoSource,
    diagnostics::{EnvInfo, EnvironmentCollector, direct_egress::DirectEgressProbe},
    geo::{CountryIndexSource, GeoIndexError, GeodataMode, IndexKey, Loaded, OnChange},
    runtime::version::CoreVersionError,
    service::actor::ServiceClient,
    state::profiles::NewProfileRequest,
    storage::Storage,
};
use nyanpasu_jobs::JobsClient;
use nyanpasu_paths::PathResolver;
use std::sync::{Arc, Mutex as StdMutex};
use tempfile::TempDir;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// A home without databases, for tests whose subject is not the index.
#[cfg(test)]
pub struct NoopCountryIndexSource;

#[cfg(test)]
impl CountryIndexSource for NoopCountryIndexSource {
    fn load(
        &self,
        _: GeodataMode,
        _: Option<IndexKey>,
    ) -> std::result::Result<Loaded, GeoIndexError> {
        Ok(Loaded::Missing)
    }

    fn watch(&self, _: OnChange) -> std::result::Result<Box<dyn Send>, GeoIndexError> {
        Ok(Box::new(()))
    }
}

mockall::mock! {
    DirectEgressProbe {}
    #[async_trait::async_trait]
    impl DirectEgressProbe for DirectEgressProbe {
        async fn ipv4(&self) -> Option<std::net::Ipv4Addr>;
        async fn ipv6(&self) -> Option<std::net::Ipv6Addr>;
    }
}

mockall::mock! {
    CoreVersionReader {}
    #[async_trait::async_trait]
    impl nyanpasu_core::runtime::version::CoreVersionReader for CoreVersionReader {
        async fn read(&self, core: nyanpasu_config::application::ClashCore)
            -> std::result::Result<String, CoreVersionError>;
    }
}

pub(crate) struct FixedDeviceInfoSource;

impl DeviceInfoSource for FixedDeviceInfoSource {
    fn snapshot(&self) -> nyanpasu_core::device::DeviceInfo {
        nyanpasu_core::device::DeviceInfo {
            hwid: "0123456789abcdef0123456789abcdef".into(),
            device_os: "Linux".into(),
            os_version: "test-os-version".into(),
            device_model: "Test device".into(),
        }
    }
}

struct UnconfiguredEnvironmentCollector;

impl EnvironmentCollector for UnconfiguredEnvironmentCollector {
    fn collect(&self) -> std::io::Result<EnvInfo<'static>> {
        panic!("environment collection must be configured explicitly in this test")
    }
}

struct IdleEndpoint;

#[async_trait::async_trait]
impl nyanpasu_core::control::endpoint::ControlEndpoint for IdleEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Local
    }

    async fn submit(
        &self,
        submission: nyanpasu_core::control::endpoint::CoreSubmission,
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
        nyanpasu_core::control::endpoint::CoreStatusSnapshot,
        nyanpasu_core_manager::CoreError,
    > {
        Ok(nyanpasu_core::control::endpoint::CoreStatusSnapshot {
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

pub(crate) struct TestControlEndpoint {
    /// Which host this endpoint claims to be. A graph that moves the
    /// runtime between hosts needs two of these, and the verification of a
    /// restore compares the host the receipt names with the one that
    /// answered.
    host: ExecutionHost,
    fail: std::sync::atomic::AtomicBool,
    failure_kind: StdMutex<Option<&'static str>>,
    effective_enabled: std::sync::atomic::AtomicBool,
    effective_queries: std::sync::atomic::AtomicUsize,
    submissions: std::sync::atomic::AtomicUsize,
    pub(crate) local_ipc: StdMutex<Option<nyanpasu_core_manager::LocalIpcSettings>>,
    /// The endpoint's current applied revision (finding 2's CAS
    /// enforcement). Starts non-`None` so the router's very first pump
    /// read seeds the projection with a real baseline, and advances on
    /// every accepted reconcile so a stale `expected_applied` can be
    /// told apart from a fresh one the way the real runtime's
    /// `apply_config` does.
    revision: StdMutex<nyanpasu_ipc::api::status::RevisionIdInfo>,
    /// The source identity of that same revision. Tracked separately
    /// because the two move independently: a restart re-stamps the
    /// epoch-specific controller endpoint into the effective document
    /// while the configuration it came from is unchanged.
    source_hash: StdMutex<String>,
    /// Terminal results, keyed by operation id, computed once at
    /// `submit` time so `wait_operation` replays that decision instead
    /// of re-running (and re-advancing) the CAS check.
    operations:
        StdMutex<std::collections::HashMap<String, nyanpasu_ipc::api::core::v2::OperationInfo>>,
    /// What `status()` reports for `state` and `applied_kind` (R5): the
    /// stop-decision tests script the host's applied identity
    /// independently of this fake's CAS bookkeeping. Defaults to
    /// `(None, None)` -- unknown state, unknown applied kind -- the same
    /// starting point a brand new endpoint the router has not yet
    /// classified would report, which the production rule already
    /// treats conservatively as "not proven stopped".
    status_override: StdMutex<(
        Option<nyanpasu_ipc::api::status::CoreStateDetail>,
        Option<nyanpasu_core_manager::CoreKind>,
    )>,
    /// Scripts whether the next `Recover` submission fails (R6a): the
    /// death-proof step must abort
    /// `ApplicationWorkflowClient::replace_binary` before the installer
    /// runs when recovery itself cannot prove the core is dead.
    /// Defaults to `false` so existing tests, which never scripted
    /// `Recover` before it became an unconditional step, keep seeing it
    /// succeed.
    recover_should_fail: std::sync::atomic::AtomicBool,
    result_missing: std::sync::atomic::AtomicBool,
    /// Every advisory check this endpoint was asked to run, so a test can
    /// compare what the check saw with what the reconcile submitted.
    checks: StdMutex<Vec<nyanpasu_core::control::endpoint::CheckSubmission>>,
    /// What the next check answers. The default mirrors the in-process
    /// control plane accepting a document.
    check_answer: StdMutex<TestCheckAnswer>,
    /// The config bytes of every reconcile, so a test can prove the check
    /// and the apply consumed the same document.
    reconciled: StdMutex<Vec<Vec<u8>>>,
    /// Scripts the runtime answering `RolledBack`: the request failed and
    /// the manager put itself back on whatever it was already running, so
    /// the tracked revision does not advance.
    rolls_back: std::sync::atomic::AtomicBool,
    /// Scripts `status()` failing, which is what an endpoint that stopped
    /// answering looks like to a caller that needs a fresh observation.
    status_fails: std::sync::atomic::AtomicBool,
    status_reads: std::sync::atomic::AtomicUsize,
}

/// The three things a host can say about a candidate document.
#[derive(Clone)]
pub(crate) enum TestCheckAnswer {
    Pass,
    Reject(nyanpasu_core_manager::CoreError),
    Unsupported,
    /// The host accepts the request and never answers, the shape of a
    /// wedged binary or a daemon that stopped responding.
    Hang,
}

impl TestControlEndpoint {
    pub(crate) fn succeeding() -> Arc<Self> {
        Self::succeeding_on(ExecutionHost::Local)
    }

    /// The same endpoint, owned by `host`.
    pub(crate) fn succeeding_on(host: ExecutionHost) -> Arc<Self> {
        Arc::new(Self {
            host,
            fail: std::sync::atomic::AtomicBool::new(false),
            failure_kind: StdMutex::new(None),
            effective_enabled: std::sync::atomic::AtomicBool::new(false),
            effective_queries: std::sync::atomic::AtomicUsize::new(0),
            submissions: std::sync::atomic::AtomicUsize::new(0),
            local_ipc: StdMutex::new(None),
            revision: StdMutex::new(Self::initial_revision()),
            source_hash: StdMutex::new("source".to_owned()),
            operations: StdMutex::new(std::collections::HashMap::new()),
            status_override: StdMutex::new((None, None)),
            recover_should_fail: std::sync::atomic::AtomicBool::new(false),
            result_missing: std::sync::atomic::AtomicBool::new(false),
            checks: StdMutex::new(Vec::new()),
            check_answer: StdMutex::new(TestCheckAnswer::Pass),
            reconciled: StdMutex::new(Vec::new()),
            rolls_back: std::sync::atomic::AtomicBool::new(false),
            status_fails: std::sync::atomic::AtomicBool::new(false),
            status_reads: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// Boots `client` the way setup does: StartupReconcile proves the
    /// owner and applies the committed configuration. A host that has not
    /// been told otherwise boots with its core stopped, as a fresh one
    /// does.
    pub(crate) async fn prime(&self, client: &NyanpasuClient) {
        let failed = self.fail.swap(false, std::sync::atomic::Ordering::SeqCst);
        {
            let mut status = self.status_override.lock().unwrap();
            if status.0.is_none() {
                status.0 =
                    Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None });
            }
        }
        let report = client.startup_reconcile().await;
        assert_eq!(
            report.outcome,
            application_workflow::startup::StartupOutcome::Ready,
            "fixture boot: {report:?}"
        );
        self.fail.store(failed, std::sync::atomic::Ordering::SeqCst);
        self.submissions
            .store(0, std::sync::atomic::Ordering::SeqCst);
        self.checks.lock().unwrap().clear();
        self.reconciled.lock().unwrap().clear();
    }

    pub(crate) fn failing() -> Arc<Self> {
        Arc::new(Self {
            host: ExecutionHost::Local,
            fail: std::sync::atomic::AtomicBool::new(true),
            failure_kind: StdMutex::new(None),
            effective_enabled: std::sync::atomic::AtomicBool::new(false),
            effective_queries: std::sync::atomic::AtomicUsize::new(0),
            submissions: std::sync::atomic::AtomicUsize::new(0),
            local_ipc: StdMutex::new(None),
            revision: StdMutex::new(Self::initial_revision()),
            source_hash: StdMutex::new("source".to_owned()),
            operations: StdMutex::new(std::collections::HashMap::new()),
            status_override: StdMutex::new((None, None)),
            recover_should_fail: std::sync::atomic::AtomicBool::new(false),
            result_missing: std::sync::atomic::AtomicBool::new(false),
            checks: StdMutex::new(Vec::new()),
            check_answer: StdMutex::new(TestCheckAnswer::Pass),
            reconciled: StdMutex::new(Vec::new()),
            rolls_back: std::sync::atomic::AtomicBool::new(false),
            status_fails: std::sync::atomic::AtomicBool::new(false),
            status_reads: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn initial_revision() -> nyanpasu_ipc::api::status::RevisionIdInfo {
        nyanpasu_ipc::api::status::RevisionIdInfo {
            epoch: 1,
            generation: 1,
            effective_hash: "effective".into(),
        }
    }

    pub(crate) fn set_failure(&self, kind: Option<&'static str>) {
        self.fail
            .store(kind.is_some(), std::sync::atomic::Ordering::SeqCst);
        *self.failure_kind.lock().unwrap() = kind;
    }
    pub(crate) fn submissions(&self) -> usize {
        self.submissions.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Scripts `status()` as unreadable from now on.
    pub(crate) fn status_reads(&self) -> usize {
        self.status_reads.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn set_status_fails(&self, fails: bool) {
        self.status_fails
            .store(fails, std::sync::atomic::Ordering::SeqCst);
    }

    /// Scripts the next reconciles as rolled back.
    pub(crate) fn set_rolls_back(&self, rolls_back: bool) {
        self.rolls_back
            .store(rolls_back, std::sync::atomic::Ordering::SeqCst);
    }

    /// Scripts whether the host serves effective-config snapshots at all.
    /// Without one the apply is recorded but never inspected, so a test
    /// that needs `state.applied` to advance has to turn this on.
    pub(crate) fn set_effective_enabled(&self, enabled: bool) {
        self.effective_enabled
            .store(enabled, std::sync::atomic::Ordering::SeqCst);
    }

    /// Moves the effective revision the endpoint reports as applied. On its
    /// own this is what a restart of the *same* configuration looks like:
    /// the managed controller endpoint carries the epoch, so a new epoch
    /// always brings a new effective hash.
    pub(crate) fn set_effective_hash(&self, hash: &str) {
        self.revision.lock().unwrap().effective_hash = hash.into();
    }

    /// Moves the source identity the endpoint reports as applied, standing
    /// in for a core that is running a different document than the app
    /// expects.
    pub(crate) fn set_source_hash(&self, hash: &str) {
        *self.source_hash.lock().unwrap() = hash.to_owned();
    }

    pub(crate) fn set_check_answer(&self, answer: TestCheckAnswer) {
        *self.check_answer.lock().unwrap() = answer;
    }

    /// The documents this endpoint was asked to check, in order.
    pub(crate) fn checked(&self) -> Vec<nyanpasu_core::control::endpoint::CheckSubmission> {
        self.checks.lock().unwrap().clone()
    }

    /// The config bytes of every reconcile this endpoint was asked to run.
    pub(crate) fn reconciled_bytes(&self) -> Vec<Vec<u8>> {
        self.reconciled.lock().unwrap().clone()
    }

    /// Scripts what `status()` reports next (R5 stop-decision tests).
    pub(crate) fn set_status(
        &self,
        state: Option<nyanpasu_ipc::api::status::CoreStateDetail>,
        applied_kind: Option<nyanpasu_core_manager::CoreKind>,
    ) {
        *self.status_override.lock().unwrap() = (state, applied_kind);
    }

    /// Scripts whether the next `Recover` submission fails (R6a tests).
    pub(crate) fn set_result_missing(&self, missing: bool) {
        self.result_missing
            .store(missing, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn set_recover_should_fail(&self, should_fail: bool) {
        self.recover_should_fail
            .store(should_fail, std::sync::atomic::Ordering::SeqCst);
    }

    /// Computes and records the terminal result for `submission`.
    /// `Reconcile` enforces CAS the way the real runtime's
    /// `apply_config` does: `expected_applied: None` skips the check, a
    /// `Some(r)` that disagrees with the tracked revision fails with
    /// `revision_conflict`, and an accepted reconcile advances it.
    fn operation(
        &self,
        submission: &nyanpasu_core::control::endpoint::CoreSubmission,
    ) -> nyanpasu_ipc::api::core::v2::OperationInfo {
        let id = submission.envelope.operation_id.to_string();
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return nyanpasu_ipc::api::core::v2::OperationInfo {
                id,
                phase: nyanpasu_ipc::api::core::v2::OperationPhase::Failed,
                output: None,
                error: Some(nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                    kind: self.failure_kind.lock().unwrap().map(Into::into),
                    message: "reconcile boom".into(),
                    retryable: false,
                }),
            };
        }
        // F4 regression coverage needs a real `Stopped` answer: the
        // updater's replacement step stops the core before copying the
        // binary, and the facade rejects any other output for `Stop`.
        if matches!(
            submission.envelope.command,
            nyanpasu_core_manager::CoreCommand::Stop
        ) {
            // A host that stopped its core says so when it is next read,
            // which is the proof a retired or stopped owner is held to.
            self.status_override.lock().unwrap().0 =
                Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None });
            return nyanpasu_ipc::api::core::v2::OperationInfo {
                id,
                phase: nyanpasu_ipc::api::core::v2::OperationPhase::Succeeded,
                output: Some(nyanpasu_ipc::api::core::v2::OperationOutputInfo::Stopped),
                error: None,
            };
        }
        // R6a: `ApplicationWorkflowClient::replace_binary` now submits
        // `Recover` unconditionally as the death proof, and the facade
        // rejects any other output for it. Scriptable to fail so a test
        // can prove the replacement aborts before its installer runs when
        // recovery cannot prove the core dead.
        if matches!(
            submission.envelope.command,
            nyanpasu_core_manager::CoreCommand::Recover
        ) {
            if self
                .recover_should_fail
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                return nyanpasu_ipc::api::core::v2::OperationInfo {
                    id,
                    phase: nyanpasu_ipc::api::core::v2::OperationPhase::Failed,
                    output: None,
                    error: Some(nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                        kind: None,
                        message: "recover boom".into(),
                        retryable: false,
                    }),
                };
            }
            return nyanpasu_ipc::api::core::v2::OperationInfo {
                id,
                phase: nyanpasu_ipc::api::core::v2::OperationPhase::Succeeded,
                output: Some(nyanpasu_ipc::api::core::v2::OperationOutputInfo::Recovered),
                error: None,
            };
        }
        let nyanpasu_core_manager::CoreCommand::Reconcile(request) = &submission.envelope.command
        else {
            return successful_reconcile(submission.envelope.operation_id);
        };
        *self.local_ipc.lock().unwrap() = request.options.local_ipc;
        let nyanpasu_core_manager::ConfigInput::Inline { bytes, .. } = &request.config;
        self.reconciled.lock().unwrap().push(bytes.clone());
        let mut current = self.revision.lock().unwrap();
        if let Some(expected) = &request.expected_applied {
            let stale = expected.epoch.get() != current.epoch
                || expected.generation != current.generation
                || expected.effective_hash != current.effective_hash;
            if stale {
                return nyanpasu_ipc::api::core::v2::OperationInfo {
                    id,
                    phase: nyanpasu_ipc::api::core::v2::OperationPhase::Failed,
                    output: None,
                    error: Some(nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                        kind: Some("revision_conflict".into()),
                        message: format!(
                            "revision conflict: expected epoch={} generation={} \
                             effective_hash={}, actual epoch={} generation={} \
                             effective_hash={}",
                            expected.epoch.get(),
                            expected.generation,
                            expected.effective_hash,
                            current.epoch,
                            current.generation,
                            current.effective_hash,
                        ),
                        retryable: true,
                    }),
                };
            }
        }
        let rolled_back = self.rolls_back.load(std::sync::atomic::Ordering::SeqCst);
        if !rolled_back {
            current.generation += 1;
            *self.source_hash.lock().unwrap() = nyanpasu_core_manager::payload_digest(bytes);
            *self.status_override.lock().unwrap() = (
                Some(nyanpasu_ipc::api::status::CoreStateDetail::Running {
                    epoch: current.epoch,
                    pid: 7,
                }),
                Some(request.core.kind),
            );
        }
        let applied = current.clone();
        nyanpasu_ipc::api::core::v2::OperationInfo {
            id,
            phase: nyanpasu_ipc::api::core::v2::OperationPhase::Succeeded,
            output: Some(
                nyanpasu_ipc::api::core::v2::OperationOutputInfo::Reconciled(
                    nyanpasu_ipc::api::core::v2::ReconcileOutcomeInfo {
                        outcome: if rolled_back {
                            nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::RolledBack
                        } else {
                            nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Started
                        },
                        revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                            epoch: applied.epoch,
                            generation: applied.generation,
                            source_hash: self.source_hash.lock().unwrap().clone(),
                            effective_hash: applied.effective_hash,
                        },
                        warning: None,
                        failed_apply: rolled_back
                            .then(|| "scripted: the target would not start".to_owned()),
                    },
                ),
            ),
            error: None,
        }
    }
}

#[async_trait::async_trait]
impl nyanpasu_core::control::endpoint::ControlEndpoint for TestControlEndpoint {
    fn host(&self) -> ExecutionHost {
        self.host
    }

    async fn check_config(
        &self,
        submission: nyanpasu_core::control::endpoint::CheckSubmission,
    ) -> nyanpasu_core::control::endpoint::CheckSupport {
        use nyanpasu_core::control::endpoint::CheckSupport;
        let answer = self.check_answer.lock().unwrap().clone();
        self.checks.lock().unwrap().push(submission);
        match answer {
            TestCheckAnswer::Pass => CheckSupport::Ran(Ok(())),
            TestCheckAnswer::Reject(error) => CheckSupport::Ran(Err(error)),
            TestCheckAnswer::Unsupported => CheckSupport::Unsupported {
                reason: "scripted: this host exposes no config check".into(),
            },
            TestCheckAnswer::Hang => std::future::pending().await,
        }
    }

    async fn submit(
        &self,
        submission: nyanpasu_core::control::endpoint::CoreSubmission,
    ) -> std::result::Result<
        nyanpasu_ipc::api::core::v2::OperationInfo,
        nyanpasu_core_manager::CoreError,
    > {
        self.submissions
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let info = self.operation(&submission);
        self.operations
            .lock()
            .unwrap()
            .insert(info.id.clone(), info.clone());
        Ok(info)
    }

    async fn wait_operation(
        &self,
        id: nyanpasu_core_manager::OperationId,
        _timeout: std::time::Duration,
    ) -> Option<nyanpasu_ipc::api::core::v2::OperationInfo> {
        if self
            .result_missing
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return None;
        }
        self.operations
            .lock()
            .unwrap()
            .get(&id.to_string())
            .cloned()
    }

    async fn effective_config(
        &self,
    ) -> std::result::Result<
        Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>,
        nyanpasu_core_manager::CoreError,
    > {
        use std::sync::atomic::Ordering;
        if !self.effective_enabled.load(Ordering::SeqCst) {
            return Ok(None);
        }
        if self.effective_queries.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(nyanpasu_core_manager::CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::BackendUnavailable,
                "snapshot temporarily unavailable",
                true,
            ));
        }
        let current = self.revision.lock().unwrap().clone();
        Ok(Some(nyanpasu_ipc::api::core::v2::CoreEffectiveConfig {
            instance_id: "test-instance".into(),
            revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                epoch: current.epoch,
                generation: current.generation,
                source_hash: self.source_hash.lock().unwrap().clone(),
                effective_hash: current.effective_hash,
            },
            config: "mode: rule\nexternal-controller-unix: /tmp/recovered.sock\n".into(),
        }))
    }

    async fn status(
        &self,
    ) -> std::result::Result<
        nyanpasu_core::control::endpoint::CoreStatusSnapshot,
        nyanpasu_core_manager::CoreError,
    > {
        self.status_reads
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.status_fails.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(nyanpasu_core_manager::CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::BackendUnavailable,
                "scripted: the host stopped answering status reads",
                true,
            ));
        }
        let (state, applied_kind) = self.status_override.lock().unwrap().clone();
        Ok(nyanpasu_core::control::endpoint::CoreStatusSnapshot {
            controller: None,
            state,
            state_changed_at: 0,
            revision: Some(self.revision.lock().unwrap().clone()),
            source_hash: Some(self.source_hash.lock().unwrap().clone()),
            healthy: Some(true),
            applied_kind,
        })
    }
}

pub(crate) fn successful_reconcile(
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

pub(crate) struct IdleServiceAdapter;

#[async_trait::async_trait]
impl nyanpasu_core::service::actor::ServiceHostAdapter for IdleServiceAdapter {
    async fn probe(
        &self,
    ) -> std::result::Result<
        nyanpasu_ipc::types::StatusInfo<'static>,
        nyanpasu_core::service::control::ServiceCommandError,
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
    ) -> std::result::Result<(), nyanpasu_core::service::control::ServiceCommandError> {
        Ok(())
    }

    async fn uninstall(
        &self,
    ) -> std::result::Result<(), nyanpasu_core::service::control::ServiceCommandError> {
        Ok(())
    }

    async fn start_daemon(
        &self,
    ) -> std::result::Result<(), nyanpasu_core::service::control::ServiceCommandError> {
        Ok(())
    }

    async fn stop_daemon(
        &self,
    ) -> std::result::Result<(), nyanpasu_core::service::control::ServiceCommandError> {
        Ok(())
    }

    async fn update(
        &self,
    ) -> std::result::Result<(), nyanpasu_core::service::control::ServiceCommandError> {
        Ok(())
    }

    fn endpoint(&self) -> nyanpasu_core::control::endpoint::EndpointHandle {
        Arc::new(IdleEndpoint)
    }
}

pub(crate) async fn test_v2_clients() -> (CoreClientV2, ServiceClient) {
    test_v2_clients_with_endpoint(test_idle_endpoint()).await
}

/// A core that answers nothing, for tests whose subject is the facade
/// rather than the running core.
pub(crate) fn test_idle_endpoint() -> nyanpasu_core::control::endpoint::EndpointHandle {
    Arc::new(IdleEndpoint)
}

pub(crate) async fn test_v2_clients_with_endpoint(
    endpoint: nyanpasu_core::control::endpoint::EndpointHandle,
) -> (CoreClientV2, ServiceClient) {
    let core = CoreClientV2::spawn(endpoint).await.unwrap();
    let service = ServiceClient::spawn(Arc::new(IdleServiceAdapter), 0)
        .await
        .unwrap();
    (core, service)
}

pub(crate) fn test_clash_config() -> ClashConfig {
    ClashConfig {
        mixed_port: PortStrategy::new_allow_fallback(0),
        ..ClashConfig::default()
    }
}

/// Seeds `path` with [`test_clash_config`], which the clash config client
/// then loads instead of creating the default.
fn test_backup_deps(dir: &TempDir) -> (PathResolver, Storage) {
    let paths = test_paths(dir.path(), dir.path().join("data"));
    std::fs::create_dir_all(paths.app_data_dir()).unwrap();
    let storage = Storage::try_new(paths.storage_path().as_std_path()).unwrap();
    (paths, storage)
}

fn seed_test_clash_config(path: impl AsRef<std::path::Path>) {
    write_clash_config(path, &test_clash_config());
}

/// The stamped file the clash config client loads.
pub(crate) fn write_clash_config(path: impl AsRef<std::path::Path>, config: &ClashConfig) {
    use nyanpasu_core::format::Format as _;

    let mut content = Vec::new();
    nyanpasu_core::migration::modules::clash_config::ClashConfigFormat::default()
        .serialize(&mut content, config, None)
        .unwrap();
    std::fs::write(path, content).unwrap();
}

pub(crate) async fn test_client_args_with_endpoint(
    dir: &TempDir,
    endpoint: nyanpasu_core::control::endpoint::EndpointHandle,
) -> ClientSetupArgs {
    let (paths, storage) = test_backup_deps(dir);
    seed_test_clash_config(paths.clash_config_path());
    let runtime_paths = RuntimePaths::from_resolver(&paths);
    let (shutdown, tasks) = (
        tokio_util::sync::CancellationToken::new(),
        tokio_util::task::TaskTracker::new(),
    );
    let (core_v2, service) = test_v2_clients_with_endpoint(endpoint).await;
    ClientSetupArgs {
        profile_user_agent: format!("clash-nyanpasu/v{}", crate::consts::BUILD_INFO.pkg_version),
        kernel_user_agent: format!("clash-nyanpasu/{}", crate::consts::BUILD_INFO.app_version),
        backup_app_version: crate::consts::BUILD_INFO.pkg_version.to_owned(),
        jobs: test_client_with_owner(shutdown.clone(), &tasks).await,
        installed_channel: ReleaseChannel::Stable,
        is_portable: false,
        environment: Arc::new(UnconfiguredEnvironmentCollector),
        device_info: Arc::new(FixedDeviceInfoSource),
        logging: logs::test_setup(paths.app_logs_dir().into_std_path_buf()),
        paths,
        storage,
        runtime_paths,
        core_specs: Arc::new(runtime_core_spec),
        invalidation: None,
        core_v2,
        service,
        system_dns: Arc::new(NoopSystemDnsCache),
        direct_egress: Arc::new(MockDirectEgressProbe::new()),
        geo_index: Arc::new(NoopCountryIndexSource),
        os_proxy: Arc::new(MockOsProxyPort::new()),
        binary_installer: Arc::new(core_lifecycle::FsBinaryInstaller),
        core_versions: Arc::new(MockCoreVersionReader::new()),
        effects: Arc::new(crate::desktop::effects_test_support::NoopApplicationEffects),
        // The real rule: a test that writes a hotkey the platform cannot
        // parse should fail here, exactly as the app would.
        accelerators: Arc::new(crate::desktop::hotkey::adapters::PlatformAcceleratorValidator),
        traffic_store: None,
        shutdown,
        tasks,
    }
}

pub(crate) fn minimal_file_profile_request() -> NewProfileRequest {
    NewProfileRequest {
        metadata: ProfileMetadata {
            name: "t".into(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new("t.yaml").unwrap(),
                            updated_at: None,
                        },
                    },
                },
                transforms: vec![],
            }),
        },
    }
}

pub(crate) fn test_paths(
    config: impl AsRef<std::path::Path>,
    data: impl AsRef<std::path::Path>,
) -> PathResolver {
    let utf8 = |path: &std::path::Path| {
        Utf8PathBuf::from_path_buf(path.to_owned()).expect("test directories are UTF-8")
    };
    PathResolver::with_base_dirs(utf8(config.as_ref()), utf8(data.as_ref()))
}

fn runtime_core_spec(
    core: &nyanpasu_config::application::ClashCore,
) -> std::result::Result<
    nyanpasu_core_manager::CoreSpec,
    nyanpasu_core::control::local_host::CoreSpecError,
> {
    use nyanpasu_core_manager::CoreKind;
    let kind = match core {
        nyanpasu_config::application::ClashCore::ClashPremium => CoreKind::ClashPremium,
        nyanpasu_config::application::ClashCore::ClashRs
        | nyanpasu_config::application::ClashCore::ClashRsAlpha => CoreKind::ClashRust,
        nyanpasu_config::application::ClashCore::Mihomo
        | nyanpasu_config::application::ClashCore::MihomoAlpha => CoreKind::Mihomo,
        nyanpasu_config::application::ClashCore::Meow
        | nyanpasu_config::application::ClashCore::MeowAlpha => CoreKind::Meow,
    };
    Ok(nyanpasu_core_manager::CoreSpec {
        kind,
        binary_path: camino::Utf8PathBuf::from("fake-core"),
        version: None,
        features: Vec::new(),
    })
}

#[cfg(test)]
pub(crate) async fn test_client_with_owner(
    shutdown: CancellationToken,
    tasks: &TaskTracker,
) -> JobsClient {
    let directory = tempfile::tempdir().unwrap();
    let client = nyanpasu_core::client::jobs::start(
        directory.path().join("jobs.redb"),
        nyanpasu_core::client::jobs::capture(),
        shutdown.clone(),
        tasks,
    )
    .await
    .unwrap();
    tokio::spawn({
        let tasks = tasks.clone();
        async move {
            let _directory = directory;
            shutdown.cancelled().await;
            tasks.wait().await;
        }
    });
    client
}
