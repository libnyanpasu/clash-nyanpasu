//! Committed application changes converge independently of source admission.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use nyanpasu_config::application::{ClashCore, I18nLanguage, NyanpasuAppConfigPatch};
use nyanpasu_contracts::{
    elevation::{ElevationConsentError, ElevationOperation},
    service::{ServiceCommand, ServiceCommandError, ServiceProcessControl},
};
use nyanpasu_runtime_values::{
    execution_host::ExecutionHost,
    mutation::{CommitAborted, ConfigDomain},
    runtime::{DegradationReason, MutationOutcome, RuntimeCommitStatus},
    runtime_error::RuntimeError,
};
use tokio::sync::Notify;

use super::{BlockingBuilder, ScriptedWaitEndpoint, WaitScript};
use crate::{
    actor_v2::{
        endpoint::{ControlEndpoint, EndpointHandle},
        service_actor::{ServiceClient, ServiceEndpointFactory},
        test_support::HostTransitionServiceAdapter,
    },
    client::{
        ClientError, NyanpasuClient,
        application_workflow::Conclusion,
        test_support::{self, TestControlEndpoint, runtime_build::TestRuntimeBuildPort},
    },
    state::config_error::ConfigError,
};

pub(super) fn receipt(
    outcome: &MutationOutcome<()>,
) -> &nyanpasu_runtime_values::runtime::CommitReceipt {
    match outcome {
        MutationOutcome::Committed { commits, .. }
        | MutationOutcome::CommittedDegraded { commits, .. } => &commits[0],
    }
}

fn switched_core(client: &NyanpasuClient) -> NyanpasuAppConfigPatch {
    let current = client.inner.application.snapshot().state.core;
    NyanpasuAppConfigPatch {
        core: Some(if current == ClashCore::ClashRs {
            ClashCore::Mihomo
        } else {
            ClashCore::ClashRs
        }),
        ..Default::default()
    }
}

pub(super) async fn client_with_scripted_endpoint(
    dir: &tempfile::TempDir,
) -> (
    NyanpasuClient,
    Arc<TestControlEndpoint>,
    Arc<ScriptedWaitEndpoint>,
) {
    let endpoint = TestControlEndpoint::succeeding();
    let scripted = ScriptedWaitEndpoint::new(endpoint.clone());
    let args = test_support::test_client_args_with_endpoint(
        dir,
        scripted.clone(),
        test_support::profile_test_adapters(dir),
    )
    .await;
    let client = NyanpasuClient::try_new(args).await.unwrap();
    endpoint.prime(&client).await;
    (client, endpoint, scripted)
}

