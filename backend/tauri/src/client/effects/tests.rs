//! Facade-level tests for the mutation pipeline.
//!
//! Everything lives in a `TempDir`, the effect port is a recording fake and the
//! core endpoint is a stub, so no OS state is touched and nothing sleeps.

use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicUsize, Ordering},
};

use nyanpasu_config::{
    application::{I18nLanguage, NyanpasuAppConfig},
    clash::config::{ClashConfig, ClashControlChannel, clash_strategy::port::PortStrategy},
    state::{
        PersistentState,
        window::{WindowLabel, WindowState},
    },
};
use struct_patch::Patch as _;
use tempfile::{TempDir, tempdir};

use super::{
    plan::{ApplicationEffect, ApplicationEffectPlan, EffectKind},
    ports::ApplicationEffectsPort,
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::{
    client::{
        NyanpasuClient,
        runtime::{DegradationPhase, MutationOutcome},
        tests::{successful_reconcile, test_client_args_with_endpoint},
    },
    core::actor_v2::endpoint::{
        ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
    },
};

/// Reports the core as stopped so the two runtime-apply kinds are
/// distinguishable: a rebuild always submits, while a control-channel apply
/// only refreshes the status and returns.
#[derive(Default)]
struct StubEndpoint {
    submissions: AtomicUsize,
    status_queries: AtomicUsize,
}

#[async_trait::async_trait]
impl ControlEndpoint for StubEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Local
    }

    async fn submit(
        &self,
        submission: CoreSubmission,
    ) -> std::result::Result<
        nyanpasu_ipc::api::core::v2::OperationInfo,
        nyanpasu_core_manager::CoreError,
    > {
        self.submissions.fetch_add(1, Ordering::SeqCst);
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
    ) -> std::result::Result<CoreStatusSnapshot, nyanpasu_core_manager::CoreError> {
        self.status_queries.fetch_add(1, Ordering::SeqCst);
        Ok(CoreStatusSnapshot {
            controller: None,
            state: Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
            state_changed_at: 0,
            revision: None,
            healthy: Some(true),
            applied_kind: None,
        })
    }
}

struct Dispatch {
    revision: EffectRevision,
    plan: ApplicationEffectPlan,
}

/// Records what the facade dispatched and, optionally, degrades every effect so
/// the committed-degraded path can be observed end to end.
struct RecordingEffectsPort {
    dispatches: StdMutex<Vec<Dispatch>>,
    degrade: bool,
}

impl RecordingEffectsPort {
    fn healthy() -> Arc<Self> {
        Arc::new(Self {
            dispatches: StdMutex::new(Vec::new()),
            degrade: false,
        })
    }

    fn degrading() -> Arc<Self> {
        Arc::new(Self {
            dispatches: StdMutex::new(Vec::new()),
            degrade: true,
        })
    }

    fn dispatches(&self) -> std::sync::MutexGuard<'_, Vec<Dispatch>> {
        self.dispatches
            .lock()
            .expect("dispatch log should not poison")
    }

    fn dispatch_count(&self) -> usize {
        self.dispatches().len()
    }

    /// The plan carried by the highest revision this port ever received, which
    /// is the desired state every owner must converge on.
    fn newest_plan(&self) -> ApplicationEffectPlan {
        self.dispatches()
            .iter()
            .max_by_key(|dispatch| dispatch.revision)
            .map(|dispatch| dispatch.plan.clone())
            .expect("at least one dispatch")
    }

    fn revisions(&self) -> Vec<u64> {
        self.dispatches()
            .iter()
            .map(|dispatch| dispatch.revision.get())
            .collect()
    }
}

#[async_trait::async_trait]
impl ApplicationEffectsPort for RecordingEffectsPort {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        let statuses = plan
            .effects()
            .iter()
            .map(|effect| EffectStatus {
                kind: effect.kind(),
                desired_revision: revision,
                applied_revision: revision,
                health: if self.degrade {
                    EffectHealth::Degraded {
                        code: "injected_effect_failure",
                        message: "effect owner refused".to_owned(),
                        retryable: true,
                    }
                } else {
                    EffectHealth::Healthy
                },
            })
            .collect();
        self.dispatches().push(Dispatch { revision, plan });
        statuses
    }

    async fn shutdown(&self) -> Vec<EffectStatus> {
        Vec::new()
    }
}

