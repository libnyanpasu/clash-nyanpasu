//! The effects port of a host that has no presentation or OS-integration owners.

use nyanpasu_core::client::effects::{
    plan::{ApplicationEffectPlan, EffectKind},
    ports::ApplicationEffectsPort,
    status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus},
};

/// Reports every planned effect as unsupported.
///
/// The desktop executor needs system-proxy, hotkey, widget, tray and logger
/// collaborators. A headless host owns none of them, so each effect is
/// reported as not applied rather than as healthy: `applied_revision` never
/// advances. `NoopApplicationEffects` would report every effect healthy and is
/// test-only for that reason.
#[derive(Debug, Default)]
pub struct UnsupportedEffects;

#[async_trait::async_trait]
impl ApplicationEffectsPort for UnsupportedEffects {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        plan.effects()
            .iter()
            .map(|effect| {
                let kind = effect.kind();
                EffectStatus {
                    kind,
                    desired_revision: revision,
                    applied_revision: EffectRevision::default(),
                    health: EffectHealth::Unsupported {
                        code: unsupported_code(kind),
                    },
                }
            })
            .collect()
    }
}

// SPIKE: finding F1. `EffectFailureCode` has no generic "unsupported" or "not
// applicable" value; each code names a failure of a desktop effect. The nearest
// code is used, and `Locale` has none, so it borrows `EffectOwnerSilent`.
const fn unsupported_code(kind: EffectKind) -> EffectFailureCode {
    match kind {
        EffectKind::Locale | EffectKind::CoreLogLevel => EffectFailureCode::EffectOwnerSilent,
        EffectKind::Logger => EffectFailureCode::LoggerRefreshFailed,
        EffectKind::CoreLogStorage => EffectFailureCode::CoreLogStorageFailed,
        EffectKind::AutoLaunch => EffectFailureCode::AutoLaunchFailed,
        EffectKind::SystemProxy => EffectFailureCode::SystemProxyApplyFailed,
        EffectKind::ProxyGuard => EffectFailureCode::SystemProxyStopped,
        EffectKind::Hotkeys => EffectFailureCode::HotkeyStopped,
        EffectKind::Widget | EffectKind::WidgetLogo => EffectFailureCode::WidgetUnavailable,
        EffectKind::Tray => EffectFailureCode::TrayRefreshFailed,
    }
}
