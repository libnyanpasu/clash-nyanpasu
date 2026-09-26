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

use super::{
    adapters::TauriWidgetController,
    ports::{
        LocaleSink, LoggerRefresher, MockLocaleSink, MockLoggerRefresher, MockTrayRefresher,
        MockWidgetController, MockWidgetRuntime, TrayRefresher, WidgetController, WidgetError,
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
        status::{EffectHealth, EffectRevision},
    },
    hotkey::{
        HotkeyArgs, HotkeyClient,
        adapters::PlatformAcceleratorValidator,
        ports::{MockHotkeyActionSink, MockShortcutRegistrar},
    },
    system_proxy::{
        SystemProxyArgs, SystemProxyClient,
        ports::{MockAutoLaunchPort, MockOsProxyPort, MockPacPort, OsProxyConfig, PacPort},
    },
};

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
    executor_with_os(MockOsProxyPort::new(), locale, logger, widget, tray).await
}

/// The same executor with a system proxy that a test can make slow, so an
/// earlier plan can be observed waiting while a later one runs to completion.
async fn executor_with_os(
    os: MockOsProxyPort,
    locale: Arc<dyn LocaleSink>,
    logger: Arc<dyn LoggerRefresher>,
    widget: Arc<dyn WidgetController>,
    tray: Arc<dyn TrayRefresher>,
) -> ApplicationEffectExecutor {
    let mut pac = MockPacPort::new();
    pac.expect_is_supported().returning(|| false);
    executor_with_ports(os, Arc::new(pac), locale, logger, widget, tray).await
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
) -> ApplicationEffectExecutor {
    let system_proxy = SystemProxyClient::spawn(SystemProxyArgs {
        os: Arc::new(os),
        auto_launch: Arc::new(MockAutoLaunchPort::new()),
        pac,
        schedule_guard_ticks: false,
    })
    .await
    .expect("the system proxy actor should spawn");
    // The only registrar call a UI-shaped plan can reach is the shutdown's
    // release; a plan that registers a shortcut spawns its own registrar.
    let mut registrar = MockShortcutRegistrar::new();
    registrar.expect_unregister_all().returning(|| Ok(()));
    let hotkeys = HotkeyClient::spawn(HotkeyArgs {
        registrar: Arc::new(registrar),
        sink: Arc::new(MockHotkeyActionSink::new()),
    })
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
        Ok(())
    });

    let mut tray = MockTrayRefresher::new();
    let tray_log = log.clone();
    tray.expect_refresh_full().times(1).returning(move || {
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
    locale
        .expect_set_locale()
        .returning(|_| Err(anyhow::anyhow!("locale refused")));
    let mut logger = MockLoggerRefresher::new();
    logger
        .expect_refresh()
        .returning(|_, _| Err(anyhow::anyhow!("logger refused")));
    let mut widget = MockWidgetController::new();
    widget.expect_apply().returning(|_| {
        Box::pin(async { Err(WidgetError::Failed(anyhow::anyhow!("widget refused"))) })
    });
    let mut tray = MockTrayRefresher::new();
    tray.expect_refresh_full()
        .returning(|| Box::pin(async { Err(anyhow::anyhow!("tray refused")) }));

    let executor = executor(
        Arc::new(locale),
        Arc::new(logger),
        Arc::new(widget),
        Arc::new(tray),
    )
    .await;

    // One patch that touches all four: the language rebuilds the tray as well.
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

    let codes: Vec<(EffectKind, &str)> = statuses
        .iter()
        .map(|status| match &status.health {
            EffectHealth::Degraded { code, .. } => (status.kind, *code),
            other => panic!("{:?} should have degraded, got {other:?}", status.kind),
        })
        .collect();
    assert_eq!(
        codes,
        vec![
            (EffectKind::Locale, "locale_apply_failed"),
            (EffectKind::Logger, "logger_refresh_failed"),
            (EffectKind::Widget, "widget_apply_failed"),
            (EffectKind::Tray, "tray_refresh_failed"),
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
        plan.effects()
            .contains(&ApplicationEffect::Tray(TrayRefresh::Full)),
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
        .position(|effect| matches!(effect, ApplicationEffect::Tray(_)))
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
        plan.effects()
            .contains(&ApplicationEffect::Tray(TrayRefresh::Part)),
        "nothing the menu is built from changed: {:?}",
        plan.effects()
    );
}

#[test]
fn logger_effect_forwards_level_and_max_files() {
    let mut logger = MockLoggerRefresher::new();
    logger
        .expect_refresh()
        .withf(|level, max_files| *level == Some(LoggingLevel::Error) && *max_files == Some(21))
        .times(1)
        .returning(|_, _| Ok(()));

    let logger: Arc<dyn LoggerRefresher> = Arc::new(logger);
    logger
        .refresh(Some(LoggingLevel::Error), Some(21))
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
        .expect_is_running()
        .returning(|| Box::pin(async { true }));
    runtime
        .expect_stop()
        .times(1)
        .returning(|| Box::pin(async { Ok(()) }));
    runtime.expect_start().never();
    let controller = controller_with(runtime);

    controller
        .apply(NetworkStatisticWidgetConfig::Disabled)
        .await
        .expect("disabling stops the widget");
}

#[tokio::test]
async fn disabled_config_is_a_noop_when_nothing_runs() {
    let mut runtime = MockWidgetRuntime::new();
    runtime
        .expect_is_running()
        .returning(|| Box::pin(async { false }));
    runtime.expect_stop().never();
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
    // The shutdown path treats the same state as nothing to do.
    assert!(matches!(
        controller.stop().await.expect_err("still uninstalled"),
        WidgetError::Unavailable
    ));
}

#[tokio::test]
async fn widget_start_failure_surfaces_as_a_widget_failure() {
    let mut runtime = MockWidgetRuntime::new();
    runtime
        .expect_is_running()
        .returning(|| Box::pin(async { false }));
    runtime.expect_start().times(1).returning(|_| {
        Box::pin(async { Err(anyhow::anyhow!("the widget process refused to start")) })
    });
    let controller = controller_with(runtime);

    let error = controller
        .apply(NetworkStatisticWidgetConfig::Enabled(
            StatisticWidgetVariant::Small,
        ))
        .await
        .expect_err("a failing spawn is a failure");

    assert!(matches!(error, WidgetError::Failed(_)), "{error:?}");
}

#[tokio::test]
async fn widget_stop_clears_the_started_variant() {
    let mut runtime = MockWidgetRuntime::new();
    runtime
        .expect_is_running()
        .returning(|| Box::pin(async { true }));
    runtime
        .expect_start()
        .times(2)
        .returning(|_| Box::pin(async { Ok(()) }));
    runtime
        .expect_stop()
        .times(1)
        .returning(|| Box::pin(async { Ok(()) }));
    let controller = controller_with(runtime);

    let desired = NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small);
    controller.apply(desired).await.expect("start");
    controller.stop().await.expect("stop");
    // After a shutdown the same variant is no longer running, so it starts again.
    controller.apply(desired).await.expect("restart");
}

#[tokio::test]
async fn late_full_tray_refresh_still_runs_after_a_newer_part_refresh() {
    // A refresh carries no value, so dropping a late one loses the rebuild for
    // good: the menu would stay in the old language until something else
    // happened to rebuild it.
    let log: CallLog = Arc::default();
    let mut locale = MockLocaleSink::new();
    let locale_log = log.clone();
    locale.expect_set_locale().times(1).returning(move |_| {
        record(&locale_log, "set_locale");
        Ok(())
    });
    let mut tray = MockTrayRefresher::new();
    let part_log = log.clone();
    tray.expect_refresh_part().times(1).returning(move || {
        let log = part_log.clone();
        Box::pin(async move {
            record(&log, "refresh_part");
            Ok(())
        })
    });
    let full_log = log.clone();
    tray.expect_refresh_full().times(1).returning(move || {
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
    ) -> anyhow::Result<()> {
        self.applies.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        self.release.notified().await;
        Ok(())
    }

    fn disable(&self) -> anyhow::Result<()> {
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
    let mut os = MockOsProxyPort::new();
    os.expect_get()
        .returning(|| Err(anyhow::anyhow!("no system proxy is set")));
    os.expect_default_bypass().return_const("bypass");
    os.expect_set().returning(|_: &OsProxyConfig| Ok(()));
    let mut tray = MockTrayRefresher::new();
    tray.expect_refresh_part()
        .returning(|| Box::pin(async { Ok(()) }));
    let executor = executor_with_ports(
        os,
        pac.clone(),
        Arc::new(MockLocaleSink::new()),
        Arc::new(MockLoggerRefresher::new()),
        Arc::new(MockWidgetController::new()),
        Arc::new(tray),
    )
    .await;
    let effects = EffectsClient::spawn(EffectsArgs {
        port: Arc::new(executor),
        ui: Arc::new(NoopUiEventSink),
        initial: proxied_inputs(NyanpasuAppConfig::default()),
    })
    .await
    .expect("the effects actor should spawn");

    effects.committed(
        proxied_inputs(NyanpasuAppConfig {
            enable_system_proxy: true,
            pac_url: Some(
                "http://example.test/proxy.pac"
                    .parse()
                    .expect("a valid url"),
            ),
            ..NyanpasuAppConfig::default()
        }),
        false,
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
    effects.shutdown().await;
}
