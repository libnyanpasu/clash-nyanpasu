//! Adapter-level tests against mocked capabilities: no Tauri runtime, no tray,
//! no widget process, no sleep. Ordering is asserted from a shared call log
//! rather than from timing.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use nyanpasu_config::{
    application::{I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig, NyanpasuAppConfig},
    runtime::executor::ResolvedPortBindings,
};
use nyanpasu_egui::widget::StatisticWidgetVariant;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    adapters::TauriWidgetController,
    ports::{
        LocaleSink, LogRotation, LoggerError, LoggerRefresher, MockLocaleSink, MockLoggerRefresher,
        MockTrayRefresher, MockWidgetController, MockWidgetRuntime, TrayError, TrayRefresher,
        WIDGET_STOP_BOUND, WidgetController, WidgetError,
    },
};
use crate::client::{
    effects::{
        executor::ApplicationEffectExecutor,
        plan::{
            ApplicationEffect, ApplicationEffectInputs, ApplicationEffectPlan, EffectKind,
            TrayRefresh,
        },
        ports::ApplicationEffectsPort,
        status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus},
    },
    hotkey::{
        HotkeyArgs, HotkeyClient,
        adapters::PlatformAcceleratorValidator,
        ports::{
            HotkeyAction, HotkeyActionSink, HotkeyParseError, MockHotkeyActionSink,
            MockShortcutRegistrar, ShortcutError, ShortcutRegistrar,
        },
    },
    system_proxy::{
        SystemProxyArgs, SystemProxyClient,
        ports::{
            MockAutoLaunchPort, MockOsProxyPort, MockPacPort, OsProxyConfig, OsProxyError,
            OsProxyPort, PacError, PacPort,
        },
    },
};

/// The root shutdown, shared by every owner a test builds.
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

    /// Whether every owner has finished within `wait`.
    async fn finished_within(&self, wait: std::time::Duration) -> bool {
        tokio::time::timeout(wait, self.tasks.wait()).await.is_ok()
    }
}

type CallLog = Arc<Mutex<Vec<&'static str>>>;

fn record(log: &CallLog, call: &'static str) {
    log.lock().expect("call log should not poison").push(call);
}

fn calls(log: &CallLog) -> Vec<&'static str> {
    log.lock().expect("call log should not poison").clone()
}

/// Two inputs that differ only in the fields a test changes, so the plan under
/// assertion carries exactly the effect that field implies.
fn inputs(app: NyanpasuAppConfig) -> ApplicationEffectInputs {
    ApplicationEffectInputs::project(
        &app,
        &nyanpasu_config::clash::config::ClashConfig::default(),
        None,
    )
}

/// The same projection with a port the session has already resolved, which is
/// what the system proxy actor needs before it writes anything to the OS.
fn proxied_inputs(app: NyanpasuAppConfig) -> ApplicationEffectInputs {
    ApplicationEffectInputs::project(
        &app,
        &nyanpasu_config::clash::config::ClashConfig::default(),
        Some(ResolvedPortBindings {
            mixed_port: 7890,
            port: None,
            socks_port: None,
            external_controller: None,
        }),
    )
}

