//! Actor tests against fake ports: no OS call, no network, no sleep.
//!
//! The guard timer is not scheduled (`schedule_guard_ticks: false`); a test
//! delivers `GuardTick` itself, so every guard assertion is deterministic.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use super::{
    SystemProxyArgs, SystemProxyClient,
    ports::{
        AutoLaunchPort, MockAutoLaunchPort, MockOsProxyPort, MockPacPort, OsProxyConfig,
        OsProxyPort, PacPort,
    },
};
use crate::client::effects::{
    plan::{EffectKind, ProxyGuardDesired, SystemProxyDesired},
    status::{EffectHealth, EffectRevision, EffectStatus},
};

const BYPASS: &str = "localhost";
const DEFAULT_BYPASS: &str = "platform-default";

/// Records every write so a test can assert on the sequence instead of on a
/// single expectation, and reports whatever failure the test installed.
#[derive(Default)]
struct RecordingOsProxy {
    writes: Mutex<Vec<OsProxyConfig>>,
    original: Mutex<Option<OsProxyConfig>>,
    fail_set: Mutex<bool>,
}

impl RecordingOsProxy {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn with_original(original: OsProxyConfig) -> Arc<Self> {
        Arc::new(Self {
            original: Mutex::new(Some(original)),
            ..Self::default()
        })
    }

    fn writes(&self) -> Vec<OsProxyConfig> {
        self.writes.lock().expect("write log").clone()
    }

    fn last_write(&self) -> OsProxyConfig {
        self.writes().pop().expect("at least one os write")
    }

    fn fail_set(&self, fail: bool) {
        *self.fail_set.lock().expect("failure switch") = fail;
    }
}

impl OsProxyPort for RecordingOsProxy {
    fn get(&self) -> anyhow::Result<OsProxyConfig> {
        self.original
            .lock()
            .expect("original")
            .clone()
            .ok_or_else(|| anyhow::anyhow!("no system proxy is set"))
    }

    fn set(&self, config: &OsProxyConfig) -> anyhow::Result<()> {
        if *self.fail_set.lock().expect("failure switch") {
            anyhow::bail!("the os refused the proxy settings");
        }
        self.writes.lock().expect("write log").push(config.clone());
        Ok(())
    }

    fn default_bypass(&self) -> &'static str {
        DEFAULT_BYPASS
    }
}

fn silent_auto_launch() -> Arc<dyn AutoLaunchPort> {
    let mut port = MockAutoLaunchPort::new();
    port.expect_is_enabled().returning(|| Ok(false));
    port.expect_set_enabled().returning(|_| Ok(()));
    Arc::new(port)
}

fn unsupported_pac() -> Arc<dyn PacPort> {
    let mut pac = MockPacPort::new();
    pac.expect_is_supported().returning(|| false);
    Arc::new(pac)
}

async fn spawn(
    os: Arc<dyn OsProxyPort>,
    auto_launch: Arc<dyn AutoLaunchPort>,
    pac: Arc<dyn PacPort>,
) -> SystemProxyClient {
    SystemProxyClient::spawn(SystemProxyArgs {
        os,
        auto_launch,
        pac,
        schedule_guard_ticks: false,
    })
    .await
    .expect("the system proxy actor should spawn")
}

async fn spawn_with_os(os: Arc<dyn OsProxyPort>) -> SystemProxyClient {
    spawn(os, silent_auto_launch(), unsupported_pac()).await
}

fn proxy(enabled: bool, port: Option<u16>) -> SystemProxyDesired {
    SystemProxyDesired {
        enabled,
        bypass: BYPASS.to_owned(),
        port,
        pac_url: None,
    }
}

fn pac_proxy(url: &str) -> SystemProxyDesired {
    SystemProxyDesired {
        enabled: true,
        bypass: BYPASS.to_owned(),
        port: Some(7890),
        pac_url: Some(url.parse().expect("a valid pac url")),
    }
}

fn guard(enabled: bool, interval: Duration) -> ProxyGuardDesired {
    ProxyGuardDesired { enabled, interval }
}

fn rev(value: u64) -> EffectRevision {
    EffectRevision::new(value)
}

fn health_of(statuses: &[EffectStatus], kind: EffectKind) -> EffectHealth {
    statuses
        .iter()
        .find(|status| status.kind == kind)
        .unwrap_or_else(|| panic!("{kind:?} should be reported"))
        .health
        .clone()
}

