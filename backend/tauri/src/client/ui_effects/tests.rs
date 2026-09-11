//! Adapter-level tests against mocked capabilities: no Tauri runtime, no tray,
//! no widget process, no sleep. Ordering is asserted from a shared call log
//! rather than from timing.

use std::sync::{Arc, Mutex};

use nyanpasu_config::application::{
    I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig, NyanpasuAppConfig,
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
        ports::{MockAutoLaunchPort, MockOsProxyPort, MockPacPort},
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

/// The executor under its real dispatch logic, with only the UI capabilities
/// mocked. The system proxy and hotkey actors are spawned against fake ports
/// and never reached by a UI-only plan.
async fn executor(
    locale: Arc<dyn LocaleSink>,
    logger: Arc<dyn LoggerRefresher>,
    widget: Arc<dyn WidgetController>,
    tray: Arc<dyn TrayRefresher>,
) -> ApplicationEffectExecutor {
    let system_proxy = SystemProxyClient::spawn(SystemProxyArgs {
        os: Arc::new(MockOsProxyPort::new()),
        auto_launch: Arc::new(MockAutoLaunchPort::new()),
        pac: Arc::new(MockPacPort::new()),
        schedule_guard_ticks: false,
    })
    .await
    .expect("the system proxy actor should spawn");
    let hotkeys = HotkeyClient::spawn(HotkeyArgs {
        registrar: Arc::new(MockShortcutRegistrar::new()),
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
