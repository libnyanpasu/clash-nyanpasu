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
        ports::{MockAutoLaunchPort, MockOsProxyPort, MockPacPort, OsProxyConfig},
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
    let system_proxy = SystemProxyClient::spawn(SystemProxyArgs {
        os: Arc::new(os),
        auto_launch: Arc::new(MockAutoLaunchPort::new()),
        pac: Arc::new(pac),
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

/// A plan whose only effect is the widget, so a revision assertion is about the
/// widget alone.
fn widget_plan(
    before: NetworkStatisticWidgetConfig,
    after: NetworkStatisticWidgetConfig,
) -> ApplicationEffectPlan {
    let plan = ApplicationEffectPlan::diff(
        &inputs(NyanpasuAppConfig {
            network_statistic_widget: before,
            ..NyanpasuAppConfig::default()
        }),
        &inputs(NyanpasuAppConfig {
            network_statistic_widget: after,
            ..NyanpasuAppConfig::default()
        }),
    );
    assert_eq!(
        plan.effects().len(),
        1,
        "only the widget should differ: {:?}",
        plan.effects()
    );
    plan
}

#[tokio::test]
async fn stale_ui_revision_is_superseded_without_touching_adapters() {
    let mut widget = MockWidgetController::new();
    widget
        .expect_apply()
        .times(1)
        .returning(|_| Box::pin(async { Ok(()) }));
    let executor = executor(
        Arc::new(MockLocaleSink::new()),
        Arc::new(MockLoggerRefresher::new()),
        Arc::new(widget),
        Arc::new(MockTrayRefresher::new()),
    )
    .await;
    let plan = widget_plan(
        NetworkStatisticWidgetConfig::Disabled,
        NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small),
    );

    let newer = executor.apply(EffectRevision::new(5), plan.clone()).await;
    let older = executor.apply(EffectRevision::new(3), plan).await;

    assert_eq!(newer[0].health, EffectHealth::Healthy, "{newer:?}");
    assert_eq!(
        older[0].health,
        EffectHealth::Superseded,
        "an older plan must not reach the adapter at all: {older:?}"
    );
    assert_eq!(
        older[0].applied_revision,
        EffectRevision::new(5),
        "the reported applied revision is the one that is actually installed"
    );
}

#[tokio::test]
async fn newer_ui_revision_still_applies_after_a_failure() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut widget = MockWidgetController::new();
    let counter = attempts.clone();
    widget.expect_apply().times(2).returning(move |_| {
        let first = counter.fetch_add(1, Ordering::SeqCst) == 0;
        Box::pin(async move {
            if first {
                Err(WidgetError::Failed(anyhow::anyhow!("widget refused")))
            } else {
                Ok(())
            }
        })
    });
    let executor = executor(
        Arc::new(MockLocaleSink::new()),
        Arc::new(MockLoggerRefresher::new()),
        Arc::new(widget),
        Arc::new(MockTrayRefresher::new()),
    )
    .await;
    let plan = widget_plan(
        NetworkStatisticWidgetConfig::Disabled,
        NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small),
    );

    let failed = executor.apply(EffectRevision::new(1), plan.clone()).await;
    assert!(
        matches!(
            failed[0].health,
            EffectHealth::Degraded {
                code: "widget_apply_failed",
                ..
            }
        ),
        "{failed:?}"
    );

    // A failure consumes the revision, so the same one again is stale: the
    // facade re-dispatches a retryable failure with a fresh, higher revision.
    let replayed = executor.apply(EffectRevision::new(1), plan.clone()).await;
    assert_eq!(replayed[0].health, EffectHealth::Superseded, "{replayed:?}");

    let retried = executor.apply(EffectRevision::new(2), plan).await;
    assert_eq!(retried[0].health, EffectHealth::Healthy, "{retried:?}");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2,
        "the adapter is reached once per revision that is not stale"
    );
}

#[tokio::test]
async fn a_plan_overtaken_mid_flight_does_not_re_apply_its_widget() {
    // The system proxy is dispatched before the widget, so an OS call that
    // never returns holds the earlier plan exactly where the facade's released
    // gate lets a later one pass it.
    let (entered_tx, mut entered_rx) = tokio::sync::mpsc::unbounded_channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let release_rx = Arc::new(Mutex::new(Some(release_rx)));
    let mut os = MockOsProxyPort::new();
    os.expect_get()
        .returning(|| Err(anyhow::anyhow!("no system proxy is set")));
    os.expect_default_bypass().return_const("bypass");
    os.expect_set().returning(move |_: &OsProxyConfig| {
        entered_tx.send(()).expect("the test is still waiting");
        let receiver = release_rx
            .lock()
            .expect("gate should not poison")
            .take()
            .expect("only the first plan is gated");
        receiver
            .blocking_recv()
            .expect("the test releases the gate");
        Ok(())
    });

    let applied: Arc<Mutex<Vec<NetworkStatisticWidgetConfig>>> = Arc::default();
    let mut widget = MockWidgetController::new();
    let widget_log = applied.clone();
    widget.expect_apply().returning(move |config| {
        widget_log
            .lock()
            .expect("widget log should not poison")
            .push(config);
        Box::pin(async { Ok(()) })
    });
    let mut tray = MockTrayRefresher::new();
    tray.expect_refresh_part()
        .returning(|| Box::pin(async { Ok(()) }));

    let executor = Arc::new(
        executor_with_os(
            os,
            Arc::new(MockLocaleSink::new()),
            Arc::new(MockLoggerRefresher::new()),
            Arc::new(widget),
            Arc::new(tray),
        )
        .await,
    );

    // Revision 1 enables the widget behind a system-proxy change.
    let overtaken = tokio::spawn({
        let executor = executor.clone();
        let plan = ApplicationEffectPlan::diff(
            &proxied_inputs(NyanpasuAppConfig::default()),
            &proxied_inputs(NyanpasuAppConfig {
                enable_system_proxy: true,
                network_statistic_widget: NetworkStatisticWidgetConfig::Enabled(
                    StatisticWidgetVariant::Small,
                ),
                ..NyanpasuAppConfig::default()
            }),
        );
        async move { executor.apply(EffectRevision::new(1), plan).await }
    });
    // A deadline rather than a sleep: if the earlier plan ever stops reaching
    // the os call this fails instead of hanging the suite.
    tokio::time::timeout(std::time::Duration::from_secs(10), entered_rx.recv())
        .await
        .expect("the first plan should reach the os proxy call")
        .expect("the gate sender outlives the actor");

    // Revision 2 disables it and finishes first, because nothing in it waits.
    let newer = executor
        .apply(
            EffectRevision::new(2),
            widget_plan(
                NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small),
                NetworkStatisticWidgetConfig::Disabled,
            ),
        )
        .await;
    assert_eq!(newer[0].health, EffectHealth::Healthy, "{newer:?}");

    release_tx.send(()).expect("the gated os call is waiting");
    let overtaken = overtaken.await.expect("the overtaken plan completes");

    let widget_status = overtaken
        .iter()
        .find(|status| status.kind == EffectKind::Widget)
        .expect("the overtaken plan carries the widget");
    assert_eq!(
        widget_status.health,
        EffectHealth::Superseded,
        "the user turned the widget off after this plan started: {overtaken:?}"
    );
    assert_eq!(
        applied
            .lock()
            .expect("widget log should not poison")
            .as_slice(),
        [NetworkStatisticWidgetConfig::Disabled],
        "the widget keeps the newest desired state"
    );
}
