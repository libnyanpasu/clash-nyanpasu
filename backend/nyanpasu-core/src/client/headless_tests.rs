//! Missing shell capabilities through the real shared effect executor and facade.

use std::{error::Error as _, sync::Arc, time::Duration};

use nyanpasu_config::application::{I18nLanguage, NyanpasuAppConfigPatch};
use tempfile::TempDir;

use super::{
    ClientError, ClientSetupArgs, NyanpasuClient,
    convergence::ConvergenceHealth,
    debug_http::{DebugHttpControl, DebugHttpControlError, DebugHttpStatus},
    effects::{
        actor::EffectsSnapshot,
        executor::ApplicationEffectExecutor,
        plan::EffectKind,
        ports::{CommitNotifications as _, MockLoggerRefresher},
        status::{EffectFailureCode, EffectHealth, EffectRevision},
    },
    hotkey::ports::{HotkeyAction, WindowError},
    system_proxy::{SystemProxyArgs, SystemProxyClient},
    test_support,
    test_support::TestControlEndpoint,
};
use crate::state::config_error::ConfigError;
use nyanpasu_contracts::system_proxy::{MockAutoLaunchPort, MockOsProxyPort, MockPacPort};

const PRESENTATION_KINDS: [EffectKind; 5] = [
    EffectKind::Locale,
    EffectKind::Hotkeys,
    EffectKind::Widget,
    EffectKind::WidgetLogo,
    EffectKind::Tray,
];

async fn headless_args(dir: &TempDir) -> ClientSetupArgs {
    let mut args = test_support::test_client_args_with_endpoint(
        dir,
        TestControlEndpoint::succeeding(),
        test_support::profile_test_adapters(dir),
    )
    .await;
    let mut auto_launch = MockAutoLaunchPort::new();
    auto_launch.expect_is_enabled().returning(|| Ok(false));
    let system_proxy = SystemProxyClient::spawn(
        SystemProxyArgs {
            os: Arc::new(MockOsProxyPort::new()),
            auto_launch: Arc::new(auto_launch),
            pac: Arc::new(MockPacPort::new()),
            schedule_guard_ticks: false,
            shutdown: args.shutdown.child_token(),
        },
        &args.tasks,
    )
    .await
    .unwrap();
    let mut logger = MockLoggerRefresher::new();
    logger.expect_refresh().returning(|_, _| Ok(()));
    args.effects = Arc::new(ApplicationEffectExecutor::new(
        system_proxy,
        Arc::new(logger),
        None,
    ));
    args.window = None;
    args.accelerators = None;
    args.host_adapters.debug_http = None;
    args
}

async fn headless_client(dir: &TempDir) -> NyanpasuClient {
    NyanpasuClient::try_new(headless_args(dir).await)
        .await
        .unwrap()
}

async fn wait_for(
    client: &NyanpasuClient,
    predicate: impl Fn(&EffectsSnapshot) -> bool,
) -> EffectsSnapshot {
    let mut status = client.inner.effects.subscribe();
    tokio::time::timeout(Duration::from_secs(5), status.wait_for(predicate))
        .await
        .unwrap()
        .unwrap()
        .clone()
}

async fn reconcile(client: &NyanpasuClient) -> EffectsSnapshot {
    client.inner.effects.publish_full(None);
    wait_for(client, |snapshot| {
        // One entry per `EffectKind`.
        snapshot.effects.len() == 11
            && snapshot
                .effects
                .iter()
                .all(|effect| !matches!(effect.status.health, EffectHealth::Pending))
    })
    .await
}

async fn shutdown(client: &NyanpasuClient) {
    client.request_shutdown();
    tokio::time::timeout(Duration::from_secs(5), client.wait_shutdown())
        .await
        .unwrap();
}

#[tokio::test]
async fn headless_client_constructs_without_presentation_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    let args = headless_args(&dir).await;
    assert!(args.window.is_none());
    assert!(args.accelerators.is_none());
    assert!(args.host_adapters.debug_http.is_none());
    let client = NyanpasuClient::try_new(args).await.unwrap();
    assert!(client.get_app_config().await.is_ok());
    shutdown(&client).await;
}