/// The executor under its real dispatch logic, with only the UI capabilities
/// mocked. The system proxy and hotkey actors are spawned against fake ports
/// and never reached by a UI-only plan.
async fn executor(
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
async fn executor_with_os(
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
async fn executor_with_ports(
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
async fn executor_with_owners(
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
    ApplicationEffectExecutor::new(
        system_proxy,
        hotkeys,
        Arc::new(PlatformAcceleratorValidator),
        locale,
        logger,
        widget,
        tray,
    )
}

fn controller_with(runtime: MockWidgetRuntime) -> TauriWidgetController {
    let controller = TauriWidgetController::default();
    controller
        .install(Arc::new(runtime))
        .expect("the runtime installs once");
    controller
}

#[tokio::test]
async fn locale_is_applied_before_tray_refresh() {
    let log: CallLog = Arc::default();

    let mut locale = MockLocaleSink::new();
    let locale_log = log.clone();
    locale.expect_set_locale().times(1).returning(move |_| {
        record(&locale_log, "set_locale");
    });

    let mut tray = MockTrayRefresher::new();
    let tray_log = log.clone();
    tray.expect_refresh_full().times(1).returning(move |_| {
        let log = tray_log.clone();
        Box::pin(async move {
            record(&log, "refresh_full");
            Ok(())
        })
    });

    let executor = executor(
        Arc::new(locale),
        Arc::new(MockLoggerRefresher::new()),
        Arc::new(MockWidgetController::new()),
        Arc::new(tray),
    )
    .await;

    let app = NyanpasuAppConfig {
        language: I18nLanguage::Russian,
        ..NyanpasuAppConfig::default()
    };
    let plan = ApplicationEffectPlan::diff(&inputs(NyanpasuAppConfig::default()), &inputs(app));
    let statuses = executor.apply(EffectRevision::new(1), plan).await;

    assert_eq!(
        calls(&log),
        vec!["set_locale", "refresh_full"],
        "the tray menu is rendered from the locale, so the locale goes first"
    );
    assert!(
        statuses
            .iter()
            .all(|status| status.health == EffectHealth::Healthy),
        "{statuses:?}"
    );
}

#[tokio::test]
async fn each_ui_failure_gets_its_own_code() {
    let mut locale = MockLocaleSink::new();
    locale.expect_set_locale().returning(|_| ());
    let mut logger = MockLoggerRefresher::new();
    logger
        .expect_refresh()
        .returning(|_, _| Err(LoggerError::ReloadThreadStopped));
    let mut widget = MockWidgetController::new();
    widget
        .expect_apply()
        .returning(|_| Box::pin(async { Err(spawn_refused("widget refused")) }));
    let mut tray = MockTrayRefresher::new();
    tray.expect_refresh_full().returning(|_| {
        Box::pin(async {
            Err(TrayError::ScheduleTrayWork {
                source: "tray refused".into(),
            })
        })
    });

    let executor = executor(
        Arc::new(locale),
        Arc::new(logger),
        Arc::new(widget),
        Arc::new(tray),
    )
    .await;

    // One patch that touches all four: the language rebuilds the tray as well.
    // Setting the locale cannot fail, so the other three degrade beside it.
    let app = NyanpasuAppConfig {
        language: I18nLanguage::Russian,
        app_log_level: LoggingLevel::Error,
        network_statistic_widget: NetworkStatisticWidgetConfig::Enabled(
            StatisticWidgetVariant::Small,
        ),
        ..NyanpasuAppConfig::default()
    };
    let plan = ApplicationEffectPlan::diff(&inputs(NyanpasuAppConfig::default()), &inputs(app));
    assert_eq!(plan.effects().len(), 4, "{:?}", plan.effects());
    let statuses = executor.apply(EffectRevision::new(4), plan).await;

    let codes: Vec<(EffectKind, EffectFailureCode)> = statuses
        .iter()
        .filter(|status| status.kind != EffectKind::Locale)
        .map(|status| match &status.health {
            EffectHealth::Degraded { code, .. } => (status.kind, *code),
            other => panic!("{:?} should have degraded, got {other:?}", status.kind),
        })
        .collect();
    assert_eq!(
        codes,
        vec![
            (EffectKind::Logger, EffectFailureCode::LoggerRefreshFailed),
            (EffectKind::Widget, EffectFailureCode::WidgetApplyFailed),
            (EffectKind::Tray, EffectFailureCode::TrayRefreshFailed),
        ],
        "one failure must not mask another"
    );
}

#[test]
fn language_change_requests_full_refresh() {
    let before = inputs(NyanpasuAppConfig::default());
    let after = inputs(NyanpasuAppConfig {
        language: I18nLanguage::Russian,
        ..NyanpasuAppConfig::default()
    });

    let plan = ApplicationEffectPlan::diff(&before, &after);

    assert!(
        plan.effects().contains(&ApplicationEffect::Tray(
            TrayRefresh::Full,
            after.tray_view()
        )),
        "a language change rebuilds the menu: {:?}",
        plan.effects()
    );
    let locale = plan
        .effects()
        .iter()
        .position(|effect| matches!(effect, ApplicationEffect::Locale(_)))
        .expect("a language change carries the locale");
    let tray = plan
        .effects()
        .iter()
        .position(|effect| matches!(effect, ApplicationEffect::Tray(..)))
        .expect("a language change refreshes the tray");
    assert!(locale < tray, "the plan orders the locale before the tray");
}

#[test]
fn system_proxy_change_only_requests_part_refresh() {
    let before = inputs(NyanpasuAppConfig::default());
    let after = inputs(NyanpasuAppConfig {
        enable_system_proxy: !NyanpasuAppConfig::default().enable_system_proxy,
        ..NyanpasuAppConfig::default()
    });

    let plan = ApplicationEffectPlan::diff(&before, &after);

    assert!(
        plan.effects().contains(&ApplicationEffect::Tray(
            TrayRefresh::Part,
            after.tray_view()
        )),
        "nothing the menu is built from changed: {:?}",
        plan.effects()
    );
}

#[test]
fn logger_effect_forwards_level_and_rotation() {
    let rotation = LogRotation {
        max_files: 21,
        max_file_size: 5,
    };
    let mut logger = MockLoggerRefresher::new();
    logger
        .expect_refresh()
        .withf(move |level, forwarded| {
            *level == Some(LoggingLevel::Error) && *forwarded == Some(rotation)
        })
        .times(1)
        .returning(|_, _| Ok(()));

    let logger: Arc<dyn LoggerRefresher> = Arc::new(logger);
    logger
        .refresh(Some(LoggingLevel::Error), Some(rotation))
        .expect("the logger accepts the reload signal");
}

#[tokio::test]
async fn widget_same_variant_is_a_noop() {
    let mut runtime = MockWidgetRuntime::new();
    runtime
        .expect_start()
        .times(1)
        .returning(|_| Box::pin(async { Ok(()) }));
    runtime
        .expect_is_running()
        .returning(|| Box::pin(async { true }));
    runtime.expect_stop().never();
    let controller = controller_with(runtime);

    let desired = NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small);
    controller.apply(desired).await.expect("first apply starts");
    controller
        .apply(desired)
        .await
        .expect("a repeated apply is accepted");
}

#[tokio::test]
async fn widget_variant_change_restarts_the_widget() {
    let mut runtime = MockWidgetRuntime::new();
    runtime
        .expect_start()
        .times(2)
        .returning(|_| Box::pin(async { Ok(()) }));
    runtime
        .expect_is_running()
        .returning(|| Box::pin(async { true }));
    let controller = controller_with(runtime);

    controller
        .apply(NetworkStatisticWidgetConfig::Enabled(
            StatisticWidgetVariant::Small,
        ))
        .await
        .expect("first apply starts");
    controller
        .apply(NetworkStatisticWidgetConfig::Enabled(
            StatisticWidgetVariant::Large,
        ))
        .await
        .expect("a different variant restarts");
}

#[tokio::test]
async fn disabled_config_stops_a_running_widget() {
    let mut runtime = MockWidgetRuntime::new();
    runtime
        .expect_stop()
        .times(1)
        .returning(|_| Box::pin(async { Ok(()) }));
    runtime.expect_start().never();
    let controller = controller_with(runtime);

    controller
        .apply(NetworkStatisticWidgetConfig::Disabled)
        .await
        .expect("disabling stops the widget");
}

/// Whether the widget runs is not asked: one that is owned without running
/// has to be stopped too, and a stop with nothing owned is a no-op.
#[tokio::test]
async fn disabled_config_stops_without_asking_whether_the_widget_runs() {
    let mut runtime = MockWidgetRuntime::new();
    runtime.expect_is_running().never();
    runtime
        .expect_stop()
        .times(1)
        .returning(|_| Box::pin(async { Ok(()) }));
    runtime.expect_start().never();
    let controller = controller_with(runtime);

    controller
        .apply(NetworkStatisticWidgetConfig::Disabled)
        .await
        .expect("there is nothing to stop");
}

#[tokio::test]
async fn widget_before_install_degrades() {
    let controller = TauriWidgetController::default();

    let error = controller
        .apply(NetworkStatisticWidgetConfig::Enabled(
            StatisticWidgetVariant::Small,
        ))
        .await
        .expect_err("no runtime is installed yet");

    assert!(
        matches!(error, WidgetError::Unavailable),
        "an uninstalled controller is unavailable, not a widget failure: {error:?}"
    );
}

#[tokio::test]
async fn widget_start_failure_surfaces_as_a_widget_failure() {
    let mut runtime = MockWidgetRuntime::new();
    runtime
        .expect_is_running()
        .returning(|| Box::pin(async { false }));
    runtime.expect_start().times(1).returning(|_| {
        Box::pin(async { Err(spawn_refused("the widget process refused to start")) })
    });
    let controller = controller_with(runtime);

    let error = controller
        .apply(NetworkStatisticWidgetConfig::Enabled(
            StatisticWidgetVariant::Small,
        ))
        .await
        .expect_err("a failing spawn is a failure");

    assert!(
        matches!(error, WidgetError::SpawnWidget { .. }),
        "{error:?}"
    );
}

fn spawn_refused(reason: &str) -> WidgetError {
    WidgetError::SpawnWidget {
        variant: "small".into(),
        source: std::io::Error::other(reason.to_owned()),
    }
}

fn widget_health(statuses: Vec<EffectStatus>) -> EffectHealth {
    let [status] = <[EffectStatus; 1]>::try_from(statuses).expect("a widget-only plan");
    assert_eq!(status.kind, EffectKind::Widget);
    status.health
}

/// A start whose cleanup could not release the handshake leaves the widget
/// owned in `Starting`, which is not running. Every disable retries that
/// cleanup and stays degraded until the worker is seen to end; once nothing
/// is owned, a disable is a clean no-op.
#[tokio::test(start_paused = true)]
async fn disabling_retries_the_cleanup_of_a_widget_that_never_started() {
    let host = crate::widget::tests::FakeWidgetHost::blocking();
    host.refuse_release();
    let manager = crate::widget::WidgetManager::new(
        host.clone(),
        CancellationToken::new(),
        &TaskTracker::new(),
    );
    let controller = Arc::new(TauriWidgetController::default());
    controller
        .install(Arc::new(manager.clone()))
        .expect("the runtime installs once");
    let executor = executor(
        Arc::new(MockLocaleSink::new()),
        Arc::new(MockLoggerRefresher::new()),
        controller,
        Arc::new(MockTrayRefresher::new()),
    )
    .await;
    let disabled = inputs(NyanpasuAppConfig::default());
    let enabled = inputs(NyanpasuAppConfig {
        network_statistic_widget: NetworkStatisticWidgetConfig::Enabled(
            StatisticWidgetVariant::Small,
        ),
        ..NyanpasuAppConfig::default()
    });
    let disable = || ApplicationEffectPlan::diff(&enabled, &disabled);

    // The child dies before it connects and the release reaches nothing, so
    // the start fails and its cleanup runs out of time on the blocked worker.
    // That worker also keeps the paused clock from moving on its own.
    let mut starting = std::pin::pin!(executor.apply(
        EffectRevision::new(1),
        ApplicationEffectPlan::diff(&disabled, &enabled)
    ));
    tokio::select! {
        statuses = &mut starting => panic!("the start ended before its spawn: {statuses:?}"),
        () = host.spawned.notified() => {}
    }
    host.exit();
    tokio::select! {
        statuses = &mut starting => panic!("the start ended before its cleanup: {statuses:?}"),
        () = host.released.notified() => {}
    }
    tokio::time::advance(WIDGET_STOP_BOUND).await;
    assert!(
        matches!(
            widget_health(starting.await),
            EffectHealth::Degraded { code: EffectFailureCode::WidgetApplyFailed, ref message, .. }
                if message.contains("exited before it connected")
        ),
        "the start failed"
    );
    assert_eq!(manager.owned().await, Some("starting"));

    let mut disabling = std::pin::pin!(executor.apply(EffectRevision::new(2), disable()));
    tokio::select! {
        statuses = &mut disabling => panic!("disabled without a cleanup: {statuses:?}"),
        () = host.released.notified() => {}
    }
    tokio::time::advance(WIDGET_STOP_BOUND).await;
    assert_eq!(
        widget_health(disabling.await),
        EffectHealth::Degraded {
            code: EffectFailureCode::WidgetApplyFailed,
            message: "widget handshake worker still blocked".into(),
            retryable: true,
        }
    );
    assert_eq!(manager.owned().await, Some("starting"));

    // Once the handshake ends, the next disable sees the worker gone.
    host.unblock();
    assert_eq!(
        widget_health(executor.apply(EffectRevision::new(3), disable()).await),
        EffectHealth::Healthy
    );
    assert_eq!(manager.owned().await, None);

    let before = host.events();
    assert_eq!(
        widget_health(executor.apply(EffectRevision::new(4), disable()).await),
        EffectHealth::Healthy
    );
    assert_eq!(host.events(), before, "nothing was left to stop");
}

#[tokio::test]
async fn late_full_tray_refresh_still_runs_after_a_newer_part_refresh() {
    // A part refresh repaints a menu that is already built, so dropping a late
    // rebuild loses it for good: the menu would stay in the old language until
    // something else happened to rebuild it.
    let log: CallLog = Arc::default();
    let mut locale = MockLocaleSink::new();
    let locale_log = log.clone();
    locale.expect_set_locale().times(1).returning(move |_| {
        record(&locale_log, "set_locale");
    });
    let mut tray = MockTrayRefresher::new();
    let part_log = log.clone();
    tray.expect_refresh_part().times(1).returning(move |_| {
        let log = part_log.clone();
        Box::pin(async move {
            record(&log, "refresh_part");
            Ok(())
        })
    });
    let full_log = log.clone();
    tray.expect_refresh_full().times(1).returning(move |_| {
        let log = full_log.clone();
        Box::pin(async move {
            record(&log, "refresh_full");
            Ok(())
        })
    });

    let executor = executor(
        Arc::new(locale),
        Arc::new(MockLoggerRefresher::new()),
        Arc::new(MockWidgetController::new()),
        Arc::new(tray),
    )
    .await;

    // Revision 2 only repaints the tray items and finishes first.
    let default = NyanpasuAppConfig::default();
    let part = ApplicationEffectPlan::diff(
        &inputs(default.clone()),
        &inputs(NyanpasuAppConfig {
            enable_tray_text: !default.enable_tray_text,
            ..default.clone()
        }),
    );
    let newer = executor.apply(EffectRevision::new(2), part).await;
    assert!(
        newer
            .iter()
            .all(|status| status.health == EffectHealth::Healthy),
        "{newer:?}"
    );

    // Revision 1 is the language change that was delayed behind it.
    let full = ApplicationEffectPlan::diff(
        &inputs(default.clone()),
        &inputs(NyanpasuAppConfig {
            language: I18nLanguage::Russian,
            ..default
        }),
    );
    let older = executor.apply(EffectRevision::new(1), full).await;

    assert!(
        older
            .iter()
            .all(|status| status.health == EffectHealth::Healthy),
        "the rebuild is not superseded, because nothing newer performed one: {older:?}"
    );
    assert_eq!(
        calls(&log),
        vec!["refresh_part", "set_locale", "refresh_full"],
        "the delayed rebuild still runs, and still runs after its own locale"
    );
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
impl PacPort for HeldPac {
    fn is_supported(&self) -> bool {
        true
    }

    async fn apply(
        &self,
        _url: &url::Url,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> Result<(), PacError> {
        self.applies.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        self.release.notified().await;
        Ok(())
    }

    fn disable(&self) -> Result<(), PacError> {
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn a_held_pac_keeps_its_group_until_the_owner_settles() {
    use crate::client::{
        NoopUiEventSink,
        convergence::ConvergenceHealth,
        effects::{
            actor::{EffectsArgs, EffectsClient},
            ports::CommitNotifications,
        },
    };

    let pac = Arc::new(HeldPac::default());
    let shutdown = Shutdown::new();
    let mut os = MockOsProxyPort::new();
    os.expect_get()
        .returning(|| Err(OsProxyError::unreadable("no system proxy is set")));
    os.expect_default_bypass().return_const("bypass");
    os.expect_set().returning(|_: &OsProxyConfig| Ok(()));
    let mut tray = MockTrayRefresher::new();
    tray.expect_refresh_part()
        .returning(|_| Box::pin(async { Ok(()) }));
    let executor = executor_with_ports(
        os,
        pac.clone(),
        Arc::new(MockLocaleSink::new()),
        Arc::new(MockLoggerRefresher::new()),
        Arc::new(MockWidgetController::new()),
        Arc::new(tray),
        &shutdown,
    )
    .await;
    let effects = EffectsClient::spawn(
        EffectsArgs {
            port: Arc::new(executor),
            ui: Arc::new(NoopUiEventSink),
            initial: proxied_inputs(NyanpasuAppConfig::default()),
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

// -- the owners' own cleanups on the shutdown token -------------------------

fn quiet_tray() -> MockTrayRefresher {
    let mut tray = MockTrayRefresher::new();
    tray.expect_refresh_part()
        .returning(|_| Box::pin(async { Ok(()) }));
    tray.expect_refresh_full()
        .returning(|_| Box::pin(async { Ok(()) }));
    tray
}

/// Holds a PAC download until the shutdown token fires, logging both ends.
struct CancelledPac {
    log: CallLog,
    started: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl PacPort for CancelledPac {
    fn is_supported(&self) -> bool {
        true
    }

    async fn apply(
        &self,
        _url: &url::Url,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<(), PacError> {
        record(&self.log, "pac download started");
        self.started.notify_one();
        cancel.cancelled().await;
        record(&self.log, "pac download cancelled");
        Err(PacError::DownloadCancelled)
    }

    fn disable(&self) -> Result<(), PacError> {
        record(&self.log, "pac disabled");
        Ok(())
    }
}

/// X5 (V16): the token reaches the PAC download in flight, the restore runs
/// after it on the owner's own exit path, and nothing installs a proxy again.
#[tokio::test]
async fn the_shutdown_ends_a_pac_download_before_the_restore_runs() {
    use crate::client::{
        NoopUiEventSink,
        effects::{
            actor::{EffectsArgs, EffectsClient},
            ports::CommitNotifications,
        },
    };

    let shutdown = Shutdown::new();
    let log = CallLog::default();
    let pac = Arc::new(CancelledPac {
        log: log.clone(),
        started: tokio::sync::Notify::new(),
    });
    let mut os = MockOsProxyPort::new();
    os.expect_get()
        .returning(|| Err(OsProxyError::unreadable("no system proxy is set")));
    os.expect_default_bypass().return_const("bypass");
    let written = log.clone();
    os.expect_set().returning(move |config: &OsProxyConfig| {
        record(
            &written,
            match config.enable {
                true => "os proxy enabled",
                false => "os proxy disabled",
            },
        );
        Ok(())
    });
    let executor = executor_with_ports(
        os,
        pac.clone(),
        Arc::new(MockLocaleSink::new()),
        Arc::new(MockLoggerRefresher::new()),
        Arc::new(MockWidgetController::new()),
        Arc::new(quiet_tray()),
        &shutdown,
    )
    .await;
    let effects = EffectsClient::spawn(
        EffectsArgs {
            port: Arc::new(executor),
            ui: Arc::new(NoopUiEventSink),
            initial: proxied_inputs(NyanpasuAppConfig::default()),
            shutdown: shutdown.token.child_token(),
        },
        &shutdown.tasks,
    )
    .await
    .expect("the effects actor should spawn");
    effects.application_committed(
        (&NyanpasuAppConfig {
            enable_system_proxy: true,
            ..NyanpasuAppConfig::default()
        })
            .into(),
        Vec::new(),
    );
    effects
        .subscribe()
        .wait_for(|snapshot| {
            snapshot.effects.iter().any(|progress| {
                progress.status.kind == EffectKind::SystemProxy
                    && progress.status.health == EffectHealth::Healthy
            })
        })
        .await
        .expect("the effects actor is alive");
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

    shutdown.request();

    assert!(
        shutdown
            .finished_within(std::time::Duration::from_secs(12))
            .await
    );
    assert_eq!(
        calls(&log),
        vec![
            "os proxy enabled",
            "pac download started",
            "pac download cancelled",
            "os proxy disabled",
        ],
        "the restore is the only write once the shutdown began"
    );
}

/// X5 (V19): an OS call the token cannot interrupt keeps the restore behind
/// it for as long as it takes; nothing gives up on it, and the write the
/// stuck call resumes into never happens.
#[tokio::test]
async fn the_restore_waits_for_an_os_call_the_token_cannot_interrupt() {
    let shutdown = Shutdown::new();
    let reading = Arc::new(tokio::sync::Notify::new());
    let (release, released) = std::sync::mpsc::channel::<()>();
    let released = Mutex::new(released);
    let mut os = MockOsProxyPort::new();
    let entered = reading.clone();
    os.expect_get().returning(move || {
        entered.notify_one();
        let _ = released.lock().expect("gate").recv();
        Err(OsProxyError::unreadable("no system proxy is set"))
    });
    os.expect_default_bypass().return_const("bypass");
    os.expect_set().never();
    let executor = Arc::new(
        executor_with_os(
            os,
            Arc::new(MockLocaleSink::new()),
            Arc::new(MockLoggerRefresher::new()),
            Arc::new(MockWidgetController::new()),
            Arc::new(quiet_tray()),
            &shutdown,
        )
        .await,
    );
    let enabling = ApplicationEffectPlan::from_effects(
        ApplicationEffectPlan::full(&proxied_inputs(NyanpasuAppConfig {
            enable_system_proxy: true,
            ..NyanpasuAppConfig::default()
        }))
        .effects()
        .iter()
        .filter(|effect| effect.kind() == EffectKind::SystemProxy)
        .cloned()
        .collect(),
    );
    let applying = tokio::spawn({
        let executor = executor.clone();
        async move { executor.apply(EffectRevision::new(1), enabling).await }
    });
    reading.notified().await;

    shutdown.request();
    // Far past the five seconds the restore used to be given.
    assert!(
        !shutdown
            .finished_within(std::time::Duration::from_millis(200))
            .await,
        "the restore waits for the capture in flight"
    );

    release.send(()).expect("the capture is still parked");
    let statuses = applying.await.expect("the apply task should finish");
    assert!(
        statuses.iter().all(|status| matches!(
            status.health,
            EffectHealth::Degraded {
                code: EffectFailureCode::SystemProxyShutDown,
                ..
            }
        )),
        "{statuses:?}"
    );
    assert!(
        shutdown
            .finished_within(std::time::Duration::from_secs(5))
            .await
    );
}

/// A registrar whose release blocks until the test lets it go.
#[derive(Default)]
struct HeldRegistrar {
    releasing: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl ShortcutRegistrar for HeldRegistrar {
    fn validate(&self, _: &str) -> Result<(), HotkeyParseError> {
        Ok(())
    }

    async fn register(
        &self,
        _: &str,
        _: HotkeyAction,
        _: Arc<dyn HotkeyActionSink>,
    ) -> Result<(), ShortcutError> {
        Ok(())
    }

    async fn unregister(&self, _: &str) -> Result<(), ShortcutError> {
        Ok(())
    }

    async fn unregister_all(&self) -> Result<(), ShortcutError> {
        self.releasing.notify_one();
        self.release.notified().await;
        Ok(())
    }
}

/// An OS proxy whose writes block once the first one went through, so the
/// restore can be held after the proxy was installed.
struct HeldOsProxy {
    writes: std::sync::atomic::AtomicUsize,
    writing: tokio::sync::Notify,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl OsProxyPort for HeldOsProxy {
    fn get(&self) -> Result<OsProxyConfig, OsProxyError> {
        Err(OsProxyError::unreadable("no system proxy is set"))
    }

    fn set(&self, _: &OsProxyConfig) -> Result<(), OsProxyError> {
        if self.writes.fetch_add(1, Ordering::SeqCst) > 0 {
            self.writing.notify_one();
            let _ = self.release.lock().expect("gate").recv();
        }
        Ok(())
    }

    fn default_bypass(&self) -> &'static str {
        "bypass"
    }
}

/// A plan that blocks inside the effects owner until the test lets it go.
#[derive(Default)]
struct HeldEffects {
    applying: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

#[async_trait::async_trait]
impl ApplicationEffectsPort for HeldEffects {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        self.applying.notify_one();
        self.release.notified().await;
        plan.effects()
            .iter()
            .map(|effect| EffectStatus {
                kind: effect.kind(),
                desired_revision: revision,
                applied_revision: revision,
                health: EffectHealth::Healthy,
            })
            .collect()
    }
}

/// V18: three owners whose cleanups all block start them at once, and the
/// shutdown waits until every one of them has really finished.
#[tokio::test]
async fn the_owners_clean_up_independently_and_the_shutdown_waits_for_all() {
    use crate::client::{
        NoopUiEventSink,
        effects::{
            actor::{EffectsArgs, EffectsClient},
            ports::CommitNotifications,
        },
    };

    let shutdown = Shutdown::new();
    let (release_restore, restore_gate) = std::sync::mpsc::channel::<()>();
    let os = Arc::new(HeldOsProxy {
        writes: std::sync::atomic::AtomicUsize::new(0),
        writing: tokio::sync::Notify::new(),
        release: Mutex::new(restore_gate),
    });
    let registrar = Arc::new(HeldRegistrar::default());
    let mut pac = MockPacPort::new();
    pac.expect_is_supported().returning(|| false);
    let executor = executor_with_owners(
        os.clone(),
        Arc::new(pac),
        registrar.clone(),
        Arc::new(MockLocaleSink::new()),
        Arc::new(MockLoggerRefresher::new()),
        Arc::new(MockWidgetController::new()),
        Arc::new(quiet_tray()),
        &shutdown,
    )
    .await;
    // The proxy is installed, so the restore has something to put back.
    let installed = executor
        .apply(
            EffectRevision::new(1),
            ApplicationEffectPlan::from_effects(
                ApplicationEffectPlan::full(&proxied_inputs(NyanpasuAppConfig {
                    enable_system_proxy: true,
                    ..NyanpasuAppConfig::default()
                }))
                .effects()
                .iter()
                .filter(|effect| effect.kind() == EffectKind::SystemProxy)
                .cloned()
                .collect(),
            ),
        )
        .await;
    assert!(
        installed
            .iter()
            .all(|status| status.health == EffectHealth::Healthy),
        "{installed:?}"
    );
    let held = Arc::new(HeldEffects::default());
    let effects = EffectsClient::spawn(
        EffectsArgs {
            port: held.clone(),
            ui: Arc::new(NoopUiEventSink),
            initial: inputs(NyanpasuAppConfig::default()),
            shutdown: shutdown.token.child_token(),
        },
        &shutdown.tasks,
    )
    .await
    .expect("the effects actor should spawn");
    effects.publish_full(None);
    held.applying.notified().await;

    shutdown.request();

    // All three cleanups are under way at once.
    os.writing.notified().await;
    registrar.releasing.notified().await;
    let pending = std::time::Duration::from_millis(50);
    assert!(!shutdown.finished_within(pending).await);
    release_restore
        .send(())
        .expect("the restore is still parked");
    assert!(!shutdown.finished_within(pending).await);
    registrar.release.notify_one();
    assert!(!shutdown.finished_within(pending).await);
    held.release.notify_waiters();
    assert!(
        shutdown
            .finished_within(std::time::Duration::from_secs(5))
            .await
    );
}

/// The effects actor over a real widget controller and manager whose host
/// is `host`, with the widget's start already parked in its handshake.
async fn effects_with_a_widget_starting(
    host: &Arc<crate::widget::tests::FakeWidgetHost>,
    shutdown: &Shutdown,
) -> (
    crate::client::effects::actor::EffectsClient,
    crate::widget::WidgetManager,
) {
    use crate::client::{
        NoopUiEventSink,
        effects::{
            actor::{EffectsArgs, EffectsClient},
            ports::CommitNotifications,
        },
    };

    let manager = crate::widget::WidgetManager::new(
        host.clone(),
        shutdown.token.child_token(),
        &shutdown.tasks,
    );
    let controller = Arc::new(TauriWidgetController::default());
    controller
        .install(Arc::new(manager.clone()))
        .expect("the runtime installs once");
    let executor = executor_with_os(
        MockOsProxyPort::new(),
        Arc::new(MockLocaleSink::new()),
        Arc::new(MockLoggerRefresher::new()),
        controller,
        Arc::new(quiet_tray()),
        shutdown,
    )
    .await;
    let effects = EffectsClient::spawn(
        EffectsArgs {
            port: Arc::new(executor),
            ui: Arc::new(NoopUiEventSink),
            initial: inputs(NyanpasuAppConfig::default()),
            shutdown: shutdown.token.child_token(),
        },
        &shutdown.tasks,
    )
    .await
    .expect("the effects actor should spawn");
    effects.application_committed(
        (&NyanpasuAppConfig {
            network_statistic_widget: NetworkStatisticWidgetConfig::Enabled(
                StatisticWidgetVariant::Small,
            ),
            ..NyanpasuAppConfig::default()
        })
            .into(),
        Vec::new(),
    );
    host.spawned.notified().await;
    (effects, manager)
}

/// X5c (V25): the token ends a handshake the widget never completes. The
/// child is killed, the handshake released and the worker seen to end, and
/// the widget's process-boundary bounds still apply.
#[tokio::test]
async fn the_shutdown_ends_a_widget_handshake_and_reaps_the_child() {
    let shutdown = Shutdown::new();
    let host = crate::widget::tests::FakeWidgetHost::blocking();
    let (_effects, manager) = effects_with_a_widget_starting(&host, &shutdown).await;

    shutdown.request();

    assert!(
        shutdown
            .finished_within(std::time::Duration::from_secs(12))
            .await,
        "the effects group ends with the handshake"
    );
    assert_eq!(manager.owned().await, None);
    assert_eq!(
        host.events(),
        vec!["spawn", "kill", "release", "handshake released"]
    );
}

/// The refresher owns no logger of its own: it forwards to the reload channel
/// it was handed, and says so once nothing is left to receive.
#[test]
fn the_logger_refresher_forwards_to_the_reload_channel_it_was_given() {
    let (reload, signals) = std::sync::mpsc::channel();
    let refresher = super::adapters::TracingLoggerRefresher::new(reload);
    let rotation = LogRotation {
        max_files: 3,
        max_file_size: 10,
    };

    refresher
        .refresh(Some(LoggingLevel::Info), Some(rotation))
        .unwrap();
    assert_eq!(
        signals.try_recv().unwrap(),
        (Some(LoggingLevel::Info), Some(rotation))
    );

    drop(signals);
    assert!(refresher.refresh(None, Some(rotation)).is_err());
}