/// Degrades one effect kind for the first `failing_dispatches` dispatches and
/// reports everything healthy afterwards, so a retry can be observed succeeding.
struct FlakyEffectsPort {
    dispatches: StdMutex<Vec<Dispatch>>,
    kind: EffectKind,
    retryable: bool,
    failing_dispatches: usize,
}

impl FlakyEffectsPort {
    fn new(kind: EffectKind, retryable: bool, failing_dispatches: usize) -> Arc<Self> {
        Arc::new(Self {
            dispatches: StdMutex::new(Vec::new()),
            kind,
            retryable,
            failing_dispatches,
        })
    }

    fn dispatches(&self) -> std::sync::MutexGuard<'_, Vec<Dispatch>> {
        self.dispatches
            .lock()
            .expect("dispatch log should not poison")
    }

    fn dispatch_count(&self) -> usize {
        self.dispatches().len()
    }

    fn plan(&self, index: usize) -> ApplicationEffectPlan {
        self.dispatches()
            .get(index)
            .map(|dispatch| dispatch.plan.clone())
            .unwrap_or_else(|| panic!("dispatch {index} should have happened"))
    }
}

#[async_trait::async_trait]
impl ApplicationEffectsPort for FlakyEffectsPort {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        let failing = self.dispatch_count() < self.failing_dispatches;
        let statuses = plan
            .effects()
            .iter()
            .map(|effect| EffectStatus {
                kind: effect.kind(),
                desired_revision: revision,
                applied_revision: revision,
                health: if failing && effect.kind() == self.kind {
                    EffectHealth::Degraded {
                        code: "injected_effect_failure",
                        message: "effect owner refused".to_owned(),
                        retryable: self.retryable,
                    }
                } else {
                    EffectHealth::Healthy
                },
            })
            .collect();
        self.dispatches().push(Dispatch { revision, plan });
        statuses
    }

    async fn shutdown(&self) -> Vec<EffectStatus> {
        Vec::new()
    }
}

/// Fails the application commit as soon as the patch turns the system proxy on,
/// while accepting the seed snapshot the client loads at startup.
struct FailingVergeMirror;

impl crate::state::mirror::VergeLegacyBridge for FailingVergeMirror {
    fn prepare(
        &self,
        snap: &NyanpasuAppConfig,
    ) -> anyhow::Result<Box<dyn crate::state::mirror::PreparedLegacyMirror>> {
        if snap.enable_system_proxy {
            anyhow::bail!("injected application mirror prepare failure");
        }
        Ok(Box::new(crate::state::mirror::NoopPreparedLegacyMirror))
    }

    fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
        Ok(NyanpasuAppConfig::default())
    }
}

fn client_with(
    dir: &TempDir,
    port: Arc<dyn ApplicationEffectsPort>,
    endpoint: Arc<StubEndpoint>,
) -> NyanpasuClient {
    let mut args = test_client_args_with_endpoint(dir, endpoint);
    args.effects = port;
    NyanpasuClient::try_new_with_args(args).expect("client should construct")
}

fn language_patch(language: I18nLanguage) -> nyanpasu_config::application::NyanpasuAppConfigPatch {
    let mut patch = NyanpasuAppConfig::new_empty_patch();
    patch.language = Some(language);
    patch
}

fn degradations(outcome: &MutationOutcome<()>) -> &[crate::client::runtime::Degradation] {
    match outcome {
        MutationOutcome::Applied { .. } => &[],
        MutationOutcome::CommittedDegraded { degradations, .. } => degradations,
    }
}