#[tokio::test]
async fn missing_hotkey_capability_rejects_before_commit() {
    let dir = tempfile::tempdir().unwrap();
    let args = headless_args(&dir).await;
    let path = args.paths.application_config_path();
    let client = NyanpasuClient::try_new(args).await.unwrap();
    client
        .patch_app_config(NyanpasuAppConfigPatch {
            language: Some(I18nLanguage::Korean),
            ..Default::default()
        })
        .await
        .unwrap();
    let before = serde_json::to_value(client.get_app_config().await.unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let version = client.configuration_status().source_versions.application;
    for hotkeys in [vec!["toggle_tun_mode,Control+Q".to_owned()], vec![]] {
        let error = client
            .patch_app_config(NyanpasuAppConfigPatch {
                hotkeys: Some(hotkeys),
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "rejected edits must not persist"
        );
        assert_eq!(
            serde_json::to_value(client.get_app_config().await.unwrap()).unwrap(),
            before
        );
        assert_eq!(
            client.configuration_status().source_versions.application,
            version
        );
        assert!(matches!(
            error,
            ClientError::Config(ConfigError::ValidateHotkeyCapability)
        ));
    }
    let error = ConfigError::ValidateHotkeyCapability;
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        serde_json::json!({"kind": "validate_hotkey_capability"})
    );
    assert_eq!(error.to_string(), "editing hotkeys is not supported");
    assert!(error.source().is_none());
    shutdown(&client).await;
}

#[tokio::test]
async fn unrelated_config_patch_does_not_require_hotkeys() {
    let dir = tempfile::tempdir().unwrap();
    let client = headless_client(&dir).await;
    let before = client.configuration_status().source_versions.application;
    client
        .patch_app_config(NyanpasuAppConfigPatch {
            language: Some(I18nLanguage::Korean),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        client.get_app_config().await.unwrap().language,
        I18nLanguage::Korean
    );
    assert_eq!(
        client.configuration_status().source_versions.application,
        before + 1
    );
    shutdown(&client).await;
}

#[tokio::test]
async fn missing_window_returns_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let client = headless_client(&dir).await;
    let error = client
        .dispatch_hotkey_action(HotkeyAction::OpenOrCloseDashboard)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        ClientError::Window(WindowError::ToggleDashboardUnsupported)
    ));
    let error = WindowError::ToggleDashboardUnsupported;
    assert_eq!(
        error.to_string(),
        "toggling the dashboard window is not supported"
    );
    assert!(error.source().is_none());
    shutdown(&client).await;
}

struct FailedDebugHttp;

#[async_trait::async_trait]
impl DebugHttpControl for FailedDebugHttp {
    async fn status(&self) -> anyhow::Result<DebugHttpStatus> {
        Err(std::io::Error::other("status cause").into())
    }

