//! Explicit application-boundary conversions for configuration.

use nyanpasu_core::{
    client::{
        configuration_status::{
            ConfigurationStatus as DomainConfigurationStatus,
            EffectConvergence as DomainEffectConvergence, OperationStatus as DomainOperationStatus,
            RuntimeConvergence as DomainRuntimeConvergence, SourceVersions as DomainSourceVersions,
        },
        convergence::ConvergenceHealth as DomainConvergenceHealth,
        debug_http::DebugHttpStatus as DomainDebugHttpStatus,
        effects::{
            plan::EffectKind as DomainEffectKind,
            status::EffectFailureCode as DomainEffectFailureCode,
        },
    },
    state::profiles::sources::{
        SourceOrigin as DomainSourceOrigin, SourceOutcome as DomainSourceOutcome,
        SourceStatus as DomainSourceStatus,
    },
};
use nyanpasu_rpc_protocol::wire;

use super::{IntoApplication, IntoWire};

impl IntoWire for DomainConfigurationStatus {
    type Wire = wire::configuration::ConfigurationStatus;

    fn into_wire(self) -> Self::Wire {
        wire::configuration::ConfigurationStatus {
            event_seq: self.event_seq,
            maintenance: self.maintenance,
            source_versions: self.source_versions.into_wire(),
            runtime: self.runtime.into_wire(),
            effects: self
                .effects
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
            sources: self
                .sources
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
            active: self.active,
            recent_operations: self
                .recent_operations
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
        }
    }
}

impl IntoWire for DomainSourceVersions {
    type Wire = wire::configuration::SourceVersions;

    fn into_wire(self) -> Self::Wire {
        wire::configuration::SourceVersions {
            application: self.application,
            clash: self.clash,
            session: self.session,
            profiles: self.profiles,
        }
    }
}

impl IntoWire for DomainRuntimeConvergence {
    type Wire = wire::configuration::RuntimeConvergence;

    fn into_wire(self) -> Self::Wire {
        wire::configuration::RuntimeConvergence {
            health: self.health.into_wire(),
            operation_id: self.operation_id,
            attempts: self.attempts,
            automatic_remaining: self.automatic_remaining,
            message: self.message,
        }
    }
}

impl IntoWire for DomainEffectConvergence {
    type Wire = wire::configuration::EffectConvergence;

    fn into_wire(self) -> Self::Wire {
        wire::configuration::EffectConvergence {
            kind: self.kind.into_wire(),
            health: self.health.into_wire(),
            desired_revision: self.desired_revision,
            applied_revision: self.applied_revision,
            attempts: self.attempts,
            automatic_remaining: self.automatic_remaining,
            code: self.code.map(|value| value.into_wire()),
            message: self.message,
        }
    }
}

impl IntoWire for DomainOperationStatus {
    type Wire = wire::configuration::OperationStatus;

    fn into_wire(self) -> Self::Wire {
        wire::configuration::OperationStatus {
            operation_id: self.operation_id,
            domain: self.domain,
            outcome: self.outcome,
            conclusion: self.conclusion,
            message: self.message,
        }
    }
}

impl IntoWire for DomainConvergenceHealth {
    type Wire = wire::configuration::ConvergenceHealth;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainConvergenceHealth::Healthy => wire::configuration::ConvergenceHealth::Healthy,
            DomainConvergenceHealth::Pending => wire::configuration::ConvergenceHealth::Pending,
            DomainConvergenceHealth::RetryScheduled => {
                wire::configuration::ConvergenceHealth::RetryScheduled
            }
            DomainConvergenceHealth::WaitingDependency => {
                wire::configuration::ConvergenceHealth::WaitingDependency
            }
            DomainConvergenceHealth::Blocked => wire::configuration::ConvergenceHealth::Blocked,
            DomainConvergenceHealth::RecoveryRequired => {
                wire::configuration::ConvergenceHealth::RecoveryRequired
            }
        }
    }
}

impl IntoWire for DomainEffectKind {
    type Wire = wire::configuration::EffectKind;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainEffectKind::Locale => wire::configuration::EffectKind::Locale,
            DomainEffectKind::Logger => wire::configuration::EffectKind::Logger,
            DomainEffectKind::CoreLogLevel => wire::configuration::EffectKind::CoreLogLevel,
            DomainEffectKind::CoreLogStorage => wire::configuration::EffectKind::CoreLogStorage,
            DomainEffectKind::AutoLaunch => wire::configuration::EffectKind::AutoLaunch,
            DomainEffectKind::SystemProxy => wire::configuration::EffectKind::SystemProxy,
            DomainEffectKind::ProxyGuard => wire::configuration::EffectKind::ProxyGuard,
            DomainEffectKind::Hotkeys => wire::configuration::EffectKind::Hotkeys,
            DomainEffectKind::Widget => wire::configuration::EffectKind::Widget,
            DomainEffectKind::WidgetLogo => wire::configuration::EffectKind::WidgetLogo,
            DomainEffectKind::Tray => wire::configuration::EffectKind::Tray,
        }
    }
}

impl IntoApplication for wire::configuration::EffectKind {
    type Application = DomainEffectKind;

