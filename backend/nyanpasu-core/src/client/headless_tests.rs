//! Missing shell capabilities through the real shared effect executor and facade.

use std::{error::Error as _, sync::Arc, time::Duration};

use nyanpasu_config::application::{I18nLanguage, NyanpasuAppConfigPatch, RemoteAccessConfig};
use nyanpasu_runtime_values::{
    configuration::{ConvergenceHealth, EffectFailureCode, EffectKind},
    hotkey::HotkeyAction,
};
use tempfile::TempDir;

use super::{
    ClientError, ClientSetupArgs, NyanpasuClient, SetRemoteAccessError,
    effects::{
        actor::EffectsSnapshot,
        executor::ApplicationEffectExecutor,
        ports::{CommitNotifications as _, MockLoggerRefresher},
        status::{EffectHealth, EffectRevision},
    },
    hotkey::ports::WindowError,
    http_server::{HttpServerControl, HttpServerControlError, HttpServerStatus},
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
    args.host_adapters.http_server = None;
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
        snapshot.effects.len() == 12
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
    assert!(args.host_adapters.http_server.is_none());
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

struct FailedHttpServer;

#[async_trait::async_trait]
impl HttpServerControl for FailedHttpServer {
    async fn status(&self) -> anyhow::Result<HttpServerStatus> {
        Err(std::io::Error::other("status cause").into())
    }

    async fn configure(
        &self,
        _: &nyanpasu_config::application::HttpAccessConfig,
    ) -> anyhow::Result<HttpServerStatus> {
        Err(std::io::Error::other("configure cause").into())
    }

    async fn launch_url(&self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn missing_http_server_returns_structured_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let client = headless_client(&dir).await;
    let read = client.http_server_status().await.unwrap_err();
    let SetRemoteAccessError::Server { source: change } = client
        .set_remote_access(RemoteAccessConfig::default())
        .await
        .unwrap_err()
    else {
        panic!("a host without an HTTP server rejects the change");
    };
    assert!(matches!(
        read,
        HttpServerControlError::ReadStatusUnsupported
    ));
    assert!(matches!(change, HttpServerControlError::ChangeUnsupported));
    assert_eq!(client.remote_access_config(), RemoteAccessConfig::default());
    for (error, kind, message) in [
        (
            read,
            "read_status_unsupported",
            "reading the HTTP server status is not supported",
        ),
        (
            change,
            "change_unsupported",
            "changing the HTTP server settings is not supported",
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
    args.host_adapters.http_server = Some(Arc::new(|| {
        Box::pin(async { Ok(Arc::new(FailedHttpServer) as Arc<dyn HttpServerControl>) })
    }));
    let client = NyanpasuClient::try_new(args).await.unwrap();
    let read = client.http_server_status().await.unwrap_err();
    assert!(matches!(read, HttpServerControlError::ReadStatus { .. }));
    assert_eq!(
        serde_json::to_value(&read).unwrap(),
        serde_json::json!({"kind": "read_status"})
    );
    assert_eq!(read.to_string(), "could not read the HTTP server status");
    assert_eq!(read.source().unwrap().to_string(), "status cause");
    assert!(
        read.source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .is_some()
    );
    // The settings commit first, so a listener failure afterwards is a degraded outcome
    // and not an error that suggests nothing was committed.
    let outcome = client
        .set_remote_access(RemoteAccessConfig::default())
        .await
        .unwrap();
    assert!(matches!(
        outcome.degradations(),
        [degradation] if matches!(
            degradation.reason,
            nyanpasu_runtime_values::runtime::DegradationReason::HttpServerUnavailable
        )
    ));
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
async fn headless_shutdown_without_http_owner_completes() {
    let dir = tempfile::tempdir().unwrap();
    let client = headless_client(&dir).await;
    shutdown(&client).await;
}

#[tokio::test]
async fn the_general_patch_cannot_write_remote_access() {
    let dir = tempfile::tempdir().unwrap();
    let client = headless_client(&dir).await;
    let mut remote_access = RemoteAccessConfig::default();
    remote_access.http.enabled = true;

    let error = client
        .patch_app_config(NyanpasuAppConfigPatch {
            remote_access: Some(remote_access),
            proxy_layout_column: Some(3),
            ..Default::default()
        })
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        ClientError::Config(ConfigError::InvalidRemoteAccess {
            reason: nyanpasu_config::application::RemoteAccessError::UseDedicatedCommand
        })
    ));
    assert_eq!(client.remote_access_config(), RemoteAccessConfig::default());
    shutdown(&client).await;
}