    async fn set_enabled(&self, _: bool) -> anyhow::Result<DebugHttpStatus> {
        Err(std::io::Error::other("enabled cause").into())
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn missing_debug_http_returns_structured_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let client = headless_client(&dir).await;
    let read = client.debug_http_status().await.unwrap_err();
    let change = client.set_debug_http_enabled(true).await.unwrap_err();
    assert!(matches!(read, DebugHttpControlError::ReadStatusUnsupported));
    assert!(matches!(
        change,
        DebugHttpControlError::ChangeEnabledUnsupported
    ));
    assert!(matches!(
        client.set_debug_http_enabled(false).await.unwrap_err(),
        DebugHttpControlError::ChangeEnabledUnsupported
    ));
    for (error, kind, message) in [
        (
            read,
            "read_status_unsupported",
            "reading debug HTTP status is not supported",
        ),
        (
            change,
            "change_enabled_unsupported",
            "changing debug HTTP enablement is not supported",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({"kind": kind})
        );
        assert_eq!(error.to_string(), message);
        assert!(error.source().is_none());
    }
    shutdown(&client).await;

    let dir = tempfile::tempdir().unwrap();
    let mut args = headless_args(&dir).await;
    args.host_adapters.debug_http = Some(Arc::new(|| {
        Box::pin(async { Ok(Arc::new(FailedDebugHttp) as Arc<dyn DebugHttpControl>) })
    }));
    let client = NyanpasuClient::try_new(args).await.unwrap();
    let read = client.debug_http_status().await.unwrap_err();
    let change = client.set_debug_http_enabled(true).await.unwrap_err();
    assert!(matches!(read, DebugHttpControlError::ReadStatus { .. }));
    assert!(matches!(
        change,
        DebugHttpControlError::ChangeEnabled { .. }
    ));
    for (error, kind, message, cause) in [
        (
            read,
            "read_status",
            "could not read debug HTTP status",
            "status cause",
        ),
        (
            change,
            "change_enabled",
            "could not change debug HTTP enablement",
            "enabled cause",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({"kind": kind})
        );
        assert_eq!(error.to_string(), message);
        assert_eq!(error.source().unwrap().to_string(), cause);
        assert!(
            error
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .is_some()
        );
    }
    shutdown(&client).await;
}

#[tokio::test]
async fn unsupported_presentation_never_advances_applied_revision() {
    let dir = tempfile::tempdir().unwrap();
    let client = headless_client(&dir).await;
    let snapshot = reconcile(&client).await;
    for kind in PRESENTATION_KINDS {
        let progress = snapshot
            .effects
            .iter()
            .find(|effect| effect.status.kind == kind)
            .unwrap();
        assert_eq!(
            progress.status.applied_revision,
            EffectRevision::default(),
            "{kind:?} must not be reported applied"
        );
        assert!(progress.status.desired_revision > EffectRevision::default());
        assert_eq!(
            progress.status.health,
            EffectHealth::Unsupported {
                code: EffectFailureCode::PresentationUnsupported
            }
        );
    }
    assert_eq!(
        serde_json::to_value(EffectFailureCode::PresentationUnsupported).unwrap(),
        "presentation_unsupported"
    );
    client.retry_effect_now(EffectKind::Hotkeys).unwrap();
    client.request_tray_refresh().unwrap();
    client.request_widget_logo_refresh().unwrap();
    let retried = wait_for(&client, |state| {
        [
            EffectKind::Hotkeys,
            EffectKind::Tray,
            EffectKind::WidgetLogo,
        ]
        .iter()
        .all(|kind| {
            state.effects.iter().any(|effect| {
                effect.status.kind == *kind
                    && effect.status.desired_revision
                        > snapshot
                            .effects
                            .iter()
                            .find(|previous| previous.status.kind == *kind)
                            .unwrap()
                            .status
                            .desired_revision
                    && effect.health == ConvergenceHealth::Blocked
            })
        })
    })
    .await;
    for effect in retried
        .effects
        .iter()
        .filter(|effect| PRESENTATION_KINDS.contains(&effect.status.kind))
    {
        assert_eq!(effect.status.applied_revision, EffectRevision::default());
        assert_eq!(
            effect.status.health,
            EffectHealth::Unsupported {
                code: EffectFailureCode::PresentationUnsupported
            }
        );
    }
    shutdown(&client).await;
}

#[tokio::test]
async fn unsupported_presentation_does_not_retry() {
    let dir = tempfile::tempdir().unwrap();
    let client = headless_client(&dir).await;
    let snapshot = reconcile(&client).await;
    // No timer is armed, so nothing will ever retry the blocked effects.
    assert_eq!(client.inner.effects.retry_deadline().await, None);
    for kind in PRESENTATION_KINDS {
        let effect = snapshot
            .effects
            .iter()
            .find(|effect| effect.status.kind == kind)
            .unwrap();
        assert_eq!(effect.health, ConvergenceHealth::Blocked);
        assert_eq!(effect.attempts, 1);
    }
    shutdown(&client).await;
}

#[tokio::test]
async fn headless_shared_effects_still_converge() {
    let dir = tempfile::tempdir().unwrap();
    let client = headless_client(&dir).await;
    let snapshot = reconcile(&client).await;
    for kind in [
        EffectKind::Logger,
        EffectKind::SystemProxy,
        EffectKind::ProxyGuard,
        EffectKind::AutoLaunch,
        EffectKind::CoreLogLevel,
        EffectKind::CoreLogStorage,
    ] {
        let effect = snapshot
            .effects
            .iter()
            .find(|effect| effect.status.kind == kind)
            .unwrap();
        assert_eq!(effect.status.health, EffectHealth::Healthy, "{kind:?}");
        assert!(effect.status.applied_revision > EffectRevision::default());
        assert_eq!(
            effect.status.applied_revision,
            effect.status.desired_revision
        );
    }
    shutdown(&client).await;
}

#[tokio::test]
async fn headless_shutdown_without_debug_owner_completes() {
    let dir = tempfile::tempdir().unwrap();
    let client = headless_client(&dir).await;
    client.shutdown_debug_http().await.unwrap();
    shutdown(&client).await;
    client.shutdown_debug_http().await.unwrap();
}
