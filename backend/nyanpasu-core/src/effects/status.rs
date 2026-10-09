//! Result protocol for applied effects: per-effect health plus the revision
//! bookkeeping that lets an owner drop a superseded reconcile.

use serde::Serialize;

use super::EffectKind;

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

/// Why an effect is not in its desired state, in the terms the UI localizes.
///
/// One value per kind of failure, named after the effect that failed. The
/// owner that classifies a failure is the one that picks the code; the
/// message beside it is only the diagnostic text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EffectFailureCode {
    PresentationUnsupported,
    HotkeyInvalidBindings,
    HotkeyPartialRegistration,
    HotkeyShutDown,
    HotkeyStopped,
    LoggerRefreshFailed,
    CoreLogStorageFailed,
    WidgetUnavailable,
    WidgetApplyFailed,
    TrayRefreshFailed,
    EffectOwnerSilent,
    ProxyGuardWaitingDependency,
    AutoLaunchFailed,
    PacDisableFailed,
    PacApplyFailed,
    PacUnsupported,
    SystemProxyApplyFailed,
    SystemProxyPortUnresolved,
    SystemProxyRestoreFailed,
    SystemProxyShutDown,
    SystemProxyStopped,
}

impl EffectFailureCode {
    /// The failure is a startup-ordering fact rather than a fault: the
    /// effect is waiting for something that has not been resolved yet.
    pub const fn waits_for_dependency(self) -> bool {
        matches!(
            self,
            Self::WidgetUnavailable
                | Self::SystemProxyPortUnresolved
                | Self::ProxyGuardWaitingDependency
        )
    }
}

/// The diagnostic text of a failure: its message, then each cause down the
/// chain. Unlike `Display` it keeps what the library underneath reported.
pub fn failure_text(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        text.push_str(": ");
        text.push_str(&next.to_string());
        cause = next.source();
    }
    text
}

// Every variant but `Healthy` is constructed by the effect owners, which are
// added one task at a time.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectHealth {
    Pending,
    Healthy,
    Degraded {
        code: EffectFailureCode,
        message: String,
        retryable: bool,
    },
    /// A newer reconcile already applied a higher revision.
    Superseded,
    /// The platform or a dependency cannot provide the effect at all.
    Unsupported {
        code: EffectFailureCode,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectStatus {
    pub kind: EffectKind,
    pub desired_revision: EffectRevision,
    /// Last successfully installed desired revision. Failed attempts never advance it.
    pub applied_revision: EffectRevision,
    pub health: EffectHealth,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_text_joins_the_whole_chain() {
        #[derive(Debug, snafu::Snafu)]
        #[snafu(display("outer"))]
        struct Outer {
            source: std::io::Error,
        }
        let error = Outer {
            source: std::io::Error::other("inner"),
        };

        assert_eq!(failure_text(&error), "outer: inner");
    }

    #[test]
    fn revision_is_monotonically_comparable() {
        assert!(EffectRevision::new(1) < EffectRevision::new(2));
        assert_eq!(EffectRevision::default(), EffectRevision::new(0));
        assert_eq!(EffectRevision::new(7).get(), 7);
    }
}