fn code_of(statuses: &[EffectStatus], kind: EffectKind) -> String {
    match health_of(statuses, kind) {
        EffectHealth::Degraded { code, .. } => code.to_owned(),
        other => panic!("{kind:?} should be degraded, was {other:?}"),
    }
}

#[tokio::test]
async fn enabling_sets_os_proxy_with_resolved_port() {
    let os = RecordingOsProxy::new();
    let client = spawn_with_os(os.clone()).await;

    let statuses = client
        .reconcile(rev(1), Some(proxy(true, Some(7890))), None, None)
        .await;

    assert_eq!(
        health_of(&statuses, EffectKind::SystemProxy),
        EffectHealth::Healthy
    );
    assert_eq!(
        os.last_write(),
        OsProxyConfig {
            enable: true,
            host: "127.0.0.1".to_owned(),
            port: 7890,
            bypass: BYPASS.to_owned(),
        }
    );
}

#[tokio::test]
async fn empty_bypass_falls_back_to_the_platform_default() {
    let os = RecordingOsProxy::new();
    let client = spawn_with_os(os.clone()).await;

    let desired = SystemProxyDesired {
        bypass: String::new(),
        ..proxy(true, Some(7890))
    };
    client.reconcile(rev(1), Some(desired), None, None).await;

    assert_eq!(os.last_write().bypass, DEFAULT_BYPASS);
}

#[tokio::test]
async fn startup_with_proxy_disabled_leaves_os_untouched() {
    // A full plan is reconciled on every launch. Writing a disabled value here
    // would clear whatever proxy the user or another tool had configured.
    let mut os = MockOsProxyPort::new();
    os.expect_set().never();
    os.expect_get().never();
    os.expect_default_bypass().return_const(DEFAULT_BYPASS);
    let client = spawn_with_os(Arc::new(os)).await;

    let statuses = client
        .reconcile(
            rev(1),
            Some(proxy(false, Some(7890))),
            Some(guard(true, Duration::from_secs(10))),
            None,
        )
        .await;

    assert_eq!(
        health_of(&statuses, EffectKind::SystemProxy),
        EffectHealth::Healthy
    );
}

#[tokio::test]
async fn enable_without_resolved_port_degrades_without_os_write() {
    let os = RecordingOsProxy::new();
    let client = spawn_with_os(os.clone()).await;

    let statuses = client
        .reconcile(rev(1), Some(proxy(true, None)), None, None)
        .await;

    assert_eq!(
        code_of(&statuses, EffectKind::SystemProxy),
        "system_proxy_port_unresolved"
    );
    assert!(
        os.writes().is_empty(),
        "a proxy on port zero refuses every request"
    );
    assert_eq!(client.status().await.desired, Some(proxy(true, None)));

    // The desired state is kept, so the next reconcile that carries a port
    // installs it without the user touching the setting again.
    client
        .reconcile(rev(2), Some(proxy(true, Some(7890))), None, None)
        .await;
    assert_eq!(os.last_write().port, 7890);
}

#[tokio::test]
async fn disabling_restores_and_stops_guard() {
    let os = RecordingOsProxy::new();
    let client = spawn_with_os(os.clone()).await;
    client
        .reconcile(
            rev(1),
            Some(proxy(true, Some(7890))),
            Some(guard(true, Duration::from_secs(10))),
            None,
        )
        .await;
    assert!(client.status().await.guard_active);

    client
        .reconcile(rev(2), Some(proxy(false, Some(7890))), None, None)
        .await;

    assert!(!os.last_write().enable);
    let status = client.status().await;
    assert!(
        !status.guard_active,
        "a guard with no proxy to re-apply must not keep a timer"
    );
    assert_eq!(status.guard_interval, None);
}

#[tokio::test]
async fn port_change_reapplies_system_proxy() {
    let os = RecordingOsProxy::new();
    let client = spawn_with_os(os.clone()).await;

    client
        .reconcile(rev(1), Some(proxy(true, Some(7890))), None, None)
        .await;
    client
        .reconcile(rev(2), Some(proxy(true, Some(7891))), None, None)
        .await;

    let writes = os.writes();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[1].port, 7891);
}

#[tokio::test]
async fn pac_enabled_takes_over_and_skips_plain_proxy() {
    let os = RecordingOsProxy::new();
    let mut pac = MockPacPort::new();
    pac.expect_is_supported().returning(|| true);
    pac.expect_apply().times(1).returning(|_| Ok(()));
    let client = spawn(os.clone(), silent_auto_launch(), Arc::new(pac)).await;

    let statuses = client
        .reconcile(
            rev(1),
            Some(pac_proxy("http://example.test/proxy.pac")),
            None,
            None,
        )
        .await;

    assert_eq!(
        health_of(&statuses, EffectKind::SystemProxy),
        EffectHealth::Healthy
    );
    assert!(
        os.writes().is_empty(),
        "PAC and the plain proxy are the same setting; writing both makes them fight"
    );
    assert!(client.status().await.pac_active);
}

