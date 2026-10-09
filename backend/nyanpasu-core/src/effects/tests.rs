//! Post-commit isolation and coalescing, using explicit actor acknowledgements.
use super::{
    actor::{EffectsArgs, EffectsClient, EffectsSnapshot},
    plan::{ApplicationEffect, ApplicationEffectInputs, ApplicationEffectPlan, TrayRefresh},
    ports::{ApplicationEffectsPort, CommitNotifications},
};
use crate::{
    effects::{
        EffectKind,
        status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus},
    },
    system_proxy::SystemProxyDesired,
};
use nyanpasu_config::{
    application::{I18nLanguage, NyanpasuAppConfig},
    clash::config::ClashConfig,
    runtime::executor::ResolvedPortBindings,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Notify;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[derive(Default)]
struct Port {
    calls: Mutex<Vec<(EffectRevision, Vec<ApplicationEffect>)>>,
    block: Option<EffectKind>,
    entered: Notify,
    release: Notify,
    blocked: AtomicBool,
    fail: AtomicBool,
    fail_kind: Option<EffectKind>,
    retryable: bool,
}
#[async_trait::async_trait]
impl ApplicationEffectsPort for Port {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        self.calls
            .lock()
            .unwrap()
            .push((revision, plan.effects().to_vec()));
        if plan.effects().iter().any(|e| Some(e.kind()) == self.block)
            && !self.blocked.swap(true, Ordering::SeqCst)
        {
            self.entered.notify_one();
            self.release.notified().await;
        }
        plan.effects()
            .iter()
            .map(|e| EffectStatus {
                kind: e.kind(),
                desired_revision: revision,
                applied_revision: revision,
                health: if matches!(e, ApplicationEffect::SystemProxy(desired) if desired.enabled && desired.port.is_none()) {
                    EffectHealth::Degraded { code: EffectFailureCode::SystemProxyPortUnresolved, message: "no binding".into(), retryable: true }
                } else if self.fail.load(Ordering::SeqCst) && self.fail_kind.is_none_or(|kind| kind == e.kind()) {
                    EffectHealth::Degraded {
                        code: EffectFailureCode::LoggerRefreshFailed,
                        message: "failed".into(),
                        retryable: self.retryable,
                    }
                } else {
                    EffectHealth::Healthy
                },
            })
            .collect()
    }
}
fn inputs() -> ApplicationEffectInputs {
    ApplicationEffectInputs::project(&NyanpasuAppConfig::default(), &ClashConfig::default(), None)
}
/// The effects owner's share of the root shutdown.
struct Shutdown {
    token: CancellationToken,
    tasks: TaskTracker,
}
impl Shutdown {
    fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            tasks: TaskTracker::new(),
        }
    }
    fn request(&self) {
        self.token.cancel();
        self.tasks.close();
    }
    async fn run(&self) {
        self.request();
        self.tasks.wait().await;
    }
}
async fn graph(port: Arc<dyn ApplicationEffectsPort>) -> (EffectsClient, Shutdown) {
    let shutdown = Shutdown::new();
    let client = EffectsClient::spawn(
        EffectsArgs {
            port,
            invalidation: None,
            initial: inputs(),
            shutdown: shutdown.token.clone(),
        },
        &shutdown.tasks,
    )
    .await
    .unwrap();
    (client, shutdown)
}
async fn wait(
    client: &EffectsClient,
    predicate: impl Fn(&EffectsSnapshot) -> bool,
) -> EffectsSnapshot {
    let mut status = client.subscribe();
    tokio::time::timeout(Duration::from_secs(5), status.wait_for(predicate))
        .await
        .unwrap()
        .unwrap()
        .clone()
}
#[tokio::test]
async fn blocked_pac_does_not_block_visual_or_hotkey_group() {
    let port = Arc::new(Port {
        block: Some(EffectKind::SystemProxy),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    client.publish_full(None);
    port.entered.notified().await;
    let state = wait(&client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .any(|s| s.kind == EffectKind::Tray && s.health == EffectHealth::Healthy)
            && s.effects
                .iter()
                .map(|e| &e.status)
                .any(|s| s.kind == EffectKind::Hotkeys && s.health == EffectHealth::Healthy)
    })
    .await;
    for kind in [EffectKind::SystemProxy, EffectKind::AutoLaunch] {
        assert!(
            state
                .effects
                .iter()
                .map(|e| &e.status)
                .any(|s| s.kind == kind && s.health == EffectHealth::Pending),
            "{kind:?} shares the held system proxy owner"
        );
    }
    port.release.notify_one();
    wait(&client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .all(|s| s.health == EffectHealth::Healthy)
    })
    .await;
    shutdown.run().await;
}