// Guards SourceCommitted mapping apply refusal to a committed Deferred receipt.
#[tokio::test]
async fn rejected_core_switch_keeps_the_new_source_and_the_old_revision() {
    let dir = tempfile::tempdir().unwrap();
    let (client, endpoint, _) = client_with_scripted_endpoint(&dir).await;
    let previous = endpoint.status().await.unwrap().revision;
    let patch = switched_core(&client);
    let selected = patch.core.unwrap();
    endpoint.set_rolls_back(true);
    let outcome = client.patch_app_config(patch).await.unwrap();
    assert!(matches!(outcome, MutationOutcome::CommittedDegraded { .. }));
    assert_eq!(receipt(&outcome).runtime, RuntimeCommitStatus::Deferred);
    assert_eq!(client.get_app_config().await.unwrap().core, selected);
    assert_eq!(endpoint.status().await.unwrap().revision, previous);
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards CandidateCheck propagating build failure before the source commit.
#[tokio::test]
async fn invalid_core_candidate_never_commits_or_submits() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let mut args = test_support::test_client_args_with_endpoint(
        &dir,
        endpoint.clone(),
        test_support::profile_test_adapters(&dir),
    )
    .await;
    let builder = Arc::new(BlockingBuilder {
        delegate: TestRuntimeBuildPort {
            paths: args.runtime_paths.clone(),
            capture: Arc::new(|_| Default::default()),
        },
        calls: AtomicUsize::new(1),
        entered: Notify::new(),
        release: Notify::new(),
        fail: AtomicBool::new(false),
    });
    args.host_adapters.runtime_build = builder.clone();
    let client = NyanpasuClient::try_new(args).await.unwrap();
    endpoint.prime(&client).await;
    let before = client.inner.application.snapshot();
    builder.fail.store(true, Ordering::SeqCst);
    assert!(matches!(
        client.patch_app_config(switched_core(&client)).await,
        Err(ClientError::Config(ConfigError::Commit {
            source: CommitAborted::RuntimeRefused { .. },
            ..
        }))
    ));
    assert_eq!(client.inner.application.snapshot().version, before.version);
    assert_eq!(
        client.get_app_config().await.unwrap().core,
        before.state.core
    );
    assert!(endpoint.reconciled_bytes().is_empty());
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards retaining preparation failures as blocked targets that an explicit retry can apply.
#[tokio::test]
async fn committed_rebuild_failure_stays_blocked_until_explicit_retry() {
    use nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch;
    use nyanpasu_runtime_values::{configuration::ConvergenceHealth, runtime::DegradationReason};

    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    let scripted = ScriptedWaitEndpoint::new(endpoint.clone());
    let mut args = test_support::test_client_args_with_endpoint(
        &dir,
        scripted.clone(),
        test_support::profile_test_adapters(&dir),
    )
    .await;
    let builder = Arc::new(BlockingBuilder {
        delegate: TestRuntimeBuildPort {
            paths: args.runtime_paths.clone(),
            capture: Arc::new(|_| Default::default()),
        },
        calls: AtomicUsize::new(1),
        entered: Notify::new(),
        release: Notify::new(),
        fail: AtomicBool::new(false),
    });
    args.host_adapters.runtime_build = builder.clone();
    let client = NyanpasuClient::try_new(args).await.unwrap();
    endpoint.prime(&client).await;
    scripted.queue(WaitScript::Held);
    let applying = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .patch_runtime_overrides(ClashGuardOverridesPatch {
                    ipv6: Some(true),
                    ..Default::default()
                })
                .await
        }
    });
    scripted.held().await;
    let submitted = endpoint.reconciled_bytes().len();
    let mut settings = client.inner.application.subscribe_settings_changes();
    settings.borrow_and_update();
    builder.calls.store(0, Ordering::SeqCst);
    let saving = tokio::spawn({
        let client = client.clone();
        async move { client.patch_app_config(switched_core(&client)).await }
    });
    builder.entered.notified().await;

    // Each check captures the other domain before its later commit, so neither
    // checked candidate describes the combined target the workflow will read.
    client
        .inner
        .clash_config
        .patch_overrides(serde_json::from_value(serde_json::json!({"allow-lan": true})).unwrap())
        .await
        .unwrap();
    builder.release.notify_one();
    settings.changed().await.unwrap();
    assert_eq!(endpoint.checked().len(), 3);
    builder.fail.store(true, Ordering::SeqCst);
    scripted.release();
    applying.await.unwrap().unwrap();
    let saved = saving.await.unwrap().unwrap();
    assert_eq!(receipt(&saved).runtime, RuntimeCommitStatus::Deferred);
    assert!(matches!(
        &saved.degradations()[0].reason,
        DegradationReason::RuntimeDeferred { cause }
            if matches!(cause.as_ref(), RuntimeError::BuildRuntime { .. })
    ));
    super::barrier(&client.inner.application_workflow).await;
    assert_eq!(endpoint.reconciled_bytes().len(), submitted);
    let status = client.configuration_status();
    assert!(status.active.is_none());
    assert_eq!(status.runtime.health, ConvergenceHealth::Blocked);
    assert_eq!(
        status.runtime.message.as_deref(),
        Some(saved.degradations()[0].message.as_str())
    );

    builder.fail.store(false, Ordering::SeqCst);
    client.retry_runtime_now().await.unwrap();
    assert_eq!(endpoint.reconciled_bytes().len(), submitted + 1);
    assert_eq!(endpoint.checked().len(), 3);
    let applied: yaml_serde::Value =
        yaml_serde::from_slice(endpoint.reconciled_bytes().last().unwrap()).unwrap();
    assert_eq!(applied["ipv6"].as_bool(), Some(true));
    assert_eq!(applied["allow-lan"].as_bool(), Some(true));
    assert_eq!(
        client
            .inner
            .application_workflow
            .0
            .runtime
            .last_confirmed_runtime_receipt()
            .unwrap()
            .target_core,
        client.get_app_config().await.unwrap().core
    );
    assert_eq!(
        client.configuration_status().runtime.health,
        ConvergenceHealth::Healthy
    );
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards settled's watch-closed branch, including an already closed receiver.
#[tokio::test]
async fn settled_after_workflow_stop_reports_closed() {
    let dir = tempfile::tempdir().unwrap();
    let (client, _, _) = client_with_scripted_endpoint(&dir).await;
    let workflow = client.inner.application_workflow.clone();
    client.request_shutdown();
    client.wait_shutdown().await;
    let status = workflow
        .settled(
            ConfigDomain::Application,
            client.inner.application.snapshot().version,
        )
        .await;
    assert!(matches!(status.conclusion, Conclusion::Closed));
}

