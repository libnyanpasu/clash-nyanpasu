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
    mutation::{CommitAborted, ConfigDomain, RuntimeAftermath},
    runtime::{MutationOutcome, RuntimeCommitStatus},
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

fn receipt(outcome: &MutationOutcome<()>) -> &nyanpasu_runtime_values::runtime::CommitReceipt {
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

async fn client_with_scripted_endpoint(
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
            source: CommitAborted::RuntimeRefused {
                runtime: RuntimeAftermath::Untouched,
                ..
            },
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
    assert_eq!(
        receipt(&first).runtime,
        RuntimeCommitStatus::RecoveryRequired
    );
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
    let committed = client.inner.application.patch(patch).await.unwrap();
    let status = client
        .inner
        .application_workflow
        .settled(ConfigDomain::Application, committed.version)
        .await;
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
        source:
            CommitAborted::RuntimeRefused {
                errors,
                runtime: RuntimeAftermath::Untouched,
                ..
            },
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