#[test]
fn commit_failure_runs_no_effects() {
    let dir = tempdir().expect("tempdir should be created");
    let port = RecordingEffectsPort::healthy();
    let mut args = test_client_args_with_endpoint(&dir, Arc::new(StubEndpoint::default()));
    args.effects = port.clone();
    args.bridges.verge = Arc::new(FailingVergeMirror);
    let client = NyanpasuClient::try_new_with_args(args).expect("client should construct");

    tauri::async_runtime::block_on(async {
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_system_proxy = Some(true);
        let error = client
            .patch_app_config(patch)
            .await
            .expect_err("a failing commit must not report success");
        assert!(
            format!("{error:#}").contains("injected application mirror"),
            "unexpected error: {error:#}"
        );
        assert_eq!(
            port.dispatch_count(),
            0,
            "nothing was committed, so nothing may be dispatched"
        );
        assert!(
            !client
                .get_app_config()
                .await
                .expect("config should read back")
                .enable_system_proxy,
            "the rejected value must not be visible"
        );
    });
}

#[test]
fn effect_failure_keeps_committed_config() {
    let dir = tempdir().expect("tempdir should be created");
    let port = RecordingEffectsPort::degrading();
    let client = client_with(&dir, port.clone(), Arc::new(StubEndpoint::default()));

    tauri::async_runtime::block_on(async {
        let outcome = client
            .patch_app_config(language_patch(I18nLanguage::Korean))
            .await
            .expect("a post-commit effect failure is not a commit failure");

        let degradations = degradations(&outcome);
        assert!(
            degradations
                .iter()
                .any(|degradation| degradation.code == "injected_effect_failure"),
            "expected the effect failure on the wire: {degradations:?}"
        );
        assert_eq!(
            client
                .get_app_config()
                .await
                .expect("config should read back")
                .language,
            I18nLanguage::Korean,
            "a degraded effect must not roll back committed state"
        );
    });
}

#[test]
fn unchanged_patch_dispatches_no_effects() {
    let dir = tempdir().expect("tempdir should be created");
    let port = RecordingEffectsPort::healthy();
    let client = client_with(&dir, port.clone(), Arc::new(StubEndpoint::default()));

    tauri::async_runtime::block_on(async {
        let current = client
            .get_app_config()
            .await
            .expect("config should read back")
            .language;
        let outcome = client
            .patch_app_config(language_patch(current))
            .await
            .expect("a no-op patch should succeed");

        assert!(matches!(outcome, MutationOutcome::Applied { .. }));
        assert_eq!(
            port.dispatch_count(),
            0,
            "an empty plan is not dispatched at all"
        );
    });
}

#[test]
fn revisions_are_allocated_in_commit_order() {
    let dir = tempdir().expect("tempdir should be created");
    let port = RecordingEffectsPort::healthy();
    let client = client_with(&dir, port.clone(), Arc::new(StubEndpoint::default()));

    tauri::async_runtime::block_on(async {
        client
            .patch_app_config(language_patch(I18nLanguage::Korean))
            .await
            .expect("first patch should commit");
        client
            .patch_app_config(language_patch(I18nLanguage::Russian))
            .await
            .expect("second patch should commit");

        let revisions = port.revisions();
        assert_eq!(revisions.len(), 2);
        assert!(
            revisions[0] < revisions[1],
            "revision order must follow commit order: {revisions:?}"
        );
    });
}

#[test]
fn stale_reconcile_does_not_overwrite_newer_state() {
    let dir = tempdir().expect("tempdir should be created");
    let port = RecordingEffectsPort::healthy();
    let client = client_with(&dir, port.clone(), Arc::new(StubEndpoint::default()));

    tauri::async_runtime::block_on(async {
        client
            .patch_app_config(language_patch(I18nLanguage::Korean))
            .await
            .expect("first patch should commit");
        client
            .patch_app_config(language_patch(I18nLanguage::Russian))
            .await
            .expect("second patch should commit");

        // Owners resolve conflicts by revision, so the newest revision has to
        // carry the newest desired value for that rule to converge.
        let newest = port.newest_plan();
        assert!(
            newest.effects().iter().any(|effect| matches!(
                effect,
                ApplicationEffect::Locale(language) if *language == I18nLanguage::Russian
            )),
            "newest revision must carry the last committed value: {newest:?}"
        );
        assert_eq!(
            client
                .get_app_config()
                .await
                .expect("config should read back")
                .language,
            I18nLanguage::Russian
        );
    });
}

