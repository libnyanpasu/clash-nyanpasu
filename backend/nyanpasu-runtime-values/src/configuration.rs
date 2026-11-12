//! Configuration convergence snapshots and source receipts, without owners or retry policy.

use nyanpasu_config::profile::ProfileId;
use serde::Serialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ConfigurationStatus {
    pub event_seq: u64,
    pub maintenance: Option<String>,
    pub source_versions: SourceVersions,
    pub runtime: RuntimeConvergence,
    pub effects: Vec<EffectConvergence>,
    /// The latest background-source receipt per profile.
    pub sources: Vec<SourceStatus>,
    pub active: Option<String>,
    pub recent_operations: Vec<OperationStatus>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SourceVersions {
    pub application: u64,
    pub clash: u64,
    pub session: u64,
    pub profiles: u64,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RuntimeConvergence {
    pub health: ConvergenceHealth,
    pub operation_id: Option<String>,
    pub attempts: u32,
    pub automatic_remaining: u8,
    pub message: Option<String>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct EffectConvergence {
    pub kind: EffectKind,
    pub health: ConvergenceHealth,
    pub desired_revision: u64,
    pub applied_revision: u64,
    pub attempts: u32,
    pub automatic_remaining: u8,
    /// What kind of failure the effect is in, for the UI to localize.
    pub code: Option<EffectFailureCode>,
    /// The failure's diagnostic text, as the owner reported it.
    pub message: Option<String>,
}
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct OperationStatus {
    pub operation_id: String,
    pub domain: String,
    pub outcome: String,
    pub conclusion: String,
    pub message: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConvergenceHealth {
    Healthy,
    Pending,
    RetryScheduled,
    WaitingDependency,
    Blocked,
    RecoveryRequired,
}

/// Execution order of a plan. The ordering is load-bearing: the tray menu is
/// rendered with the process-wide locale, and the proxy guard re-applies the
/// system proxy value that the `SystemProxy` effect just installed.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    Locale,
    Logger,
    CoreLogLevel,
    CoreLogStorage,
    TransparentProxy,
    AutoLaunch,
    SystemProxy,
    ProxyGuard,
    Hotkeys,
    Widget,
    WidgetLogo,
    Tray,
}

/// Why an effect is not in its desired state, in the terms the UI localizes.
///
/// One value per kind of failure, named after the effect that failed, or
/// after the capability a shell lacks when several effects share it. The
/// owner that classifies a failure is the one that picks the code; the
/// message beside it is only the diagnostic text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EffectFailureCode {
    PresentationUnsupported,
    HotkeyInvalidBindings,
    HotkeyPartialRegistration,
    HotkeyShutDown,
    HotkeyStopped,
    LoggerRefreshFailed,
    CoreLogStorageFailed,
    TransparentProxyFailed,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SourceOrigin {
    ScheduledRefresh,
    ManualRefresh,
    ExternalFile,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceOutcome {
    Committed {
        operation_id: Option<String>,
    },
    /// Its result no longer applied to the profile and was dropped (V31).
    Superseded {
        reason: String,
    },
    Failed {
        message: String,
    },
    /// The content reached the source transaction and was refused; nothing
    /// was committed and an external file is never rewritten (V33).
    Rejected {
        code: String,
        message: String,
    },
}

impl SourceOutcome {
    pub const SUBSCRIPTION_REJECTED: &'static str = "subscription_rejected";
    pub const EXTERNAL_SOURCE_REJECTED: &'static str = "external_source_rejected";

    pub fn rejected(code: &str, error: impl std::fmt::Display) -> Self {
        Self::Rejected {
            code: code.into(),
            message: error.to_string(),
        }
    }

    pub fn health(&self) -> ConvergenceHealth {
        match self {
            Self::Committed { .. } | Self::Superseded { .. } => ConvergenceHealth::Healthy,
            Self::Failed { .. } | Self::Rejected { .. } => ConvergenceHealth::Blocked,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SourceStatus {
    pub profile: ProfileId,
    pub name: String,
    pub origin: SourceOrigin,
    pub outcome: SourceOutcome,
    pub health: ConvergenceHealth,
    /// Unix milliseconds.
    pub at: i64,
}
