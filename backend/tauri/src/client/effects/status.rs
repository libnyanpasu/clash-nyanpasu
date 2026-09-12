//! Result protocol for applied effects: per-effect health plus the revision
//! bookkeeping that lets an owner drop a superseded reconcile.

use super::plan::EffectKind;
use crate::client::runtime::{Degradation, DegradationPhase};

/// Monotonic counter over committed desired configurations, allocated by the
/// facade after a commit.
///
/// Distinct from a typed actor's per-domain `version`: one patch can bump two
/// unrelated domain versions, which cannot be totally ordered. Not persisted —
/// a restart begins at zero and is reconciled by a full plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct EffectRevision(u64);

impl EffectRevision {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    // Read by the effect owners that compare it against their applied revision.
    #[allow(dead_code)]
    pub const fn get(self) -> u64 {
        self.0
    }
}

// Every variant but `Healthy` is constructed by the effect owners, which are
// added one task at a time.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectHealth {
    Healthy,
    Degraded {
        /// Stable snake_case code, not a free-form phrase.
        code: &'static str,
        message: String,
        retryable: bool,
    },
    /// A newer reconcile already applied a higher revision.
    Superseded,
    /// The platform or a dependency cannot provide the effect at all.
    Unsupported {
        code: &'static str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectStatus {
    pub kind: EffectKind,
    pub desired_revision: EffectRevision,
    /// The revision the effect owner has installed.
    ///
    /// For [`EffectKind::Tray`] this is the attempt sequence instead: a
    /// refresh carries no value, so nothing about it is "installed" and the
    /// plan revision cannot order two of them. The executor numbers the
    /// attempts in the order they run and reports the number here.
    pub applied_revision: EffectRevision,
    pub health: EffectHealth,
}

/// Only a genuine failure degrades a mutation outcome. `Superseded` is the
/// normal result of concurrency and `Unsupported` is a platform fact, so
/// neither reaches the caller as a degradation.
pub fn degradation_of(status: &EffectStatus) -> Option<Degradation> {
    let EffectHealth::Degraded {
        code,
        message,
        retryable,
    } = &status.health
    else {
        return None;
    };

    Some(Degradation {
        phase: phase_of(status.kind),
        code: (*code).to_owned(),
        message: message.clone(),
        retryable: *retryable,
    })
}

const fn phase_of(kind: EffectKind) -> DegradationPhase {
    match kind {
        EffectKind::Locale | EffectKind::Logger | EffectKind::Widget | EffectKind::Tray => {
            DegradationPhase::UiEffect
        }
        EffectKind::AutoLaunch
        | EffectKind::SystemProxy
        | EffectKind::ProxyGuard
        | EffectKind::Hotkeys => DegradationPhase::SystemEffect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(kind: EffectKind, health: EffectHealth) -> EffectStatus {
        EffectStatus {
            kind,
            desired_revision: EffectRevision::new(4),
            applied_revision: EffectRevision::new(4),
            health,
        }
    }

    fn degraded() -> EffectHealth {
        EffectHealth::Degraded {
            code: "system_proxy_apply_failed",
            message: "os refused".to_owned(),
            retryable: true,
        }
    }

    #[test]
    fn degradation_maps_system_kinds_to_system_effect_phase() {
        for kind in [
            EffectKind::AutoLaunch,
            EffectKind::SystemProxy,
            EffectKind::ProxyGuard,
            EffectKind::Hotkeys,
        ] {
            let degradation = degradation_of(&status(kind, degraded()))
                .unwrap_or_else(|| panic!("{kind:?} must degrade"));
            assert_eq!(degradation.phase, DegradationPhase::SystemEffect);
            assert_eq!(degradation.code, "system_proxy_apply_failed");
            assert_eq!(degradation.message, "os refused");
            assert!(degradation.retryable);
        }
    }

    #[test]
    fn degradation_maps_ui_kinds_to_ui_effect_phase() {
        for kind in [
            EffectKind::Locale,
            EffectKind::Logger,
            EffectKind::Widget,
            EffectKind::Tray,
        ] {
            let degradation = degradation_of(&status(kind, degraded()))
                .unwrap_or_else(|| panic!("{kind:?} must degrade"));
            assert_eq!(degradation.phase, DegradationPhase::UiEffect);
        }
    }

    #[test]
    fn healthy_superseded_and_unsupported_produce_no_degradation() {
        for health in [
            EffectHealth::Healthy,
            EffectHealth::Superseded,
            EffectHealth::Unsupported {
                code: "pac_unsupported",
            },
        ] {
            assert_eq!(
                degradation_of(&status(EffectKind::SystemProxy, health)),
                None
            );
        }
    }

    #[test]
    fn revision_is_monotonically_comparable() {
        assert!(EffectRevision::new(1) < EffectRevision::new(2));
        assert_eq!(EffectRevision::default(), EffectRevision::new(0));
        assert_eq!(EffectRevision::new(7).get(), 7);
    }
}