#[test]
fn mixed_port_change_requests_rebuild() {
    let dir = tempdir().expect("tempdir should be created");
    let endpoint = Arc::new(StubEndpoint::default());
    let client = client_with(&dir, RecordingEffectsPort::healthy(), endpoint.clone());

    tauri::async_runtime::block_on(async {
        let current = client
            .get_clash_config()
            .await
            .expect("clash config should read back")
            .mixed_port;
        let mut patch = ClashConfig::new_empty_patch();
        patch.mixed_port = Some(PortStrategy::new_allow_fallback(current.start_port + 1));
        let submissions_before = endpoint.submissions.load(Ordering::SeqCst);

        client
            .patch_clash_config(patch)
            .await
            .expect("mixed port patch should commit");

        assert!(
            endpoint.submissions.load(Ordering::SeqCst) > submissions_before,
            "a mixed-port change has to rebuild the running config"
        );
    });
}

#[test]
fn control_channel_change_prefers_apply_control_channel() {
    let dir = tempdir().expect("tempdir should be created");
    let endpoint = Arc::new(StubEndpoint::default());
    let client = client_with(&dir, RecordingEffectsPort::healthy(), endpoint.clone());

    tauri::async_runtime::block_on(async {
        let current = client
            .get_clash_config()
            .await
            .expect("clash config should read back")
            .clash_control_channel;
        let mut patch = ClashConfig::new_empty_patch();
        patch.clash_control_channel = Some(match current {
            ClashControlChannel::PreferIpc => ClashControlChannel::HttpOnly,
            ClashControlChannel::HttpOnly => ClashControlChannel::PreferIpc,
        });
        let submissions_before = endpoint.submissions.load(Ordering::SeqCst);
        let statuses_before = endpoint.status_queries.load(Ordering::SeqCst);

        client
            .patch_clash_config(patch)
            .await
            .expect("control channel patch should commit");

        assert!(
            endpoint.status_queries.load(Ordering::SeqCst) > statuses_before,
            "the control-channel apply inspects the core status"
        );
        assert_eq!(
            endpoint.submissions.load(Ordering::SeqCst),
            submissions_before,
            "a stopped core must not be reconciled by a control-channel change"
        );
    });
}

#[test]
fn session_state_patch_produces_empty_plan() {
    let dir = tempdir().expect("tempdir should be created");
    let port = RecordingEffectsPort::healthy();
    let endpoint = Arc::new(StubEndpoint::default());
    let client = client_with(&dir, port.clone(), endpoint.clone());

    tauri::async_runtime::block_on(async {
        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(
            [(
                WindowLabel("main".to_owned()),
                WindowState {
                    width: 1440,
                    height: 900,
                    x: 1,
                    y: 2,
                    maximized: false,
                    fullscreen: false,
                },
            )]
            .into_iter()
            .collect(),
        );
        let submissions_before = endpoint.submissions.load(Ordering::SeqCst);

        let outcome = client
            .patch_session_state(patch)
            .await
            .expect("session state patch should commit");

        assert!(matches!(outcome, MutationOutcome::Applied { .. }));
        assert_eq!(
            port.dispatch_count(),
            0,
            "session state carries no effect field"
        );
        assert_eq!(
            endpoint.submissions.load(Ordering::SeqCst),
            submissions_before,
            "session state must never trigger a runtime apply"
        );
    });
}