// Guards publishing seen and Unknown even when an earlier action is unresolved.
#[tokio::test]
async fn a_commit_behind_an_unresolved_action_settles_as_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let (client, _, scripted) = client_with_scripted_endpoint(&dir).await;
    scripted.queue(WaitScript::Missing);
    let first = client
        .patch_app_config(switched_core(&client))
        .await
        .unwrap();
    assert_eq!(receipt(&first).runtime, RuntimeCommitStatus::Deferred);
    let patch = NyanpasuAppConfigPatch {
        enable_builtin_enhanced: Some(
            !client
                .get_app_config()
                .await
                .unwrap()
                .enable_builtin_enhanced,
        ),
        ..Default::default()
    };
    let enabled = patch.enable_builtin_enhanced.unwrap();
    let outcome = client.patch_app_config(patch).await.unwrap();
    assert_eq!(receipt(&outcome).runtime, RuntimeCommitStatus::Deferred);
    assert!(matches!(
        &outcome.degradations()[0].reason,
        DegradationReason::RuntimeDeferred { cause }
            if matches!(cause.as_ref(), RuntimeError::ConfigurationNotInEffect { .. })
    ));
    let committed = client.inner.application.snapshot();
    assert_eq!(committed.state.enable_builtin_enhanced, enabled);
    let status = client
        .inner
        .application_workflow
        .0
        .reconciler
        .borrow()
        .clone();
    assert!(matches!(status.conclusion, Conclusion::Unknown { .. }));
    assert_eq!(status.seen.application, committed.version);
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards checking outside the workflow mailbox and bypassing settled for non-runtime saves.
#[tokio::test]
async fn language_save_finishes_while_an_application_is_held() {
    let dir = tempfile::tempdir().unwrap();
    let (client, endpoint, scripted) = client_with_scripted_endpoint(&dir).await;
    scripted.queue(WaitScript::Held);
    let applying = tokio::spawn({
        let client = client.clone();
        async move { client.patch_app_config(switched_core(&client)).await }
    });
    scripted.held().await;
    let language = if client.get_app_config().await.unwrap().language == I18nLanguage::English {
        I18nLanguage::Korean
    } else {
        I18nLanguage::English
    };
    let saved = client
        .patch_app_config(NyanpasuAppConfigPatch {
            language: Some(language),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(receipt(&saved).runtime, RuntimeCommitStatus::Unchanged);
    assert_eq!(client.get_app_config().await.unwrap().language, language);
    assert!(!applying.is_finished());
    scripted.release();
    let applied = applying.await.unwrap().unwrap();
    assert_eq!(receipt(&applied).runtime, RuntimeCommitStatus::Applied);
    assert_eq!(endpoint.checked().len(), 1);
    client.request_shutdown();
    client.wait_shutdown().await;
}

struct PreparingDaemon {
    delegate: HostTransitionServiceAdapter,
    installed: AtomicBool,
    refuse_install: AtomicBool,
    app_path: std::path::PathBuf,
}
impl PreparingDaemon {
    fn assert_preference_uncommitted(&self) {
        let bytes = std::fs::read(&self.app_path).unwrap();
        let config: yaml_serde::Value = yaml_serde::from_slice(&bytes).unwrap();
        assert_ne!(
            config
                .get("enable_service_mode")
                .and_then(yaml_serde::Value::as_bool),
            Some(true)
        );
    }
}
#[async_trait::async_trait]
impl ServiceProcessControl for PreparingDaemon {
    async fn probe(&self) -> Result<String, ServiceCommandError> {
        if self.installed.load(Ordering::SeqCst) {
            self.delegate.probe().await
        } else {
            Ok(serde_json::to_string(&nyanpasu_ipc::types::StatusInfo {
                name: "test-service".into(),
                version: "2.0.3".into(),
                status: nyanpasu_ipc::types::ServiceStatus::NotInstalled,
                server: None,
            })
            .unwrap())
        }
    }
    async fn install(&self) -> Result<(), ServiceCommandError> {
        self.assert_preference_uncommitted();
        if self.refuse_install.load(Ordering::SeqCst) {
            let command = ServiceCommand::Install;
            return Err(ServiceCommandError::AuthorizeServiceCommand {
                command,
                source: ElevationConsentError::AuthorizeElevation {
                    operation: ElevationOperation::Service { command },
                },
            });
        }
        self.delegate.install().await?;
        self.installed.store(true, Ordering::SeqCst);
        Ok(())
    }
    async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
        self.assert_preference_uncommitted();
        self.delegate.start_daemon().await
    }
    async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
        self.delegate.stop_daemon().await
    }
    async fn uninstall(&self) -> Result<(), ServiceCommandError> {
        self.delegate.uninstall().await
    }
    async fn update(&self) -> Result<(), ServiceCommandError> {
        self.delegate.update().await
    }
}
impl ServiceEndpointFactory for PreparingDaemon {
    fn endpoint(&self) -> EndpointHandle {
        self.delegate.endpoint()
    }
}

