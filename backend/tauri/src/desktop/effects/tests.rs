//! Presentation ordering and failure semantics over injected capabilities.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use nyanpasu_config::application::{
    I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig, NyanpasuAppConfig,
};
use nyanpasu_core::client::effects::{
    plan::{ApplicationEffect, ApplicationEffectPlan, EffectKind, TrayRefresh},
    ports::{ApplicationEffectsPort, LoggerError, MockLoggerRefresher},
    status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus},
};
use nyanpasu_widget_protocol::StatisticWidgetVariant;

use super::{
    ports::{MockLocaleSink, MockTrayRefresher, MockWidgetController, TrayError},
    test_support::{CallLog, calls, executor, inputs, record, spawn_refused},
};

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

#[tokio::test]
async fn failed_full_refresh_is_retried_until_success_then_part_stays_part() {
    let log: CallLog = Arc::default();
    let full_calls = Arc::new(AtomicUsize::new(0));
    let mut tray = MockTrayRefresher::new();
    let full_log = log.clone();
    let attempts = full_calls.clone();
    tray.expect_refresh_full().times(2).returning(move |_| {
        let first = attempts.fetch_add(1, Ordering::SeqCst) == 0;
        record(&full_log, "refresh_full");
        Box::pin(async move {
            if first {
                Err(TrayError::ScheduleTrayWork {
                    source: "tray refused".into(),
                })
            } else {
                Ok(())
            }
        })
    });
    let part_log = log.clone();
    tray.expect_refresh_part().times(1).returning(move |_| {
        record(&part_log, "refresh_part");
        Box::pin(async { Ok(()) })
    });
    let executor = executor(
        Arc::new(MockLocaleSink::new()),
        Arc::new(MockLoggerRefresher::new()),
        Arc::new(MockWidgetController::new()),
        Arc::new(tray),
    )
    .await;
    let view = inputs(NyanpasuAppConfig::default()).tray_view();

    let failed = executor
        .apply(
            EffectRevision::new(1),
            ApplicationEffectPlan::from_effects(vec![ApplicationEffect::Tray(
                TrayRefresh::Full,
                view,
            )]),
        )
        .await;
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].kind, EffectKind::Tray);
    assert_eq!(failed[0].desired_revision, EffectRevision::new(1));
    assert_eq!(failed[0].applied_revision, EffectRevision::default());
    assert!(matches!(
        failed[0].health,
        EffectHealth::Degraded {
            code: EffectFailureCode::TrayRefreshFailed,
            retryable: true,
            ..
        }
    ));
    assert_eq!(full_calls.load(Ordering::SeqCst), 1);
    assert_eq!(calls(&log), ["refresh_full"]);

    let part =
        ApplicationEffectPlan::from_effects(vec![ApplicationEffect::Tray(TrayRefresh::Part, view)]);
    let retried = executor.apply(EffectRevision::new(2), part.clone()).await;
    assert_eq!(
        retried,
        vec![EffectStatus {
            kind: EffectKind::Tray,
            desired_revision: EffectRevision::new(2),
            applied_revision: EffectRevision::new(2),
            health: EffectHealth::Healthy,
        }]
    );
    assert_eq!(full_calls.load(Ordering::SeqCst), 2);
    assert_eq!(calls(&log), ["refresh_full", "refresh_full"]);

    let refreshed = executor.apply(EffectRevision::new(3), part).await;
    assert_eq!(
        refreshed,
        vec![EffectStatus {
            kind: EffectKind::Tray,
            desired_revision: EffectRevision::new(3),
            applied_revision: EffectRevision::new(3),
            health: EffectHealth::Healthy,
        }]
    );
    assert_eq!(full_calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        calls(&log),
        ["refresh_full", "refresh_full", "refresh_part"]
    );
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
