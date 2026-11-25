//! Configuration convergence snapshots and source receipts, without owners or retry policy.

use nyanpasu_config::profile::ProfileId;
use serde::Serialize;

use crate::{candidate::CandidateSnapshotId, profiles::ProfilesError};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ConfigurationStatus {
    pub event_seq: u64,
    pub maintenance: Option<String>,
    pub source_versions: SourceVersions,
    pub runtime: RuntimeConvergence,
    pub effects: Vec<EffectConvergence>,
    /// The latest background-source receipt per profile.
    pub sources: Vec<SourceStatus>,
    /// The latest change per origin that no caller waited on and whose
    /// failure kept a candidate; held for this session only. Left out when
    /// empty, so the status reads as it did before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refused_changes: Vec<RefusedBackgroundChange>,
    pub active: Option<String>,
    pub recent_operations: Vec<OperationStatus>,
}
/// Where a change no caller waited on came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundChangeOrigin {
    /// Library sync applying the application settings.
    SyncApplication,
    /// Library sync applying the Clash settings.
    SyncClash,
    /// Library sync applying the profiles.
    SyncProfiles,
    /// A tray item or a global shortcut changing the proxy mode.
    ActionClashMode,
    /// A tray item or a global shortcut changing the system proxy.
    ActionSystemProxy,
    /// A tray item or a global shortcut changing TUN mode.
    ActionTunMode,
    /// A tray item or a global shortcut changing the transparent proxy.
    ActionTransparentProxy,
}

/// A change no caller waited on that the configuration check refused, or
/// whose commit failed after it, with the candidate it kept while this
/// session still holds it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RefusedBackgroundChange {
    pub origin: BackgroundChangeOrigin,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_snapshot_id: Option<CandidateSnapshotId>,
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
    pub message: Option<String>,
    /// The candidate the current failure kept for inspection in this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_snapshot_id: Option<CandidateSnapshotId>,
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
        /// The committed content's file is not completed yet; maintenance
        /// retries it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deferred: Option<DeferredCompletion>,
    },
    /// Its result no longer applied to the profile and was dropped (V31).
    Superseded { reason: String },
    Failed {
        message: String,
        /// The candidate the failure kept for inspection in this session.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        candidate_snapshot_id: Option<CandidateSnapshotId>,
    },
    /// The content reached the source transaction and was refused; nothing
    /// was committed and an external file is never rewritten (V33).
    Rejected {
        code: String,
        message: String,
        /// The candidate the failure kept for inspection in this session.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        candidate_snapshot_id: Option<CandidateSnapshotId>,
    },
}

impl SourceOutcome {
    pub const SUBSCRIPTION_REJECTED: &'static str = "subscription_rejected";
    pub const EXTERNAL_SOURCE_REJECTED: &'static str = "external_source_rejected";

    pub fn rejected(code: &str, error: impl std::fmt::Display) -> Self {
        Self::Rejected {
            code: code.into(),
            message: error.to_string(),
            candidate_snapshot_id: None,
        }
    }

    /// A change the source transaction refused, naming the candidate its
    /// error kept.
    pub fn refused(code: &str, error: &ProfilesError) -> Self {
        Self::Rejected {
            code: code.into(),
            message: error.report(),
            candidate_snapshot_id: error.candidate_snapshot_id().cloned(),
        }
    }

    /// A source that failed, naming the candidate its error kept.
    pub fn failed(error: &ProfilesError) -> Self {
        Self::Failed {
            message: error.report(),
            candidate_snapshot_id: error.candidate_snapshot_id().cloned(),
        }
    }

    pub fn health(&self) -> ConvergenceHealth {
        match self {
            Self::Committed {
                deferred: Some(_), ..
            } => ConvergenceHealth::RetryScheduled,
            Self::Committed { .. } | Self::Superseded { .. } => ConvergenceHealth::Healthy,
            Self::Failed { .. } | Self::Rejected { .. } => ConvergenceHealth::Blocked,
        }
    }
}

/// A committed source whose file could not be completed yet.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DeferredCompletion {
    pub message: String,
    /// The candidate the committed content was checked with, kept for
    /// inspection in this session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_snapshot_id: Option<CandidateSnapshotId>,
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