#[tokio::test]
async fn pac_failure_falls_back_to_direct_and_degrades() {
    let os = RecordingOsProxy::new();
    let mut pac = MockPacPort::new();
    pac.expect_is_supported().returning(|| true);
    pac.expect_apply()
        .returning(|_| Err(anyhow::anyhow!("the pac url is unreachable")));
    let client = spawn(os.clone(), silent_auto_launch(), Arc::new(pac)).await;

    let statuses = client
        .reconcile(
            rev(1),
            Some(pac_proxy("http://example.test/proxy.pac")),
            None,
            None,
        )
        .await;

    assert_eq!(
        code_of(&statuses, EffectKind::SystemProxy),
        "pac_apply_failed"
    );
    assert_eq!(os.last_write().port, 7890);
    assert!(!client.status().await.pac_active);
}

#[tokio::test]
async fn failed_pac_switch_disables_the_previous_pac_before_falling_back() {
    // The OS resolves the auto-config url ahead of any proxy endpoint, so the
    // fallback below is dead weight while the first url is still installed.
    let os = RecordingOsProxy::new();
    let mut pac = MockPacPort::new();
    pac.expect_is_supported().returning(|| true);
    pac.expect_apply().times(1).returning(|_| Ok(()));
    pac.expect_apply()
        .times(1)
        .returning(|_| Err(anyhow::anyhow!("the new pac url is unreachable")));
    pac.expect_disable().times(1).returning(|| Ok(()));
    let client = spawn(os.clone(), silent_auto_launch(), Arc::new(pac)).await;

    client
        .reconcile(
            rev(1),
            Some(pac_proxy("http://example.test/first.pac")),
            None,
            None,
        )
        .await;
    let statuses = client
        .reconcile(
            rev(2),
            Some(pac_proxy("http://example.test/second.pac")),
            None,
            None,
        )
        .await;

    assert_eq!(
        code_of(&statuses, EffectKind::SystemProxy),
        "pac_apply_failed"
    );
    assert!(
        !client.status().await.pac_active,
        "the OS confirmed the disable, so PAC really is off"
    );
    assert!(
        os.last_write().enable,
        "a failed PAC switch must still leave the user proxied"
    );
}

#[tokio::test]
async fn pac_disable_failure_keeps_pac_active_and_degrades() {
    let os = RecordingOsProxy::new();
    let mut pac = MockPacPort::new();
    pac.expect_is_supported().returning(|| true);
    pac.expect_apply().times(1).returning(|_| Ok(()));
    pac.expect_apply()
        .times(1)
        .returning(|_| Err(anyhow::anyhow!("the new pac url is unreachable")));
    pac.expect_disable()
        .times(1)
        .returning(|| Err(anyhow::anyhow!("the os kept the auto-config url")));
    pac.expect_disable().times(1).returning(|| Ok(()));
    let client = spawn(os.clone(), silent_auto_launch(), Arc::new(pac)).await;

    client
        .reconcile(
            rev(1),
            Some(pac_proxy("http://example.test/first.pac")),
            None,
            None,
        )
        .await;
    let statuses = client
        .reconcile(
            rev(2),
            Some(pac_proxy("http://example.test/second.pac")),
            None,
            None,
        )
        .await;

    assert_eq!(
        code_of(&statuses, EffectKind::SystemProxy),
        "pac_disable_failed"
    );
    assert!(
        client.status().await.pac_active,
        "an unconfirmed disable must not clear the flag; the url is still installed"
    );
    assert!(
        os.last_write().enable,
        "the fallback is still written so the user has a proxy"
    );

    // Still active means the exit path still has PAC to clean up, which is the
    // whole point of not clearing the flag.
    let status = client.restore().await;
    assert_eq!(status.health, EffectHealth::Healthy);
    assert!(!client.status().await.pac_active);
}