#[tokio::test]
async fn blocked_gui_does_not_block_proxy_group() {
    let port = Arc::new(Port {
        block: Some(EffectKind::Tray),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    client.publish_full(None);
    port.entered.notified().await;
    wait(&client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .any(|s| s.kind == EffectKind::SystemProxy && s.health == EffectHealth::Healthy)
    })
    .await;
    port.release.notify_one();
    shutdown.run().await;
}

#[tokio::test]
async fn queued_visual_targets_coalesce_and_full_subsumes_part() {
    let port = Arc::new(Port {
        block: Some(EffectKind::Locale),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.application_committed(desired.app.clone(), Vec::new());
    port.entered.notified().await;
    desired.app.language = I18nLanguage::English;
    client.application_committed(desired.app.clone(), Vec::new());
    desired.app.enable_tray_text = !desired.app.enable_tray_text;
    client.application_committed(desired.app.clone(), Vec::new());
    client.barrier().await;
    assert_eq!(port.calls.lock().unwrap().len(), 1);
    port.release.notify_one();
    wait(&client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .all(|s| s.health == EffectHealth::Healthy)
    })
    .await;
    let calls = port.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].1,
        vec![
            ApplicationEffect::Locale(I18nLanguage::English),
            // Widened to the queued rebuild, rendered from the newest view.
            ApplicationEffect::Tray(
                TrayRefresh::Full,
                desired.app.clone(),
                desired.clash.clone()
            )
        ]
    );
    drop(calls);
    shutdown.run().await;
}

/// The newest `kind` effect the owners were handed, once every slice sent so
/// far has been planned and the newest plan for `kind` applied.
async fn last_applied(client: &EffectsClient, port: &Port, kind: EffectKind) -> ApplicationEffect {
    client.barrier().await;
    wait(client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .any(|s| s.kind == kind && s.health == EffectHealth::Healthy)
    })
    .await;
    let applied = port
        .calls
        .lock()
        .unwrap()
        .iter()
        .rev()
        .flat_map(|(_, effects)| effects)
        .find(|effect| effect.kind() == kind)
        .cloned();
    applied.expect("the effect was planned")
}

/// V29: the application and the Runtime each send their own slice, and the
/// two interleave. Each replaces only its own part of what the effects owner
/// holds, so the system proxy, which reads both, gets the newest of each.
#[tokio::test]
async fn interleaved_slices_from_two_owners_each_keep_their_newest_value() {
    let port = Arc::new(Port::default());
    let (client, shutdown) = graph(port.clone()).await;
    let bound = |mixed_port| {
        Some(ResolvedPortBindings {
            mixed_port,
            ..ResolvedPortBindings::default()
        })
    };
    let mut app = inputs().app;
    app.enable_system_proxy = true;
    app.system_proxy_bypass = "first".into();
    client.application_committed(app.clone(), Vec::new());
    client.runtime_bound(bound(7890), false);
    app.system_proxy_bypass = "second".into();
    client.application_committed(app.clone(), Vec::new());
    assert_eq!(
        last_applied(&client, &port, EffectKind::SystemProxy).await,
        ApplicationEffect::SystemProxy(SystemProxyDesired {
            enabled: true,
            bypass: "second".into(),
            port: Some(7890),
            pac_url: None,
        }),
        "the application's slice keeps the ports the Runtime sent"
    );

    client.runtime_bound(bound(7891), false);
    assert_eq!(
        last_applied(&client, &port, EffectKind::SystemProxy).await,
        ApplicationEffect::SystemProxy(SystemProxyDesired {
            enabled: true,
            bypass: "second".into(),
            port: Some(7891),
            pac_url: None,
        }),
        "the Runtime's slice keeps the application's"
    );
    shutdown.run().await;
}