async fn service_client(
    dir: &tempfile::TempDir,
) -> (NyanpasuClient, Arc<PreparingDaemon>, Arc<BlockingBuilder>) {
    let local = TestControlEndpoint::succeeding();
    let remote = TestControlEndpoint::succeeding_on(ExecutionHost::Service);
    remote.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
        None,
    );
    let mut args = test_support::test_client_args_with_endpoint(
        dir,
        local.clone(),
        test_support::profile_test_adapters(dir),
    )
    .await;
    let daemon = Arc::new(PreparingDaemon {
        delegate: HostTransitionServiceAdapter {
            endpoint: remote,
            calls: Default::default(),
            stopped: AtomicBool::new(true),
            installed_for: args.paths.app_config_dir().as_std_path().to_owned(),
        },
        installed: AtomicBool::new(false),
        refuse_install: AtomicBool::new(false),
        app_path: args.paths.application_config_path().into_std_path_buf(),
    });
    args.service = ServiceClient::spawn(daemon.clone(), daemon.clone(), 0)
        .await
        .unwrap();
    let builder = Arc::new(BlockingBuilder {
        delegate: TestRuntimeBuildPort {
            paths: args.runtime_paths.clone(),
            capture: Arc::new(|_| Default::default()),
        },
        calls: AtomicUsize::new(1),
        entered: Notify::new(),
        release: Notify::new(),
        fail: AtomicBool::new(false),
    });
    args.host_adapters.runtime_build = builder.clone();
    let client = NyanpasuClient::try_new(args).await.unwrap();
    local.prime(&client).await;
    use nyanpasu_persistence::format::Format as _;
    let mut bytes = Vec::new();
    nyanpasu_migration::modules::application::ApplicationFormat::default()
        .serialize(&mut bytes, &client.get_app_config().await.unwrap(), None)
        .unwrap();
    std::fs::write(&daemon.app_path, bytes).unwrap();
    (client, daemon, builder)
}