#[tokio::test]
async fn pac_unsupported_platform_reports_unsupported() {
    let os = RecordingOsProxy::new();
    let client = spawn_with_os(os.clone()).await;

    let statuses = client
        .reconcile(
            rev(1),
            Some(pac_proxy("http://example.test/proxy.pac")),
            None,
            None,
        )
        .await;

    assert_eq!(
        health_of(&statuses, EffectKind::SystemProxy),
        EffectHealth::Unsupported {
            code: "pac_unsupported"
        }
    );
    assert!(
        crate::client::effects::status::degradation_of(&statuses[0]).is_none(),
        "a platform fact is not a degradation"
    );
    // Still proxied: an unsupported PAC must not leave the user with nothing.
    assert!(os.last_write().enable);
}

#[tokio::test]
async fn guard_interval_change_rebuilds_timer() {
    let client = spawn_with_os(RecordingOsProxy::new()).await;

    client
        .reconcile(
            rev(1),
            Some(proxy(true, Some(7890))),
            Some(guard(true, Duration::from_secs(10))),
            None,
        )
        .await;
    let first = client.status().await;

    client
        .reconcile(
            rev(2),
            None,
            Some(guard(true, Duration::from_secs(30))),
            None,
        )
        .await;
    let second = client.status().await;

    assert_eq!(first.guard_interval, Some(Duration::from_secs(10)));
    assert_eq!(second.guard_interval, Some(Duration::from_secs(30)));
    assert!(first.guard_active && second.guard_active);
}

#[tokio::test]
async fn guard_tick_reapplies_last_desired() {
    let os = RecordingOsProxy::new();
    let client = spawn_with_os(os.clone()).await;
    client
        .reconcile(
            rev(1),
            Some(proxy(true, Some(7890))),
            Some(guard(true, Duration::from_secs(10))),
            None,
        )
        .await;

    client.tick_guard().await;

    let writes = os.writes();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0], writes[1]);
}

#[tokio::test]
async fn guard_tick_retries_a_failed_enable() {
    // The guard is the only thing that runs again on its own, so an enable the
    // OS refused has nowhere else to be retried.
    let os = RecordingOsProxy::new();
    os.fail_set(true);
    let client = spawn_with_os(os.clone()).await;
    client
        .reconcile(
            rev(1),
            Some(proxy(true, Some(7890))),
            Some(guard(true, Duration::from_secs(10))),
            None,
        )
        .await;
    assert!(os.writes().is_empty());
    assert_eq!(client.status().await.applied_os_proxy, None);

    os.fail_set(false);
    client.tick_guard().await;

    let expected = OsProxyConfig {
        enable: true,
        host: "127.0.0.1".to_owned(),
        port: 7890,
        bypass: BYPASS.to_owned(),
    };
    assert_eq!(os.last_write(), expected);
    assert_eq!(client.status().await.applied_os_proxy, Some(expected));
}

#[tokio::test]
async fn guard_tick_uses_the_newest_desired_port() {
    let os = RecordingOsProxy::new();
    let client = spawn_with_os(os.clone()).await;
    client
        .reconcile(
            rev(1),
            Some(proxy(true, Some(7890))),
            Some(guard(true, Duration::from_secs(10))),
            None,
        )
        .await;
    os.fail_set(true);
    client
        .reconcile(rev(2), Some(proxy(true, Some(7891))), None, None)
        .await;
    os.fail_set(false);

    client.tick_guard().await;

    assert_eq!(
        os.last_write().port,
        7891,
        "a refused port change must not leave the guard re-installing the old port"
    );
}

#[tokio::test]
async fn guard_tick_is_suppressed_while_pac_active() {
    let os = RecordingOsProxy::new();
    let mut pac = MockPacPort::new();
    pac.expect_is_supported().returning(|| true);
    pac.expect_apply().returning(|_| Ok(()));
    let client = spawn(os.clone(), silent_auto_launch(), Arc::new(pac)).await;
    client
        .reconcile(
            rev(1),
            Some(pac_proxy("http://example.test/proxy.pac")),
            Some(guard(true, Duration::from_secs(10))),
            None,
        )
        .await;

    client.tick_guard().await;

    assert!(os.writes().is_empty());
}

#[tokio::test]
async fn exit_restores_captured_original() {
    let original = OsProxyConfig {
        enable: true,
        host: "127.0.0.1".to_owned(),
        port: 1080,
        bypass: "corp".to_owned(),
    };
    let os = RecordingOsProxy::with_original(original.clone());
    let client = spawn_with_os(os.clone()).await;
    client
        .reconcile(rev(1), Some(proxy(true, Some(7890))), None, None)
        .await;

    let status = client.restore().await;

    assert_eq!(status.health, EffectHealth::Healthy);
    assert_eq!(os.last_write(), original);
}