    fn into_application(self) -> Self::Application {
        match self {
            wire::configuration::EffectKind::Locale => DomainEffectKind::Locale,
            wire::configuration::EffectKind::Logger => DomainEffectKind::Logger,
            wire::configuration::EffectKind::CoreLogLevel => DomainEffectKind::CoreLogLevel,
            wire::configuration::EffectKind::CoreLogStorage => DomainEffectKind::CoreLogStorage,
            wire::configuration::EffectKind::AutoLaunch => DomainEffectKind::AutoLaunch,
            wire::configuration::EffectKind::SystemProxy => DomainEffectKind::SystemProxy,
            wire::configuration::EffectKind::ProxyGuard => DomainEffectKind::ProxyGuard,
            wire::configuration::EffectKind::Hotkeys => DomainEffectKind::Hotkeys,
            wire::configuration::EffectKind::Widget => DomainEffectKind::Widget,
            wire::configuration::EffectKind::WidgetLogo => DomainEffectKind::WidgetLogo,
            wire::configuration::EffectKind::Tray => DomainEffectKind::Tray,
        }
    }
}

impl IntoWire for DomainEffectFailureCode {
    type Wire = wire::configuration::EffectFailureCode;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainEffectFailureCode::PresentationUnsupported => {
                wire::configuration::EffectFailureCode::PresentationUnsupported
            }
            DomainEffectFailureCode::HotkeyInvalidBindings => {
                wire::configuration::EffectFailureCode::HotkeyInvalidBindings
            }
            DomainEffectFailureCode::HotkeyPartialRegistration => {
                wire::configuration::EffectFailureCode::HotkeyPartialRegistration
            }
            DomainEffectFailureCode::HotkeyShutDown => {
                wire::configuration::EffectFailureCode::HotkeyShutDown
            }
            DomainEffectFailureCode::HotkeyStopped => {
                wire::configuration::EffectFailureCode::HotkeyStopped
            }
            DomainEffectFailureCode::LoggerRefreshFailed => {
                wire::configuration::EffectFailureCode::LoggerRefreshFailed
            }
            DomainEffectFailureCode::CoreLogStorageFailed => {
                wire::configuration::EffectFailureCode::CoreLogStorageFailed
            }
            DomainEffectFailureCode::WidgetUnavailable => {
                wire::configuration::EffectFailureCode::WidgetUnavailable
            }
            DomainEffectFailureCode::WidgetApplyFailed => {
                wire::configuration::EffectFailureCode::WidgetApplyFailed
            }
            DomainEffectFailureCode::TrayRefreshFailed => {
                wire::configuration::EffectFailureCode::TrayRefreshFailed
            }
            DomainEffectFailureCode::EffectOwnerSilent => {
                wire::configuration::EffectFailureCode::EffectOwnerSilent
            }
            DomainEffectFailureCode::ProxyGuardWaitingDependency => {
                wire::configuration::EffectFailureCode::ProxyGuardWaitingDependency
            }
            DomainEffectFailureCode::AutoLaunchFailed => {
                wire::configuration::EffectFailureCode::AutoLaunchFailed
            }
            DomainEffectFailureCode::PacDisableFailed => {
                wire::configuration::EffectFailureCode::PacDisableFailed
            }
            DomainEffectFailureCode::PacApplyFailed => {
                wire::configuration::EffectFailureCode::PacApplyFailed
            }
            DomainEffectFailureCode::PacUnsupported => {
                wire::configuration::EffectFailureCode::PacUnsupported
            }
            DomainEffectFailureCode::SystemProxyApplyFailed => {
                wire::configuration::EffectFailureCode::SystemProxyApplyFailed
            }
            DomainEffectFailureCode::SystemProxyPortUnresolved => {
                wire::configuration::EffectFailureCode::SystemProxyPortUnresolved
            }
            DomainEffectFailureCode::SystemProxyRestoreFailed => {
                wire::configuration::EffectFailureCode::SystemProxyRestoreFailed
            }
            DomainEffectFailureCode::SystemProxyShutDown => {
                wire::configuration::EffectFailureCode::SystemProxyShutDown
            }
            DomainEffectFailureCode::SystemProxyStopped => {
                wire::configuration::EffectFailureCode::SystemProxyStopped
            }
        }
    }
}

impl IntoWire for DomainSourceOrigin {
    type Wire = wire::configuration::SourceOrigin;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainSourceOrigin::ScheduledRefresh => {
                wire::configuration::SourceOrigin::ScheduledRefresh
            }
            DomainSourceOrigin::ManualRefresh => wire::configuration::SourceOrigin::ManualRefresh,
            DomainSourceOrigin::ExternalFile => wire::configuration::SourceOrigin::ExternalFile,
        }
    }
}

impl IntoWire for DomainSourceOutcome {
    type Wire = wire::configuration::SourceOutcome;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainSourceOutcome::Committed { operation_id } => {
                wire::configuration::SourceOutcome::Committed { operation_id }
            }
            DomainSourceOutcome::Superseded { reason } => {
                wire::configuration::SourceOutcome::Superseded { reason }
            }
            DomainSourceOutcome::Failed { message } => {
                wire::configuration::SourceOutcome::Failed { message }
            }
            DomainSourceOutcome::Rejected { code, message } => {
                wire::configuration::SourceOutcome::Rejected { code, message }
            }
        }
    }
}

impl IntoWire for DomainSourceStatus {
    type Wire = wire::configuration::SourceStatus;

    fn into_wire(self) -> Self::Wire {
        wire::configuration::SourceStatus {
            profile: self.profile,
            name: self.name,
            origin: self.origin.into_wire(),
            outcome: self.outcome.into_wire(),
            health: self.health.into_wire(),
            at: self.at,
        }
    }
}

impl IntoWire for DomainDebugHttpStatus {
    type Wire = wire::configuration::DebugHttpStatus;

    fn into_wire(self) -> Self::Wire {
        wire::configuration::DebugHttpStatus {
            enabled: self.enabled,
            url: self.url,
        }
    }
}