// Guards PrepareService error propagation before patching the preference.
#[tokio::test]
async fn refusing_elevation_keeps_service_preference_uncommitted() {
    let dir = tempfile::tempdir().unwrap();
    let (client, daemon, _) = service_client(&dir).await;
    daemon.refuse_install.store(true, Ordering::SeqCst);
    let before = client.inner.application.snapshot().version;
    let error = client.set_service_mode(true).await.unwrap_err();
    let ClientError::Config(ConfigError::Commit {
        source: CommitAborted::RuntimeRefused { errors, .. },
        ..
    }) = error
    else {
        panic!("{error:?}")
    };
    assert!(matches!(
        errors[0].as_ref(),
        RuntimeError::MoveHost {
            host: ExecutionHost::Service,
            ..
        }
    ));
    assert_eq!(client.inner.application.snapshot().version, before);
    assert!(!client.get_app_config().await.unwrap().enable_service_mode);
    assert!(!daemon.installed.load(Ordering::SeqCst));
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards preparing the daemon before independent validation and reusing the checked candidate.
#[tokio::test]
async fn service_preparation_precedes_commit_and_application_uses_the_checked_build() {
    let dir = tempfile::tempdir().unwrap();
    let (client, daemon, builder) = service_client(&dir).await;
    let builds = builder.calls.load(Ordering::SeqCst);
    let outcome = client.set_service_mode(true).await.unwrap();
    assert_eq!(receipt(&outcome).runtime, RuntimeCommitStatus::Applied);
    assert!(client.get_app_config().await.unwrap().enable_service_mode);
    assert_eq!(client.inner.core_api.status().host, ExecutionHost::Service);
    assert_eq!(builder.calls.load(Ordering::SeqCst), builds + 1);
    assert!(daemon.installed.load(Ordering::SeqCst));
    assert!(!daemon.delegate.stopped.load(Ordering::SeqCst));
    let calls = daemon.delegate.calls.lock().unwrap().clone();
    assert!(
        calls.iter().position(|call| *call == "install").unwrap()
            < calls
                .iter()
                .position(|call| *call == "start_daemon")
                .unwrap()
    );
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards leaving a prepared daemon running when the candidate cannot be committed.
#[tokio::test]
async fn invalid_candidate_after_service_preparation_keeps_daemon_ready() {
    let dir = tempfile::tempdir().unwrap();
    let (client, daemon, builder) = service_client(&dir).await;
    let before = client.inner.application.snapshot().version;
    builder.fail.store(true, Ordering::SeqCst);
    assert!(matches!(
        client.set_service_mode(true).await,
        Err(ClientError::Config(ConfigError::Commit {
            source: CommitAborted::RuntimeRefused { .. },
            ..
        }))
    ));
    assert_eq!(client.inner.application.snapshot().version, before);
    assert!(!client.get_app_config().await.unwrap().enable_service_mode);
    assert!(daemon.installed.load(Ordering::SeqCst));
    assert!(!daemon.delegate.stopped.load(Ordering::SeqCst));
    assert_eq!(client.inner.core_api.status().host, ExecutionHost::Local);
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards release_owed maintenance even when no runtime application is needed.
#[tokio::test]
async fn leaving_service_mode_releases_daemon_for_running_and_user_stopped_cores() {
    for stop_core in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (client, daemon, _) = service_client(&dir).await;
        client.set_service_mode(true).await.unwrap();
        if stop_core {
            client.inner.application_workflow.stop_core().await.unwrap();
        }
        let outcome = client.set_service_mode(false).await.unwrap();
        assert_eq!(
            receipt(&outcome).runtime,
            if stop_core {
                RuntimeCommitStatus::SavedInactive
            } else {
                RuntimeCommitStatus::Applied
            }
        );
        assert_eq!(client.inner.core_api.status().host, ExecutionHost::Local);
        assert!(daemon.delegate.stopped.load(Ordering::SeqCst));
        client.request_shutdown();
        client.wait_shutdown().await;
    }
}

// Guards saving Clash while observation is transitioning, then applying the committed target on retry.
#[tokio::test]
async fn port_save_during_transition_commits_and_retry_applies_it() {
    use nyanpasu_ipc::api::status::CoreStateDetail;
    let dir = tempfile::tempdir().unwrap();
    let (client, endpoint, _) = client_with_scripted_endpoint(&dir).await;
    let previous = endpoint.status().await.unwrap();
    endpoint.set_status(
        Some(CoreStateDetail::Starting { epoch: 2 }),
        previous.applied_kind,
    );
    let port = client
        .get_clash_config()
        .await
        .unwrap()
        .mixed_port
        .start_port
        + 1;
    let patch =
        serde_json::from_value(serde_json::json!({"mixed_port": {"start_port": port}})).unwrap();
    let saved = client.patch_clash_config(patch).await.unwrap();
    assert_eq!(receipt(&saved).runtime, RuntimeCommitStatus::Deferred);
    assert!(matches!(saved, MutationOutcome::CommittedDegraded { .. }));
    assert_eq!(
        client
            .get_clash_config()
            .await
            .unwrap()
            .mixed_port
            .start_port,
        port
    );
    let path = client.inner.paths.clash_config_path();
    let persisted: nyanpasu_config::clash::config::ClashConfig =
        yaml_serde::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(persisted.mixed_port.start_port, port);
    assert!(endpoint.reconciled_bytes().is_empty());
    endpoint.set_status(previous.state, previous.applied_kind);
    client
        .inner
        .application_workflow
        .retry_runtime()
        .await
        .unwrap();
    assert_eq!(endpoint.reconciled_bytes().len(), 1);
    let applied: yaml_serde::Value =
        yaml_serde::from_slice(&endpoint.reconciled_bytes()[0]).unwrap();
    assert_eq!(applied["mixed-port"].as_u64(), Some(u64::from(port)));
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards RuntimeGate checking outside the workflow mailbox and publication before settled waits.
#[tokio::test]
async fn application_runtime_save_commits_while_clash_application_is_held() {
    let dir = tempfile::tempdir().unwrap();
    let (client, endpoint, scripted) = client_with_scripted_endpoint(&dir).await;
    scripted.queue(WaitScript::Held);
    let applying = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .patch_runtime_overrides(
                    nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch {
                        ipv6: Some(true),
                        ..Default::default()
                    },
                )
                .await
        }
    });
    scripted.held().await;
    let before = client.inner.application.snapshot();
    let enabled = !before.state.enable_builtin_enhanced;
    let mut settings = client.inner.application.subscribe_settings_changes();
    settings.borrow_and_update();
    let saving = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .patch_app_config(NyanpasuAppConfigPatch {
                    enable_builtin_enhanced: Some(enabled),
                    ..Default::default()
                })
                .await
        }
    });
    settings.changed().await.unwrap();
    let published = client.inner.application.snapshot();
    assert!(published.version.as_ref() > before.version.as_ref());
    assert_eq!(published.state.enable_builtin_enhanced, enabled);
    let persisted: nyanpasu_config::application::NyanpasuAppConfig = yaml_serde::from_slice(
        &std::fs::read(client.inner.paths.application_config_path()).unwrap(),
    )
    .unwrap();
    assert_eq!(persisted.enable_builtin_enhanced, enabled);
    assert!(!applying.is_finished());
    assert!(!saving.is_finished());
    assert_eq!(endpoint.checked().len(), 2);
    scripted.release();
    assert_eq!(
        receipt(&applying.await.unwrap().unwrap()).runtime,
        RuntimeCommitStatus::Applied
    );
    assert_eq!(
        receipt(&saving.await.unwrap().unwrap()).runtime,
        RuntimeCommitStatus::Applied
    );
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards the owners comparing runtime projections before calling the gate or notifying the workflow.
#[tokio::test]
async fn unchanged_runtime_fields_do_not_check_or_apply() {
    let dir = tempfile::tempdir().unwrap();
    let (client, endpoint, _) = client_with_scripted_endpoint(&dir).await;
    let application = client.get_app_config().await.unwrap();
    let clash = client.get_clash_config().await.unwrap();
    let before = client.inner.application_workflow.0.reconciler.borrow().seq;
    let saved = client
        .patch_app_config(NyanpasuAppConfigPatch {
            core: Some(application.core),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(receipt(&saved).runtime, RuntimeCommitStatus::Unchanged);
    let patch = serde_json::from_value(serde_json::json!({
        "mixed_port": { "start_port": clash.mixed_port.start_port }
    }))
    .unwrap();
    let saved = client.patch_clash_config(patch).await.unwrap();
    assert_eq!(receipt(&saved).runtime, RuntimeCommitStatus::Unchanged);
    let current = client.get_profiles().await.unwrap().current.clone();
    let saved = client.activate_profile(current).await.unwrap();
    assert_eq!(receipt(&saved).runtime, RuntimeCommitStatus::Unchanged);
    super::barrier(&client.inner.application_workflow).await;
    assert_eq!(
        client.inner.application_workflow.0.reconciler.borrow().seq,
        before
    );
    assert!(endpoint.checked().is_empty());
    assert!(endpoint.reconciled_bytes().is_empty());
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards projecting round labels and causes through the existing OperationStatus wire fields.
#[tokio::test]
async fn recent_operations_describe_reconcile_rounds() {
    let dir = tempfile::tempdir().unwrap();
    let (client, endpoint, _) = client_with_scripted_endpoint(&dir).await;
    endpoint.set_rolls_back(true);
    let outcome = client
        .patch_app_config(switched_core(&client))
        .await
        .unwrap();
    let status = client.configuration_status();
    let round = &status.recent_operations[0];
    assert_eq!(round.domain, "SourceCommitted");
    assert_eq!(round.outcome, "NotInEffect");
    assert!(
        serde_json::to_value(round)
            .unwrap()
            .get("conclusion")
            .is_none()
    );
    let journal = client.inner.application_workflow.reconciler_status();
    let recorded = journal.recent.back().unwrap();
    assert_eq!(round.operation_id, recorded.operation_id.to_string());
    assert!(matches!(
        recorded.cause.as_deref(),
        Some(RuntimeError::CoreRolledBack { .. })
    ));
    assert_eq!(
        round.message,
        recorded.cause.as_ref().map(ToString::to_string)
    );
    assert_ne!(
        Some(&round.operation_id),
        receipt(&outcome).operation_id.as_ref()
    );
    endpoint.set_rolls_back(false);
    client.inner.application_workflow.reconcile().await.unwrap();
    client
        .inner
        .application_workflow
        .settle_core_changes()
        .await;
    let history = client.configuration_status().recent_operations;
    assert_eq!(history.len(), status.recent_operations.len() + 2);
    assert_eq!(history[0].domain, "CoreChanged");
    assert_eq!(history[0].outcome, "Converged");
    assert!(
        serde_json::to_value(&history[0])
            .unwrap()
            .get("conclusion")
            .is_none()
    );
    assert_eq!(history[1].domain, "Start");
    assert_eq!(history[1].outcome, "Converged");
    assert_eq!(history[2].operation_id, round.operation_id);
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards committed saves reaching application without requiring an earlier session receipt.
#[tokio::test]
async fn running_without_a_session_receipt_does_not_refuse_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    endpoint.set_status(
        Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }),
        Some(nyanpasu_core_manager::CoreKind::Mihomo),
    );
    let args = test_support::test_client_args_with_endpoint(
        &dir,
        endpoint.clone(),
        test_support::profile_test_adapters(&dir),
    )
    .await;
    let client = NyanpasuClient::try_new(args).await.unwrap();
    assert!(
        client
            .inner
            .application_workflow
            .0
            .runtime
            .last_confirmed_runtime_receipt()
            .is_none()
    );
    let selected = switched_core(&client);
    let core = selected.core.unwrap();
    let outcome = client.patch_app_config(selected).await.unwrap();
    assert_eq!(receipt(&outcome).runtime, RuntimeCommitStatus::Applied);
    assert_eq!(client.get_app_config().await.unwrap().core, core);
    assert_eq!(endpoint.checked().len(), 1);
    assert_eq!(endpoint.reconciled_bytes().len(), 1);
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards checking each source candidate while applying only the latest target queued behind an application.
#[tokio::test]
async fn queued_commits_apply_only_the_latest_target() {
    use nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch;

    let dir = tempfile::tempdir().unwrap();
    let (client, endpoint, scripted) = client_with_scripted_endpoint(&dir).await;
    scripted.queue(WaitScript::Held);
    let applying = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .patch_runtime_overrides(ClashGuardOverridesPatch {
                    ipv6: Some(true),
                    ..Default::default()
                })
                .await
        }
    });
    scripted.held().await;
    let second = client
        .inner
        .clash_config
        .patch_overrides(serde_json::from_value(serde_json::json!({"allow-lan": true})).unwrap())
        .await
        .unwrap();
    let latest = client
        .inner
        .clash_config
        .patch_overrides(ClashGuardOverridesPatch {
            ipv6: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(latest.version.as_ref() > second.version.as_ref());
    assert_eq!(
        endpoint.checked().len(),
        3,
        "each committed candidate was checked"
    );
    scripted.release();
    applying.await.unwrap().unwrap();
    assert_eq!(
        receipt(&client.clash_outcome(second).await).runtime,
        RuntimeCommitStatus::Applied
    );
    assert_eq!(
        receipt(&client.clash_outcome(latest).await).runtime,
        RuntimeCommitStatus::Applied
    );
    super::barrier(&client.inner.application_workflow).await;
    let applied = endpoint.reconciled_bytes();
    assert_eq!(
        applied.len(),
        2,
        "queued notifications share one latest target application"
    );
    let latest: yaml_serde::Value = yaml_serde::from_slice(applied.last().unwrap()).unwrap();
    assert_eq!(latest["ipv6"].as_bool(), Some(false));
    assert_eq!(latest["allow-lan"].as_bool(), Some(true));
    client.request_shutdown();
    client.wait_shutdown().await;
}

// Guards applying committed targets on startup and retry without a second independent check.
#[tokio::test]
async fn startup_and_retry_do_not_independently_check_committed_targets() {
    use nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch;

    let dir = tempfile::tempdir().unwrap();
    let (client, endpoint, _) = client_with_scripted_endpoint(&dir).await;
    assert!(
        endpoint.checked().is_empty(),
        "startup applies an already committed target"
    );
    endpoint.set_failure(Some("queue_full"));
    let saved = client
        .patch_runtime_overrides(ClashGuardOverridesPatch {
            ipv6: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(receipt(&saved).runtime, RuntimeCommitStatus::Deferred);
    let checked = endpoint.checked();
    assert_eq!(checked.len(), 1);
    endpoint.set_failure(None);
    client.retry_runtime_now().await.unwrap();
    assert_eq!(endpoint.checked().len(), 1);
    assert_eq!(
        endpoint.reconciled_bytes(),
        [checked[0].config_bytes.clone()]
    );
    client.request_shutdown();
    client.wait_shutdown().await;
}
