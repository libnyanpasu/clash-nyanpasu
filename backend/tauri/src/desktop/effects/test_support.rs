//! Shared host-owner fixtures for presentation and concrete adapter tests.

use std::sync::{Arc, Mutex};

use nyanpasu_config::{application::NyanpasuAppConfig, runtime::executor::ResolvedPortBindings};
use nyanpasu_contracts::system_proxy::{
    MockAutoLaunchPort, MockOsProxyPort, MockPacPort, OsProxyPort, PacPort,
};
use nyanpasu_core::client::{
    effects::{
        executor::ApplicationEffectExecutor, plan::ApplicationEffectInputs, ports::LoggerRefresher,
    },
    system_proxy::{SystemProxyArgs, SystemProxyClient},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    ports::{LocaleSink, TrayRefresher, WidgetController, WidgetError},
    presentation::PresentationEffectExecutor,
};
use crate::{
    adapters::hotkey::PlatformAcceleratorValidator,
    desktop::hotkey::{
        HotkeyArgs, HotkeyClient,
        ports::{MockHotkeyActionSink, MockShortcutRegistrar, ShortcutRegistrar},
    },
};

/// The root shutdown, shared by every owner a test builds.
pub(crate) struct Shutdown {
    pub(crate) token: CancellationToken,
    pub(crate) tasks: TaskTracker,
}

impl Shutdown {
    pub(crate) fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            tasks: TaskTracker::new(),
        }
    }

    pub(crate) fn request(&self) {
        self.token.cancel();
        self.tasks.close();
    }

    /// Whether every owner has finished within `wait`.
    pub(crate) async fn finished_within(&self, wait: std::time::Duration) -> bool {
        tokio::time::timeout(wait, self.tasks.wait()).await.is_ok()
    }
}

pub(crate) type CallLog = Arc<Mutex<Vec<&'static str>>>;

pub(crate) fn record(log: &CallLog, call: &'static str) {
    log.lock().expect("call log should not poison").push(call);
}

pub(crate) fn calls(log: &CallLog) -> Vec<&'static str> {
    log.lock().expect("call log should not poison").clone()
}

/// Two inputs that differ only in the fields a test changes, so the plan under
/// assertion carries exactly the effect that field implies.
pub(crate) fn inputs(app: NyanpasuAppConfig) -> ApplicationEffectInputs {
    ApplicationEffectInputs::project(
        &app,
        &nyanpasu_config::clash::config::ClashConfig::default(),
        None,
    )
}

/// The same projection with a port the session has already resolved, which is
/// what the system proxy actor needs before it writes anything to the OS.
pub(crate) fn proxied_inputs(app: NyanpasuAppConfig) -> ApplicationEffectInputs {
    ApplicationEffectInputs::project(
        &app,
        &nyanpasu_config::clash::config::ClashConfig::default(),
        Some(ResolvedPortBindings {
            mixed_port: 7890,
            port: None,
            socks_port: None,
            redir_port: None,
            tproxy_port: None,
            external_controller: None,
        }),
    )
}

/// The executor under its real dispatch logic, with only the UI capabilities
/// mocked. The system proxy and hotkey actors are spawned against fake ports
/// and never reached by a UI-only plan.
pub(crate) async fn executor(
    locale: Arc<dyn LocaleSink>,
    logger: Arc<dyn LoggerRefresher>,
    widget: Arc<dyn WidgetController>,
    tray: Arc<dyn TrayRefresher>,
) -> ApplicationEffectExecutor {
    executor_with_os(
        MockOsProxyPort::new(),
        locale,
        logger,
        widget,
        tray,
        &Shutdown::new(),
    )
    .await
}

/// The same executor with a system proxy that a test can make slow, so an
/// earlier plan can be observed waiting while a later one runs to completion.
pub(crate) async fn executor_with_os(
    os: MockOsProxyPort,
    locale: Arc<dyn LocaleSink>,
    logger: Arc<dyn LoggerRefresher>,
    widget: Arc<dyn WidgetController>,
    tray: Arc<dyn TrayRefresher>,
    shutdown: &Shutdown,
) -> ApplicationEffectExecutor {
    let mut pac = MockPacPort::new();
    pac.expect_is_supported().returning(|| false);
    executor_with_ports(os, Arc::new(pac), locale, logger, widget, tray, shutdown).await
}

/// The same executor with the PAC port injected too, so a test can park a plan
/// exactly where the shutdown token reaches it.
pub(crate) async fn executor_with_ports(
    os: MockOsProxyPort,
    pac: Arc<dyn PacPort>,
    locale: Arc<dyn LocaleSink>,
    logger: Arc<dyn LoggerRefresher>,
    widget: Arc<dyn WidgetController>,
    tray: Arc<dyn TrayRefresher>,
    shutdown: &Shutdown,
) -> ApplicationEffectExecutor {
    // The only registrar call a UI-shaped plan can reach is the shutdown's
    // release; a plan that registers a shortcut spawns its own registrar.
    let mut registrar = MockShortcutRegistrar::new();
    registrar.expect_unregister_all().returning(|| Ok(()));
    executor_with_owners(
        Arc::new(os),
        pac,
        Arc::new(registrar),
        locale,
        logger,
        widget,
        tray,
        shutdown,
    )
    .await
}

/// The same executor over the caller's own OS proxy and shortcut registrar,
/// for a test that holds one of their cleanups.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn executor_with_owners(
    os: Arc<dyn OsProxyPort>,
    pac: Arc<dyn PacPort>,
    registrar: Arc<dyn ShortcutRegistrar>,
    locale: Arc<dyn LocaleSink>,
    logger: Arc<dyn LoggerRefresher>,
    widget: Arc<dyn WidgetController>,
    tray: Arc<dyn TrayRefresher>,
    shutdown: &Shutdown,
) -> ApplicationEffectExecutor {
    let system_proxy = SystemProxyClient::spawn(
        SystemProxyArgs {
            os,
            auto_launch: Arc::new(MockAutoLaunchPort::new()),
            pac,
            schedule_guard_ticks: false,
            shutdown: shutdown.token.child_token(),
        },
        &shutdown.tasks,
    )
    .await
    .expect("the system proxy actor should spawn");
    let hotkeys = HotkeyClient::spawn(
        HotkeyArgs {
            registrar,
            sink: Arc::new(MockHotkeyActionSink::new()),
            shutdown: shutdown.token.child_token(),
        },
        &shutdown.tasks,
    )
    .await
    .expect("the hotkey actor should spawn");
    let presentation = Arc::new(PresentationEffectExecutor::new(
        hotkeys,
        Arc::new(PlatformAcceleratorValidator),
        locale,
        widget,
        tray,
    ));
    ApplicationEffectExecutor::new(system_proxy, logger, Some(presentation))
}

pub(crate) fn spawn_refused(reason: &str) -> WidgetError {
    WidgetError::SpawnWidget {
        variant: "small".into(),
        source: std::io::Error::other(reason.to_owned()),
    }
}