#[test]
fn runtime_rebuild_failure_degrades_without_erasing_the_commit() {
    /// Refuses every submission so the post-commit rebuild fails.
    struct RefusingEndpoint;

    #[async_trait::async_trait]
    impl ControlEndpoint for RefusingEndpoint {
        fn host(&self) -> ExecutionHost {
            ExecutionHost::Local
        }

        async fn submit(
            &self,
            _submission: CoreSubmission,
        ) -> std::result::Result<
            nyanpasu_ipc::api::core::v2::OperationInfo,
            nyanpasu_core_manager::CoreError,
        > {
            Err(nyanpasu_core_manager::CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::Internal,
                "injected submit failure",
                false,
            ))
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
        ) -> std::result::Result<CoreStatusSnapshot, nyanpasu_core_manager::CoreError> {
            Ok(CoreStatusSnapshot {
                controller: None,
                state: Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
                state_changed_at: 0,
                revision: None,
                healthy: Some(true),
                applied_kind: None,
            })
        }
    }

    let dir = tempdir().expect("tempdir should be created");
    let mut args = test_client_args_with_endpoint(&dir, Arc::new(RefusingEndpoint));
    args.effects = RecordingEffectsPort::healthy();
    let client = NyanpasuClient::try_new_with_args(args).expect("client should construct");

    tauri::async_runtime::block_on(async {
        let current = client
            .get_clash_config()
            .await
            .expect("clash config should read back")
            .mixed_port;
        let mut patch = ClashConfig::new_empty_patch();
        patch.mixed_port = Some(PortStrategy::new_allow_fallback(current.start_port + 1));

        let outcome = client
            .patch_clash_config(patch)
            .await
            .expect("a failed rebuild is a degradation, not a commit failure");

        let degradations = degradations(&outcome);
        assert!(
            degradations.iter().any(|degradation| {
                degradation.phase == DegradationPhase::RuntimeBuild
                    && degradation.code == "runtime_rebuild_failed"
            }),
            "expected a runtime build degradation: {degradations:?}"
        );
        assert_eq!(
            client
                .get_clash_config()
                .await
                .expect("clash config should read back")
                .mixed_port
                .start_port,
            current.start_port + 1,
            "the commit stands even though the rebuild failed"
        );
    });
}

#[test]
fn degraded_effect_is_retried_on_identical_resubmission() {
    let dir = tempdir().expect("tempdir should be created");
    let port = FlakyEffectsPort::new(EffectKind::Locale, true, 1);
    let client = client_with(&dir, port.clone(), Arc::new(StubEndpoint::default()));

    tauri::async_runtime::block_on(async {
        let outcome = client
            .patch_app_config(language_patch(I18nLanguage::Korean))
            .await
            .expect("a post-commit effect failure is not a commit failure");
        assert!(
            matches!(outcome, MutationOutcome::CommittedDegraded { .. }),
            "the failing effect must be reported: {outcome:?}"
        );

        // The same value again: the diff is empty, so only the retry can put the
        // locale back on the wire.
        let outcome = client
            .patch_app_config(language_patch(I18nLanguage::Korean))
            .await
            .expect("resubmitting the committed value should succeed");
        assert!(
            matches!(outcome, MutationOutcome::Applied { .. }),
            "the retry succeeded, so nothing is degraded: {outcome:?}"
        );
        assert_eq!(port.dispatch_count(), 2, "the retry has to be dispatched");
        let retried = port.plan(1);
        assert_eq!(
            retried.effects(),
            [ApplicationEffect::Locale(I18nLanguage::Korean)],
            "the retry carries the full desired value of the failed kind only"
        );

        // Once it succeeded the kind leaves the retry set, so an unchanged patch
        // is a no-op again.
        client
            .patch_app_config(language_patch(I18nLanguage::Korean))
            .await
            .expect("a no-op patch should succeed");
        assert_eq!(
            port.dispatch_count(),
            2,
            "a healed effect must not be retried forever"
        );
    });
}

#[test]
fn non_retryable_degradation_is_not_retried() {
    let dir = tempdir().expect("tempdir should be created");
    let port = FlakyEffectsPort::new(EffectKind::Locale, false, 1);
    let client = client_with(&dir, port.clone(), Arc::new(StubEndpoint::default()));

    tauri::async_runtime::block_on(async {
        let outcome = client
            .patch_app_config(language_patch(I18nLanguage::Korean))
            .await
            .expect("a post-commit effect failure is not a commit failure");
        let degradations = degradations(&outcome);
        assert!(
            degradations
                .iter()
                .any(|degradation| !degradation.retryable),
            "expected a non-retryable degradation: {degradations:?}"
        );

        client
            .patch_app_config(language_patch(I18nLanguage::Korean))
            .await
            .expect("resubmitting the committed value should succeed");

        assert_eq!(
            port.dispatch_count(),
            1,
            "an effect that asked not to be retried must not be re-dispatched"
        );
    });
}