#[tokio::test]
async fn exit_disables_when_nothing_was_captured() {
    let os = RecordingOsProxy::new();
    let client = spawn_with_os(os.clone()).await;
    client
        .reconcile(rev(1), Some(proxy(true, Some(7890))), None, None)
        .await;

    client.restore().await;

    let last = os.last_write();
    assert!(!last.enable);
    assert_eq!(last.port, 7890);
}

#[tokio::test]
async fn set_failure_degrades_and_keeps_desired() {
    let os = RecordingOsProxy::new();
    os.fail_set(true);
    let client = spawn_with_os(os.clone()).await;

    let statuses = client
        .reconcile(rev(1), Some(proxy(true, Some(7890))), None, None)
        .await;

    assert_eq!(
        code_of(&statuses, EffectKind::SystemProxy),
        "system_proxy_apply_failed"
    );
    let status = client.status().await;
    assert_eq!(status.desired, Some(proxy(true, Some(7890))));
    assert_eq!(
        status.health,
        EffectHealth::Degraded {
            code: "system_proxy_apply_failed",
            message: "the os refused the proxy settings".to_owned(),
            retryable: true,
        }
    );
}

#[tokio::test]
async fn stale_revision_is_superseded() {
    let os = RecordingOsProxy::new();
    let client = spawn_with_os(os.clone()).await;
    client
        .reconcile(rev(5), Some(proxy(true, Some(7890))), None, None)
        .await;

    let statuses = client
        .reconcile(rev(3), Some(proxy(false, Some(7890))), None, None)
        .await;

    assert_eq!(
        health_of(&statuses, EffectKind::SystemProxy),
        EffectHealth::Superseded
    );
    assert_eq!(
        os.writes().len(),
        1,
        "an older desired state must not be written back"
    );
    assert_eq!(client.status().await.applied_revision, rev(5));
}

#[tokio::test]
async fn stale_proxy_revision_is_still_applied_when_newer_revision_touched_only_auto_launch() {
    // A plan carries only what changed, and the facade releases its gate before
    // dispatch, so a later auto-launch-only reconcile can overtake an earlier
    // one that carries the proxy. One revision for the whole actor would drop
    // that proxy value for good.
    let os = RecordingOsProxy::new();
    let client = spawn_with_os(os.clone()).await;

    client.reconcile(rev(2), None, None, Some(true)).await;
    let statuses = client
        .reconcile(rev(1), Some(proxy(true, Some(7890))), None, None)
        .await;

    assert_eq!(
        health_of(&statuses, EffectKind::SystemProxy),
        EffectHealth::Healthy
    );
    assert_eq!(os.last_write().port, 7890);

    // The proxy capability now holds revision 1, so the same one again is the
    // stale reconcile it always was.
    let statuses = client
        .reconcile(rev(1), Some(proxy(false, Some(7890))), None, None)
        .await;

    assert_eq!(
        health_of(&statuses, EffectKind::SystemProxy),
        EffectHealth::Superseded
    );
    assert_eq!(os.writes().len(), 1);
}

#[tokio::test]
async fn auto_launch_failure_degrades_independently() {
    let os = RecordingOsProxy::new();
    let mut auto_launch = MockAutoLaunchPort::new();
    auto_launch.expect_is_enabled().returning(|| Ok(false));
    auto_launch
        .expect_set_enabled()
        .returning(|_| Err(anyhow::anyhow!("the login item is locked")));
    let client = spawn(os.clone(), Arc::new(auto_launch), unsupported_pac()).await;

    let statuses = client
        .reconcile(rev(1), Some(proxy(true, Some(7890))), None, Some(true))
        .await;

    assert_eq!(
        code_of(&statuses, EffectKind::AutoLaunch),
        "auto_launch_failed"
    );
    assert_eq!(
        health_of(&statuses, EffectKind::SystemProxy),
        EffectHealth::Healthy
    );
    assert!(os.last_write().enable);
}

#[tokio::test]
async fn auto_launch_already_in_the_desired_state_is_not_rewritten() {
    let mut auto_launch = MockAutoLaunchPort::new();
    auto_launch.expect_is_enabled().returning(|| Ok(true));
    auto_launch.expect_set_enabled().never();
    let client = spawn(
        RecordingOsProxy::new(),
        Arc::new(auto_launch),
        unsupported_pac(),
    )
    .await;

    let statuses = client.reconcile(rev(1), None, None, Some(true)).await;

    assert_eq!(
        health_of(&statuses, EffectKind::AutoLaunch),
        EffectHealth::Healthy
    );
}