#[tokio::test]
async fn stale_completion_does_not_publish_success_for_new_desired() {
    let port = Arc::new(Port {
        block: Some(EffectKind::SystemProxy),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.system_proxy_bypass = "old".into();
    client.application_committed(desired.app.clone(), Vec::new());
    port.entered.notified().await;
    desired.app.system_proxy_bypass = "new".into();
    client.application_committed(desired.app, Vec::new());
    client.barrier().await;
    let revision = client.snapshot().revision;
    port.fail.store(true, Ordering::SeqCst);
    port.release.notify_one();
    let state = wait(&client, |s| {
        s.effects.iter().map(|e| &e.status).any(|s| {
            s.kind == EffectKind::SystemProxy && matches!(s.health, EffectHealth::Degraded { .. })
        })
    })
    .await;
    let status = state
        .effects
        .iter()
        .map(|e| &e.status)
        .find(|s| s.kind == EffectKind::SystemProxy)
        .unwrap();
    assert_eq!(status.desired_revision.get(), revision);
    assert_eq!(status.applied_revision.get(), 0);
    assert_eq!(port.calls.lock().unwrap().len(), 2);
    shutdown.run().await;
}

#[tokio::test]
async fn independent_graphs_and_shutdown_admission() {
    let a = Arc::new(Port::default());
    let b = Arc::new(Port::default());
    let (left, left_shutdown) = graph(a.clone()).await;
    let (right, right_shutdown) = graph(b.clone()).await;
    left_shutdown.request();
    // Queued before this test yields, so ahead of the drain: only the token
    // keeps it from reaching the owners.
    left.publish_full(None);
    left_shutdown.tasks.wait().await;
    assert!(a.calls.lock().unwrap().is_empty());
    right.publish_full(None);
    wait(&right, |s| {
        s.effects.len() == 10
            && s.effects
                .iter()
                .map(|e| &e.status)
                .all(|s| s.health == EffectHealth::Healthy)
    })
    .await;
    assert_eq!(b.calls.lock().unwrap().len(), 4);
    right_shutdown.run().await;
}

#[tokio::test(start_paused = true)]
async fn automatic_retries_are_bounded_and_manual_probe_does_not_refill_budget() {
    use super::convergence::ConvergenceHealth;
    let port = Arc::new(Port {
        fail: AtomicBool::new(true),
        retryable: true,
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.application_committed(desired.app.clone(), Vec::new());
    wait(&client, |s| {
        s.effects.iter().any(|p| {
            p.status.kind == EffectKind::Locale && p.health == ConvergenceHealth::RetryScheduled
        })
    })
    .await;
    for (delay, remaining) in [(1, 2), (5, 1), (30, 0)] {
        tokio::time::advance(Duration::from_secs(delay)).await;
        wait(&client, |s| {
            s.effects.iter().any(|p| {
                p.status.kind == EffectKind::Locale
                    && p.automatic_remaining == remaining
                    && p.health != ConvergenceHealth::Pending
            })
        })
        .await;
    }
    let exhausted = client.snapshot();
    assert!(
        exhausted
            .effects
            .iter()
            .filter(|p| p.status.kind == EffectKind::Locale)
            .all(|p| p.health == ConvergenceHealth::Blocked && p.attempts == 4)
    );
    let count = port.calls.lock().unwrap().len();
    tokio::time::advance(Duration::from_secs(100)).await;
    client.barrier().await;
    assert_eq!(port.calls.lock().unwrap().len(), count);
    // An unrelated save neither probes Locale nor creates a fresh budget.
    desired.app.system_proxy_bypass = "unrelated".into();
    client.application_committed(desired.app, Vec::new());
    client.barrier().await;
    assert_eq!(
        client
            .snapshot()
            .effects
            .iter()
            .find(|p| p.status.kind == EffectKind::Locale)
            .unwrap()
            .automatic_remaining,
        0
    );
    client.retry_now(EffectKind::Locale).unwrap();
    wait(&client, |s| {
        s.effects.iter().any(|p| {
            p.status.kind == EffectKind::Locale
                && p.attempts == 5
                && p.health == ConvergenceHealth::Blocked
        })
    })
    .await;
    assert_eq!(
        client
            .snapshot()
            .effects
            .iter()
            .find(|p| p.status.kind == EffectKind::Locale)
            .unwrap()
            .automatic_remaining,
        0
    );
    shutdown.run().await;
}

#[tokio::test(start_paused = true)]
async fn stale_retry_callback_keeps_the_replacement_timer_and_shutdown_cancels_it() {
    use super::convergence::ConvergenceHealth;
    let port = Arc::new(Port {
        fail: AtomicBool::new(true),
        fail_kind: Some(EffectKind::Locale),
        retryable: true,
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.application_committed(desired.app, Vec::new());
    wait(&client, |s| {
        s.effects.iter().any(|effect| {
            effect.status.kind == EffectKind::Locale
                && effect.health == ConvergenceHealth::RetryScheduled
        })
    })
    .await;
    let first = client.retry_deadline().await.unwrap();
    tokio::time::advance(Duration::from_millis(500)).await;
    client.retry_now(EffectKind::Locale).unwrap();
    wait(&client, |s| {
        s.effects.iter().any(|effect| {
            effect.status.kind == EffectKind::Locale
                && effect.attempts == 2
                && effect.health == ConvergenceHealth::RetryScheduled
        })
    })
    .await;
    let replacement = client.retry_deadline().await.unwrap();
    assert!(replacement > first);
    let identity = client.retry_timer_id().await;
    client.stale_tick(first);
    client.barrier().await;
    assert_eq!(client.retry_deadline().await, Some(replacement));
    assert_eq!(client.retry_timer_id().await, identity);
    tokio::time::advance(replacement.saturating_duration_since(tokio::time::Instant::now())).await;
    wait(&client, |s| {
        s.effects.iter().any(|effect| {
            effect.status.kind == EffectKind::Locale
                && effect.attempts == 3
                && effect.health == ConvergenceHealth::RetryScheduled
        })
    })
    .await;
    assert!(client.retry_deadline().await.is_some());
    shutdown.run().await;
    let calls = port.calls.lock().unwrap().len();
    tokio::time::advance(Duration::from_secs(60)).await;
    assert_eq!(port.calls.lock().unwrap().len(), calls);
}

#[tokio::test]
async fn missing_binding_waits_without_spending_apply_budget() {
    use super::convergence::ConvergenceHealth;
    let port = Arc::new(Port::default());
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.enable_system_proxy = true;
    desired.app.enable_proxy_guard = true;
    client.application_committed(desired.app, Vec::new());
    let state = wait(&client, |s| {
        s.effects.iter().any(|p| {
            p.status.kind == EffectKind::SystemProxy
                && p.health == ConvergenceHealth::WaitingDependency
        })
    })
    .await;
    let proxy = state
        .effects
        .iter()
        .find(|p| p.status.kind == EffectKind::SystemProxy)
        .unwrap();
    assert_eq!((proxy.attempts, proxy.automatic_remaining), (0, 3));

    client.retry_now(EffectKind::SystemProxy).unwrap();
    client.barrier().await;
    let state = wait(&client, |s| {
        s.effects.iter().any(|p| {
            p.status.kind == EffectKind::SystemProxy
                && p.health == ConvergenceHealth::WaitingDependency
        })
    })
    .await;
    assert_eq!(
        state
            .effects
            .iter()
            .find(|p| p.status.kind == EffectKind::SystemProxy)
            .unwrap()
            .attempts,
        0
    );
    shutdown.run().await;
}

#[tokio::test]
async fn same_named_failed_target_gets_one_probe_without_budget_reset() {
    use super::convergence::ConvergenceHealth;
    let port = Arc::new(Port {
        fail: AtomicBool::new(true),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.application_committed(desired.app.clone(), vec![EffectKind::Locale]);
    wait(&client, |s| {
        s.effects
            .iter()
            .any(|p| p.status.kind == EffectKind::Locale && p.health == ConvergenceHealth::Blocked)
    })
    .await;
    assert_eq!(
        client
            .snapshot()
            .effects
            .iter()
            .find(|p| p.status.kind == EffectKind::Locale)
            .unwrap()
            .attempts,
        1
    );
    client.application_committed(desired.app, vec![EffectKind::Locale]);
    wait(&client, |s| {
        s.effects.iter().any(|p| {
            p.status.kind == EffectKind::Locale
                && p.attempts == 2
                && p.health == ConvergenceHealth::Blocked
        })
    })
    .await;
    shutdown.run().await;
}

/// V20, V26: once the token is cancelled, a group already running is waited
/// for rather than aborted, and nothing new starts, whatever arrives after the
/// cancel.
#[tokio::test]
async fn the_shutdown_awaits_running_groups_and_starts_no_new_one() {
    let port = Arc::new(Port {
        block: Some(EffectKind::SystemProxy),
        ..Default::default()
    });
    let (client, shutdown) = graph(port.clone()).await;
    client.publish_full(None);
    port.entered.notified().await;
    wait(&client, |s| {
        s.effects
            .iter()
            .map(|e| &e.status)
            .any(|s| s.kind == EffectKind::Tray && s.health == EffectHealth::Healthy)
            && s.effects
                .iter()
                .map(|e| &e.status)
                .any(|s| s.kind == EffectKind::Hotkeys && s.health == EffectHealth::Healthy)
    })
    .await;
    let started = port.calls.lock().unwrap().len();

    shutdown.request();
    // Queued before this test yields, so ahead of the drain: only the token
    // keeps them from starting a group.
    let mut desired = inputs();
    desired.app.language = I18nLanguage::Korean;
    client.application_committed(desired.app, Vec::new());
    client.retry_now(EffectKind::Locale).unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), shutdown.tasks.wait())
            .await
            .is_err(),
        "the blocked group is awaited, not aborted"
    );

    port.release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), shutdown.tasks.wait())
        .await
        .expect("the owner stops once its group ended");
    assert_eq!(
        port.calls.lock().unwrap().len(),
        started,
        "no group started after the cancel"
    );
}

#[tokio::test]
async fn core_log_level_reaches_only_the_streams_owner() {
    use nyanpasu_config::clash::config::overrides::LogLevel;
    let port = Arc::new(Port::default());
    let core = crate::control::CoreClient::spawn(endpoint("http://127.0.0.1:9".into()))
        .await
        .unwrap();
    let core_logs = core_logs_client().await;
    let streams = crate::clash::ws::StreamsClient::spawn(
        core,
        core_logs.clone(),
        LogLevel::Info,
        CancellationToken::new(),
        &TaskTracker::new(),
    )
    .await
    .unwrap();
    let effects = super::executor::CoreLogCaptureEffects::new(port.clone(), streams, core_logs);
    let locale = ApplicationEffect::Locale(I18nLanguage::Russian);

    let revision = EffectRevision::new(1);
    let statuses = effects
        .apply(
            revision,
            ApplicationEffectPlan::from_effects(vec![
                locale.clone(),
                ApplicationEffect::CoreLogLevel(LogLevel::Error),
            ]),
        )
        .await;
    assert_eq!(*port.calls.lock().unwrap(), [(revision, vec![locale])]);
    assert_eq!(
        statuses
            .iter()
            .map(|status| (status.kind, status.health.clone()))
            .collect::<Vec<_>>(),
        [
            (EffectKind::Locale, EffectHealth::Healthy),
            (EffectKind::CoreLogLevel, EffectHealth::Healthy)
        ]
    );

    let statuses = effects
        .apply(
            EffectRevision::new(2),
            ApplicationEffectPlan::from_effects(vec![ApplicationEffect::CoreLogLevel(
                LogLevel::Silent,
            )]),
        )
        .await;
    assert_eq!(statuses.len(), 1);
    assert_eq!(port.calls.lock().unwrap().len(), 1);

    let mut settings = nyanpasu_config::application::CoreLogSettings::default();
    settings.compression = nyanpasu_config::application::CoreLogCompression::Trained;
    let statuses = effects
        .apply(
            EffectRevision::new(3),
            ApplicationEffectPlan::from_effects(vec![ApplicationEffect::CoreLogStorage(settings)]),
        )
        .await;
    assert_eq!(statuses[0].kind, EffectKind::CoreLogStorage);
    assert_eq!(statuses[0].health, EffectHealth::Healthy);
    assert_eq!(port.calls.lock().unwrap().len(), 1);

    settings.shard_size_mib = 0;
    let statuses = effects
        .apply(
            EffectRevision::new(4),
            ApplicationEffectPlan::from_effects(vec![ApplicationEffect::CoreLogStorage(settings)]),
        )
        .await;
    assert!(matches!(
        statuses[0].health,
        EffectHealth::Degraded {
            code: EffectFailureCode::CoreLogStorageFailed,
            retryable: false,
            ..
        }
    ));
}

use crate::{
    control::endpoint::{
        ApiChanges, ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
    },
    logs::{
        CoreLogCursor, CoreLogPage, CoreLogQuery, CoreLogRecord, CoreLogResult, CoreLogStatus,
        CoreLogStore, CoreLogsClient, PreparedCoreLog, RedbCoreLogStore,
    },
};
use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
use nyanpasu_ipc::api::{
    core::v2::{CoreApiConnection, OperationInfo, OperationOutputInfo, OperationPhase},
    status::{CoreControllerInfo, CoreStateDetail},
};
use tempfile::TempDir;
use tokio::sync::watch;

struct Endpoint {
    host: ExecutionHost,
    binding: watch::Sender<Option<CoreApiConnection>>,
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

fn endpoint(url: String) -> Arc<Endpoint> {
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

struct TestStore {
    // Field drop order keeps the directory until the database has closed.
    inner: RedbCoreLogStore,
    _directory: TempDir,
}
impl CoreLogStore for TestStore {
    fn configure(
        &mut self,
        settings: nyanpasu_config::application::CoreLogSettings,
    ) -> CoreLogResult<()> {
        self.inner.configure(settings)
    }
    fn append(&mut self, records: &[PreparedCoreLog]) -> CoreLogResult<()> {
        self.inner.append(records)
    }
    fn query(&mut self, request: CoreLogQuery) -> CoreLogResult<CoreLogPage> {
        self.inner.query(request)
    }
    fn detail(&mut self, cursor: CoreLogCursor) -> CoreLogResult<CoreLogRecord> {
        self.inner.detail(cursor)
    }
    fn clear(&mut self) -> CoreLogResult<()> {
        self.inner.clear()
    }
    fn status(&mut self) -> CoreLogResult<CoreLogStatus> {
        self.inner.status()
    }
}

async fn core_logs_client() -> CoreLogsClient {
    let directory = TempDir::new().unwrap();
    let store = TestStore {
        inner: RedbCoreLogStore::open(directory.path().into()).unwrap(),
        _directory: directory,
    };
    let client = CoreLogsClient::spawn(
        Box::new(store),
        Default::default(),
        CancellationToken::new(),
        &TaskTracker::new(),
    )
    .await
    .unwrap();
    client.set_instance(Some("instance".into())).await.unwrap();
    client
}

#[tokio::test(start_paused = true)]
async fn missing_presentation_is_unsupported_without_retry_and_neutral_owners_still_apply() {
    use super::{
        convergence::ConvergenceHealth,
        executor::{ApplicationEffectExecutor, CoreLogCaptureEffects},
    };
    use crate::{
        logs::logging::MockLoggerRefresher,
        system_proxy::{
            SystemProxyArgs, SystemProxyClient,
            ports::{MockAutoLaunchPort, MockOsProxyPort, MockPacPort},
        },
    };
    use nyanpasu_config::clash::config::overrides::LogLevel;

    let shutdown = Shutdown::new();
    let mut auto_launch = MockAutoLaunchPort::new();
    auto_launch
        .expect_is_enabled()
        .times(1)
        .returning(|| Ok(false));
    let system_proxy = SystemProxyClient::spawn(
        SystemProxyArgs {
            os: Arc::new(MockOsProxyPort::new()),
            auto_launch: Arc::new(auto_launch),
            pac: Arc::new(MockPacPort::new()),
            schedule_guard_ticks: false,
            shutdown: shutdown.token.clone(),
        },
        &shutdown.tasks,
    )
    .await
    .unwrap();
    let mut logger = MockLoggerRefresher::new();
    logger.expect_refresh().times(1).returning(|_, _| Ok(()));
    let executor = ApplicationEffectExecutor::new(system_proxy, Arc::new(logger), None);
    let core = crate::control::CoreClient::spawn(endpoint("http://127.0.0.1:9".into()))
        .await
        .unwrap();
    let storage = core_logs_client().await;
    let streams = crate::clash::ws::StreamsClient::spawn(
        core,
        storage.clone(),
        LogLevel::Info,
        shutdown.token.clone(),
        &shutdown.tasks,
    )
    .await
    .unwrap();
    let client = EffectsClient::spawn(
        EffectsArgs {
            port: Arc::new(CoreLogCaptureEffects::new(
                Arc::new(executor),
                streams,
                storage,
            )),
            invalidation: None,
            initial: inputs(),
            shutdown: shutdown.token.clone(),
        },
        &shutdown.tasks,
    )
    .await
    .unwrap();
    client.publish_full(None);
    let snapshot = wait(&client, |s| {
        s.effects.len() == 10
            && s.effects
                .iter()
                .all(|e| e.health != ConvergenceHealth::Pending)
    })
    .await;
    for progress in &snapshot.effects {
        let status = &progress.status;
        if matches!(
            status.kind,
            EffectKind::Locale | EffectKind::Hotkeys | EffectKind::Widget | EffectKind::Tray
        ) {
            assert_eq!(
                status.health,
                EffectHealth::Unsupported {
                    code: EffectFailureCode::PresentationUnsupported
                }
            );
            assert_eq!(status.applied_revision, EffectRevision::default());
            assert_eq!(status.desired_revision, EffectRevision::new(1));
            assert_eq!(progress.health, ConvergenceHealth::Blocked);
            assert_eq!((progress.attempts, progress.automatic_remaining), (1, 3));
        } else {
            assert_eq!(status.health, EffectHealth::Healthy, "{:?}", status.kind);
            assert_eq!(status.applied_revision, status.desired_revision);
        }
    }
    assert!(client.retry_deadline().await.is_none());
    tokio::time::advance(Duration::from_secs(100)).await;
    client.barrier().await;
    assert_eq!(client.snapshot().event_seq, snapshot.event_seq);
    shutdown.run().await;
}

struct RecordingPresentation {
    calls: Arc<Mutex<Vec<EffectKind>>>,
    requests: Mutex<Vec<(EffectRevision, super::ports::PresentationRequest)>>,
}

#[async_trait::async_trait]
impl super::ports::PresentationEffectsPort for RecordingPresentation {
    async fn apply(
        &self,
        revision: EffectRevision,
        request: super::ports::PresentationRequest,
    ) -> EffectStatus {
        use super::ports::PresentationRequest;
        let kind = match &request {
            PresentationRequest::Locale(_) => EffectKind::Locale,
            PresentationRequest::Hotkeys(_) => EffectKind::Hotkeys,
            PresentationRequest::Widget(_) => EffectKind::Widget,
            PresentationRequest::Tray { .. } => EffectKind::Tray,
        };
        self.calls.lock().unwrap().push(kind);
        self.requests.lock().unwrap().push((revision, request));
        EffectStatus {
            kind,
            desired_revision: revision,
            applied_revision: revision,
            health: EffectHealth::Healthy,
        }
    }
}

#[tokio::test]
async fn presentation_requests_stay_in_plan_order_around_logger() {
    use super::{executor::ApplicationEffectExecutor, ports::PresentationRequest};
    use crate::{
        logs::logging::MockLoggerRefresher,
        system_proxy::{
            SystemProxyArgs, SystemProxyClient,
            ports::{MockAutoLaunchPort, MockOsProxyPort, MockPacPort},
        },
    };
    let shutdown = Shutdown::new();
    let system_proxy = SystemProxyClient::spawn(
        SystemProxyArgs {
            os: Arc::new(MockOsProxyPort::new()),
            auto_launch: Arc::new(MockAutoLaunchPort::new()),
            pac: Arc::new(MockPacPort::new()),
            schedule_guard_ticks: false,
            shutdown: shutdown.token.clone(),
        },
        &shutdown.tasks,
    )
    .await
    .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let presentation = Arc::new(RecordingPresentation {
        calls: calls.clone(),
        requests: Mutex::new(Vec::new()),
    });
    let mut logger = MockLoggerRefresher::new();
    let logger_calls = calls.clone();
    logger.expect_refresh().times(1).returning(move |_, _| {
        logger_calls.lock().unwrap().push(EffectKind::Logger);
        Ok(())
    });
    let executor =
        ApplicationEffectExecutor::new(system_proxy, Arc::new(logger), Some(presentation.clone()));
    let desired = inputs();
    let plan = ApplicationEffectPlan::from_effects(
        ApplicationEffectPlan::full(&desired)
            .effects()
            .iter()
            .filter(|e| {
                matches!(
                    e.kind(),
                    EffectKind::Locale
                        | EffectKind::Logger
                        | EffectKind::Hotkeys
                        | EffectKind::Widget
                        | EffectKind::Tray
                )
            })
            .cloned()
            .collect(),
    );
    let revision = EffectRevision::new(7);
    let statuses = executor.apply(revision, plan).await;
    assert_eq!(
        *calls.lock().unwrap(),
        [
            EffectKind::Locale,
            EffectKind::Logger,
            EffectKind::Hotkeys,
            EffectKind::Widget,
            EffectKind::Tray
        ]
    );
    assert_eq!(
        *presentation.requests.lock().unwrap(),
        [
            (revision, PresentationRequest::Locale(desired.app.language)),
            (
                revision,
                PresentationRequest::Hotkeys(desired.app.hotkeys.clone())
            ),
            (
                revision,
                PresentationRequest::Widget(desired.app.network_statistic_widget)
            ),
            (
                revision,
                PresentationRequest::Tray {
                    refresh: TrayRefresh::Full,
                    application: desired.app,
                    clash: desired.clash
                }
            ),
        ]
    );
    assert!(statuses.iter().all(|s| s.health == EffectHealth::Healthy
        && s.desired_revision == revision
        && s.applied_revision == revision));
    shutdown.run().await;
}

/// Parks a PAC apply until the test releases it, counting how often the actor
/// reached it.
#[derive(Default)]
struct HeldPac {
    applies: AtomicUsize,
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl crate::system_proxy::ports::PacPort for HeldPac {
    fn is_supported(&self) -> bool {
        true
    }

    async fn apply(
        &self,
        _url: &url::Url,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> Result<(), crate::system_proxy::ports::PacError> {
        self.applies.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        self.release.notified().await;
        Ok(())
    }

    fn disable(&self) -> Result<(), crate::system_proxy::ports::PacError> {
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn a_held_pac_keeps_its_group_until_the_owner_settles() {
    use super::{convergence::ConvergenceHealth, executor::ApplicationEffectExecutor};
    use crate::{
        logs::logging::MockLoggerRefresher,
        system_proxy::{
            SystemProxyArgs, SystemProxyClient,
            ports::{MockAutoLaunchPort, MockOsProxyPort, OsProxyConfig, OsProxyError},
        },
    };
    let pac = Arc::new(HeldPac::default());
    let shutdown = Shutdown::new();
    let mut os = MockOsProxyPort::new();
    os.expect_get().returning(|| {
        Err(OsProxyError::ReadOsProxy {
            source: "no system proxy is set".into(),
        })
    });
    os.expect_default_bypass().return_const("bypass");
    os.expect_set().returning(|_: &OsProxyConfig| Ok(()));
    let system_proxy = SystemProxyClient::spawn(
        SystemProxyArgs {
            os: Arc::new(os),
            auto_launch: Arc::new(MockAutoLaunchPort::new()),
            pac: pac.clone(),
            schedule_guard_ticks: false,
            shutdown: shutdown.token.child_token(),
        },
        &shutdown.tasks,
    )
    .await
    .expect("the system proxy actor should spawn");
    let presentation = Arc::new(RecordingPresentation {
        calls: Arc::new(Mutex::new(Vec::new())),
        requests: Mutex::new(Vec::new()),
    });
    let executor = ApplicationEffectExecutor::new(
        system_proxy,
        Arc::new(MockLoggerRefresher::new()),
        Some(presentation),
    );
    let mut initial = inputs();
    initial.ports = Some(ResolvedPortBindings {
        mixed_port: 7890,
        port: None,
        socks_port: None,
        external_controller: None,
    });
    let effects = EffectsClient::spawn(
        EffectsArgs {
            port: Arc::new(executor),
            invalidation: None,
            initial: initial,
            shutdown: shutdown.token.child_token(),
        },
        &shutdown.tasks,
    )
    .await
    .expect("the effects actor should spawn");

    effects.application_committed(
        (&NyanpasuAppConfig {
            enable_system_proxy: true,
            pac_url: Some(
                "http://example.test/proxy.pac"
                    .parse()
                    .expect("a valid url"),
            ),
            ..NyanpasuAppConfig::default()
        })
            .into(),
        Vec::new(),
    );
    pac.started.notified().await;

    // Well past any RPC bound and every automatic retry delay. A paused clock
    // runs each timer on the way, so any retry would have been submitted.
    tokio::time::sleep(std::time::Duration::from_secs(120)).await;
    effects.barrier().await;
    let proxy = |effects: &EffectsClient| {
        effects
            .snapshot()
            .effects
            .into_iter()
            .find(|progress| progress.status.kind == EffectKind::SystemProxy)
            .expect("the plan carries the system proxy")
    };
    let held = proxy(&effects);
    assert_eq!(
        (held.health, held.attempts),
        (ConvergenceHealth::Pending, 1),
        "the group must not be released or retried while the owner still runs"
    );

    pac.release.notify_one();
    let mut status = effects.subscribe();
    status
        .wait_for(|snapshot| {
            snapshot.effects.iter().any(|progress| {
                progress.status.kind == EffectKind::SystemProxy
                    && progress.health == ConvergenceHealth::Healthy
            })
        })
        .await
        .expect("the effects actor is alive");
    assert_eq!(proxy(&effects).attempts, 1);
    assert_eq!(pac.applies.load(Ordering::SeqCst), 1);
    shutdown.request();
    // No bound here: the paused clock would run one out while the restore
    // is on a blocking thread.
    shutdown.tasks.wait().await;
}
