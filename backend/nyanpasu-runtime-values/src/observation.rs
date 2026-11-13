//! Status values a `read` caller may see.
//!
//! [`ConfigurationStatus`], [`CoreStatusInfo`] and [`ServiceHostStatus`] carry the owners' diagnostic
//! text, controller locations and the daemon's filesystem layout. These summaries keep what a
//! dashboard needs to show health, progress and retry state: identities, counts, phases and flags.
//! Diagnostic text becomes `has_diagnostic` and locations become a kind, never a filtered string,
//! because no syntax check can tell whether a path or host holds a credential. Protected callers
//! read the full values.
//!
//! Every conversion destructures its source without `..`, so a new field does not compile until it
//! is classified. A closed enum keeps its variants; an open string is dropped or reduced to a flag.

use nyanpasu_config::profile::ProfileId;
use nyanpasu_ipc::{
    api::status::{CoreControllerInfo, CoreStateDetail, RevisionIdInfo},
    types::ServiceStatus,
};
use serde::{Deserialize, Serialize};

use crate::{
    configuration::{
        ConfigurationStatus, ConvergenceHealth, EffectConvergence, EffectFailureCode, EffectKind,
        OperationStatus, RuntimeConvergence, SourceOrigin, SourceOutcome, SourceStatus,
        SourceVersions,
    },
    core_status::{
        CoreStatusInfo, EndpointConnectivity, ServiceCompat, ServiceHostStatus, ServicePhase,
    },
    execution_host::ExecutionHost,
};

/// The secret-free view of [`ConfigurationStatus`].
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ConfigurationStatusSummary {
    pub event_seq: u64,
    /// A maintenance operation is running. Its description is diagnostic text.
    pub maintenance_active: bool,
    pub source_versions: SourceVersions,
    pub runtime: RuntimeConvergenceSummary,
    pub effects: Vec<EffectConvergenceSummary>,
    /// The latest background-source receipt per profile, without the profile's name.
    pub sources: Vec<SourceSummary>,
    pub active: Option<String>,
    pub recent_operations: Vec<OperationSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RuntimeConvergenceSummary {
    pub health: ConvergenceHealth,
    pub operation_id: Option<String>,
    pub attempts: u32,
    pub automatic_remaining: u8,
    /// The owner reported diagnostic text, available to protected callers.
    pub has_diagnostic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct EffectConvergenceSummary {
    pub kind: EffectKind,
    pub health: ConvergenceHealth,
    pub desired_revision: u64,
    pub applied_revision: u64,
    pub attempts: u32,
    pub automatic_remaining: u8,
    pub code: Option<EffectFailureCode>,
    pub has_diagnostic: bool,
}

/// One settled mutation. `domain`, `outcome` and `conclusion` name variants of closed enums.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OperationSummary {
    pub operation_id: String,
    pub domain: String,
    pub outcome: String,
    pub conclusion: String,
    pub has_diagnostic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SourceSummary {
    pub profile: ProfileId,
    pub origin: SourceOrigin,
    pub outcome: SourceOutcomeSummary,
    pub health: ConvergenceHealth,
    /// Unix milliseconds.
    pub at: i64,
}

/// The refusal reason a source receipt names, from the closed set the source owner raises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SourceRejection {
    SubscriptionRejected,
    ExternalSourceRejected,
    /// A code this build does not classify; never read as success.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceOutcomeSummary {
    Committed { operation_id: Option<String> },
    Superseded,
    Failed,
    Rejected { code: SourceRejection },
}

impl From<&ConfigurationStatus> for ConfigurationStatusSummary {
    fn from(status: &ConfigurationStatus) -> Self {
        let ConfigurationStatus {
            event_seq,
            maintenance,
            source_versions,
            runtime,
            effects,
            sources,
            active,
            recent_operations,
        } = status;
        Self {
            event_seq: *event_seq,
            maintenance_active: maintenance.is_some(),
            source_versions: source_versions.clone(),
            runtime: runtime.into(),
            effects: effects.iter().map(Into::into).collect(),
            sources: sources.iter().map(Into::into).collect(),
            active: active.clone(),
            recent_operations: recent_operations.iter().map(Into::into).collect(),
        }
    }
}

impl From<&RuntimeConvergence> for RuntimeConvergenceSummary {
    fn from(runtime: &RuntimeConvergence) -> Self {
        let RuntimeConvergence {
            health,
            operation_id,
            attempts,
            automatic_remaining,
            message,
        } = runtime;
        Self {
            health: *health,
            operation_id: operation_id.clone(),
            attempts: *attempts,
            automatic_remaining: *automatic_remaining,
            has_diagnostic: message.is_some(),
        }
    }
}

impl From<&EffectConvergence> for EffectConvergenceSummary {
    fn from(effect: &EffectConvergence) -> Self {
        let EffectConvergence {
            kind,
            health,
            desired_revision,
            applied_revision,
            attempts,
            automatic_remaining,
            code,
            message,
        } = effect;
        Self {
            kind: *kind,
            health: *health,
            desired_revision: *desired_revision,
            applied_revision: *applied_revision,
            attempts: *attempts,
            automatic_remaining: *automatic_remaining,
            code: *code,
            has_diagnostic: message.is_some(),
        }
    }
}

impl From<&OperationStatus> for OperationSummary {
    fn from(operation: &OperationStatus) -> Self {
        let OperationStatus {
            operation_id,
            domain,
            outcome,
            conclusion,
            message,
        } = operation;
        Self {
            operation_id: operation_id.clone(),
            domain: domain.clone(),
            outcome: outcome.clone(),
            conclusion: conclusion.clone(),
            has_diagnostic: message.is_some(),
        }
    }
}

impl From<&SourceStatus> for SourceSummary {
    fn from(source: &SourceStatus) -> Self {
        let SourceStatus {
            profile,
            // A free-form user label; the profile listing owns names.
            name: _,
            origin,
            outcome,
            health,
            at,
        } = source;
        Self {
            profile: profile.clone(),
            origin: *origin,
            outcome: outcome.into(),
            health: *health,
            at: *at,
        }
    }
}

impl From<&SourceOutcome> for SourceOutcomeSummary {
    fn from(outcome: &SourceOutcome) -> Self {
        match outcome {
            SourceOutcome::Committed { operation_id } => Self::Committed {
                operation_id: operation_id.clone(),
            },
            // Free-form reasons and messages can quote the failing request's URL.
            SourceOutcome::Superseded { reason: _ } => Self::Superseded,
            SourceOutcome::Failed { message: _ } => Self::Failed,
            SourceOutcome::Rejected { code, message: _ } => Self::Rejected {
                code: match code.as_str() {
                    SourceOutcome::SUBSCRIPTION_REJECTED => SourceRejection::SubscriptionRejected,
                    SourceOutcome::EXTERNAL_SOURCE_REJECTED => {
                        SourceRejection::ExternalSourceRejected
                    }
                    _ => SourceRejection::Other,
                },
            },
        }
    }
}

/// How the core's controller is reached. A socket path or URL is a location, not a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum ControllerKind {
    NamedPipe,
    UnixSocket,
    Http,
}

impl From<&CoreControllerInfo> for ControllerKind {
    fn from(controller: &CoreControllerInfo) -> Self {
        match controller {
            CoreControllerInfo::NamedPipe(_) => Self::NamedPipe,
            CoreControllerInfo::UnixSocket(_) => Self::UnixSocket,
            CoreControllerInfo::Http(_) => Self::Http,
        }
    }
}

/// [`CoreStateDetail`] without the stop reason's text or the process id. The encoding stays
/// externally tagged so a consumer that tests for the `Running` key keeps working.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum CoreStateSummary {
    Stopped { has_reason: bool },
    Starting { epoch: u64 },
    Running { epoch: u64 },
    Restarting { epoch: u64, attempt: u32 },
    Switching { from: Option<u64>, to: u64 },
    Stopping { epoch: u64 },
}

impl From<&CoreStateDetail> for CoreStateSummary {
    fn from(state: &CoreStateDetail) -> Self {
        match state {
            CoreStateDetail::Stopped { reason } => Self::Stopped {
                has_reason: reason.is_some(),
            },
            CoreStateDetail::Starting { epoch } => Self::Starting { epoch: *epoch },
            CoreStateDetail::Running { epoch, pid: _ } => Self::Running { epoch: *epoch },
            CoreStateDetail::Restarting { epoch, attempt } => Self::Restarting {
                epoch: *epoch,
                attempt: *attempt,
            },
            CoreStateDetail::Switching { from, to } => Self::Switching {
                from: *from,
                to: *to,
            },
            CoreStateDetail::Stopping { epoch } => Self::Stopping { epoch: *epoch },
        }
    }
}

/// The identity of the config revision the core adopted, without its content hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RevisionSummary {
    pub epoch: u64,
    pub generation: u64,
}

impl From<&RevisionIdInfo> for RevisionSummary {
    fn from(revision: &RevisionIdInfo) -> Self {
        let RevisionIdInfo {
            epoch,
            generation,
            // A fingerprint of the effective config, which holds the controller secret.
            effective_hash: _,
        } = revision;
        Self {
            epoch: *epoch,
            generation: *generation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EndpointConnectivitySummary {
    Connected,
    ShutDown,
    HandingOff {
        from: ExecutionHost,
        to: ExecutionHost,
    },
    /// Unreachable; the transport error stays with protected callers.
    Degraded {
        desired: ExecutionHost,
    },
}

impl From<&EndpointConnectivity> for EndpointConnectivitySummary {
    fn from(connectivity: &EndpointConnectivity) -> Self {
        match connectivity {
            EndpointConnectivity::Connected => Self::Connected,
            EndpointConnectivity::ShutDown => Self::ShutDown,
            EndpointConnectivity::HandingOff { from, to } => Self::HandingOff {
                from: *from,
                to: *to,
            },
            EndpointConnectivity::Degraded { desired, reason: _ } => {
                Self::Degraded { desired: *desired }
            }
        }
    }
}

/// The secret-free view of [`CoreStatusInfo`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CoreStatusSummary {
    pub controller: Option<ControllerKind>,
    pub host: ExecutionHost,
    pub connectivity: EndpointConnectivitySummary,
    pub generation: u64,
    pub state: Option<CoreStateSummary>,
    pub state_changed_at: i64,
    pub revision: Option<RevisionSummary>,
    /// `None` is unknown, never healthy.
    pub healthy: Option<bool>,
}

impl From<&CoreStatusInfo> for CoreStatusSummary {
    fn from(status: &CoreStatusInfo) -> Self {
        let CoreStatusInfo {
            controller,
            host,
            connectivity,
            generation,
            state,
            state_changed_at,
            revision,
            healthy,
        } = status;
        Self {
            controller: controller.as_ref().map(Into::into),
            host: *host,
            connectivity: connectivity.into(),
            generation: *generation,
            state: state.as_ref().map(Into::into),
            state_changed_at: *state_changed_at,
            revision: revision.as_ref().map(Into::into),
            healthy: *healthy,
        }
    }
}

/// [`ServiceCompat`] with the daemon's version only where it parsed as a version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ServiceCompatSummary {
    Unknown,
    Compatible {
        server_version: String,
    },
    Incompatible {
        server_version: String,
        required_major: u64,
        required_min: String,
    },
    /// The daemon reported text that is not a version, and the text is not repeated.
    Unparsable,
}

impl From<&ServiceCompat> for ServiceCompatSummary {
    fn from(compat: &ServiceCompat) -> Self {
        match compat {
            ServiceCompat::Unknown => Self::Unknown,
            ServiceCompat::Compatible { server_version } => Self::Compatible {
                server_version: server_version.clone(),
            },
            ServiceCompat::Incompatible {
                server_version,
                required_major,
                required_min,
            } => Self::Incompatible {
                server_version: server_version.clone(),
                required_major: *required_major,
                required_min: required_min.clone(),
            },
            ServiceCompat::Unparsable { server_version: _ } => Self::Unparsable,
        }
    }
}

/// The secret-free view of [`ServiceHostStatus`].
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ServiceStatusSummary {
    pub name: String,
    pub version: String,
    pub status: ServiceStatus,
    /// The daemon answered the probe. Its data directories and log locations stay protected.
    pub server_reported: bool,
    pub phase: ServicePhase,
    pub compat: ServiceCompatSummary,
    pub restart_attempts: u8,
}

impl From<&ServiceHostStatus> for ServiceStatusSummary {
    fn from(status: &ServiceHostStatus) -> Self {
        let ServiceHostStatus {
            name,
            version,
            status,
            server,
            phase,
            compat,
            restart_attempts,
            settled_ready: _,
            ready_generation: _,
        } = status;
        Self {
            name: name.to_string(),
            version: version.to_string(),
            status: *status,
            server_reported: server.is_some(),
            phase: *phase,
            compat: compat.into(),
            restart_attempts: *restart_attempts,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use serde_json::json;

    use super::*;
    use crate::configuration::{ConvergenceHealth, EffectKind};

    // Hostile values: a credential in a URL path, host, userinfo, query, header, bearer and secret field.
    const SENTINELS: [&str; 7] = [
        "sentinel-path-token",
        "sentinel-host-token.example",
        "sentinel-user:sentinel-pass",
        "sentinel-query-token",
        "sentinel-header-token",
        "sentinel-bearer-token",
        "sentinel-clash-secret",
    ];

    fn assert_clean(value: &impl Serialize) {
        let text = serde_json::to_string(value).unwrap();
        for sentinel in SENTINELS {
            assert!(!text.contains(sentinel), "{sentinel} leaked: {text}");
        }
    }

    fn hostile() -> String {
        format!(
            "GET https://{}@{}/{}/sub?token={} Authorization: Bearer {} {} {}",
            SENTINELS[2],
            SENTINELS[1],
            SENTINELS[0],
            SENTINELS[3],
            SENTINELS[5],
            SENTINELS[4],
            SENTINELS[6]
        )
    }

    fn configuration() -> ConfigurationStatus {
        let profile: ProfileId = serde_json::from_value(json!("pAbc1234")).unwrap();
        ConfigurationStatus {
            event_seq: 9,
            maintenance: Some(hostile()),
            source_versions: SourceVersions {
                application: 1,
                clash: 2,
                session: 3,
                profiles: 4,
            },
            runtime: RuntimeConvergence {
                health: ConvergenceHealth::RecoveryRequired,
                operation_id: Some("op-1".into()),
                attempts: 2,
                automatic_remaining: 1,
                message: Some(hostile()),
            },
            effects: vec![EffectConvergence {
                kind: EffectKind::SystemProxy,
                health: ConvergenceHealth::Blocked,
                desired_revision: 5,
                applied_revision: 4,
                attempts: 3,
                automatic_remaining: 0,
                code: Some(EffectFailureCode::SystemProxyApplyFailed),
                message: Some(hostile()),
            }],
            sources: vec![
                SourceStatus {
                    profile: profile.clone(),
                    name: hostile(),
                    origin: SourceOrigin::ScheduledRefresh,
                    outcome: SourceOutcome::Failed { message: hostile() },
                    health: ConvergenceHealth::Blocked,
                    at: 7,
                },
                SourceStatus {
                    profile: profile.clone(),
                    name: hostile(),
                    origin: SourceOrigin::ManualRefresh,
                    outcome: SourceOutcome::rejected(
                        SourceOutcome::SUBSCRIPTION_REJECTED,
                        hostile(),
                    ),
                    health: ConvergenceHealth::Blocked,
                    at: 8,
                },
                SourceStatus {
                    profile: profile.clone(),
                    name: hostile(),
                    origin: SourceOrigin::ExternalFile,
                    outcome: SourceOutcome::Superseded { reason: hostile() },
                    health: ConvergenceHealth::Healthy,
                    at: 9,
                },
                SourceStatus {
                    profile,
                    name: hostile(),
                    origin: SourceOrigin::ExternalFile,
                    outcome: SourceOutcome::Rejected {
                        code: hostile(),
                        message: hostile(),
                    },
                    health: ConvergenceHealth::Blocked,
                    at: 10,
                },
            ],
            active: Some("op-2".into()),
            recent_operations: vec![OperationStatus {
                operation_id: "op-3".into(),
                domain: "Profiles".into(),
                outcome: "Rejected".into(),
                conclusion: "Withdrawn".into(),
                message: Some(hostile()),
            }],
        }
    }

    #[test]
    fn configuration_summary_keeps_structure_and_drops_text() {
        let summary = ConfigurationStatusSummary::from(&configuration());
        assert_clean(&summary);

        assert!(summary.maintenance_active);
        assert_eq!(summary.event_seq, 9);
        assert_eq!(summary.runtime.health, ConvergenceHealth::RecoveryRequired);
        assert_eq!(summary.runtime.operation_id.as_deref(), Some("op-1"));
        assert_eq!(
            (
                summary.runtime.attempts,
                summary.runtime.automatic_remaining
            ),
            (2, 1)
        );
        assert!(summary.runtime.has_diagnostic);
        let effect = &summary.effects[0];
        assert_eq!(effect.code, Some(EffectFailureCode::SystemProxyApplyFailed));
        assert_eq!((effect.desired_revision, effect.applied_revision), (5, 4));
        assert!(effect.has_diagnostic);
        assert_eq!(summary.active.as_deref(), Some("op-2"));
        assert_eq!(summary.recent_operations[0].conclusion, "Withdrawn");
        assert!(summary.recent_operations[0].has_diagnostic);
    }

    #[test]
    fn source_outcomes_keep_a_reason_tag_and_never_read_as_success() {
        let summary = ConfigurationStatusSummary::from(&configuration());
        let outcomes: Vec<_> = summary.sources.iter().map(|s| s.outcome.clone()).collect();
        assert_eq!(
            outcomes,
            [
                SourceOutcomeSummary::Failed,
                SourceOutcomeSummary::Rejected {
                    code: SourceRejection::SubscriptionRejected
                },
                SourceOutcomeSummary::Superseded,
                // An unclassified code is still a refusal, and its text is not repeated.
                SourceOutcomeSummary::Rejected {
                    code: SourceRejection::Other
                },
            ]
        );
        assert_eq!(summary.sources[0].health, ConvergenceHealth::Blocked);
        assert_eq!(summary.sources[3].health, ConvergenceHealth::Blocked);
    }

    #[test]
    fn an_absent_diagnostic_stays_absent() {
        let mut status = configuration();
        status.maintenance = None;
        status.runtime.message = None;
        status.effects[0].message = None;
        status.recent_operations[0].message = None;
        let summary = ConfigurationStatusSummary::from(&status);
        assert!(!summary.maintenance_active);
        assert!(!summary.runtime.has_diagnostic);
        assert!(!summary.effects[0].has_diagnostic);
        assert!(!summary.recent_operations[0].has_diagnostic);
    }

    fn core(connectivity: EndpointConnectivity) -> CoreStatusInfo {
        CoreStatusInfo {
            controller: Some(CoreControllerInfo::Http(format!(
                "http://{}/{}?secret={}",
                SENTINELS[1], SENTINELS[0], SENTINELS[6]
            ))),
            host: ExecutionHost::Service,
            connectivity,
            generation: 4,
            state: Some(CoreStateDetail::Stopped {
                reason: Some(hostile()),
            }),
            state_changed_at: 11,
            revision: Some(RevisionIdInfo {
                epoch: 1,
                generation: 2,
                effective_hash: SENTINELS[6].into(),
            }),
            healthy: None,
        }
    }

    #[test]
    fn core_summary_reduces_the_controller_to_a_kind() {
        let summary = CoreStatusSummary::from(&core(EndpointConnectivity::Degraded {
            desired: ExecutionHost::Service,
            reason: hostile(),
        }));
        assert_clean(&summary);
        assert_eq!(summary.controller, Some(ControllerKind::Http));
        assert_eq!(
            summary.connectivity,
            EndpointConnectivitySummary::Degraded {
                desired: ExecutionHost::Service
            }
        );
        assert_eq!(
            summary.state,
            Some(CoreStateSummary::Stopped { has_reason: true })
        );
        assert_eq!(
            summary.revision,
            Some(RevisionSummary {
                epoch: 1,
                generation: 2
            })
        );
        // Unknown health is carried as unknown.
        assert_eq!(summary.healthy, None);
    }

    #[test]
    fn socket_controllers_keep_their_kind_not_their_path() {
        for (controller, kind) in [
            (
                CoreControllerInfo::UnixSocket(format!("/run/{}.sock", SENTINELS[0]).into()),
                ControllerKind::UnixSocket,
            ),
            (
                CoreControllerInfo::NamedPipe(format!(r"\\.\pipe\{}", SENTINELS[0]).into()),
                ControllerKind::NamedPipe,
            ),
        ] {
            assert_eq!(ControllerKind::from(&controller), kind);
            assert_clean(&kind);
        }
    }

    #[test]
    fn running_state_keeps_the_externally_tagged_key_without_the_pid() {
        let state = CoreStateSummary::from(&CoreStateDetail::Running {
            epoch: 3,
            pid: 4242,
        });
        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value, json!({ "Running": { "epoch": 3 } }));
    }

    #[test]
    fn every_connectivity_state_keeps_its_kind() {
        let kinds: Vec<_> = [
            EndpointConnectivity::Connected,
            EndpointConnectivity::ShutDown,
            EndpointConnectivity::HandingOff {
                from: ExecutionHost::Local,
                to: ExecutionHost::Service,
            },
            EndpointConnectivity::Degraded {
                desired: ExecutionHost::Local,
                reason: hostile(),
            },
        ]
        .iter()
        .map(|c| {
            serde_json::to_value(EndpointConnectivitySummary::from(c)).unwrap()["kind"].clone()
        })
        .collect();
        assert_eq!(kinds, ["connected", "shut_down", "handing_off", "degraded"]);
    }

    fn service(compat: ServiceCompat, with_server: bool) -> ServiceHostStatus {
        let server = with_server.then(|| {
            serde_json::from_value(json!({
                "version": "2.0.3",
                "core_infos": {
                    "type": null,
                    "state": "Running",
                    "state_changed_at": 1,
                    "config_path": format!("/home/{}/config.yaml", SENTINELS[0]),
                    "controller": { "Http": format!("http://{}/{}", SENTINELS[1], SENTINELS[0]) },
                },
                "runtime_infos": {
                    "service_data_dir": format!("/home/{}/data", SENTINELS[0]),
                    "service_config_dir": "/etc/x",
                    "nyanpasu_config_dir": "/etc/y",
                    "nyanpasu_data_dir": "/var/z",
                },
                "logs": { "service_dir": format!("/var/{}", SENTINELS[0]) },
            }))
            .expect("status body")
        });
        ServiceHostStatus {
            name: Cow::Borrowed("nyanpasu-service"),
            version: Cow::Borrowed("2.0.3"),
            status: ServiceStatus::Running,
            server,
            phase: ServicePhase::Unknown,
            compat,
            restart_attempts: 2,
            settled_ready: true,
            ready_generation: 3,
        }
    }

    #[test]
    fn service_summary_reports_the_probe_without_the_daemon_layout() {
        let summary = ServiceStatusSummary::from(&service(
            ServiceCompat::Unparsable {
                server_version: hostile(),
            },
            true,
        ));
        assert_clean(&summary);
        assert!(summary.server_reported);
        assert_eq!(summary.compat, ServiceCompatSummary::Unparsable);
        // The unreadable probe stays unknown rather than ready.
        assert_eq!(summary.phase, ServicePhase::Unknown);
        assert_eq!(summary.restart_attempts, 2);
    }

    #[test]
    fn service_compatibility_keeps_its_verdict() {
        let incompatible = ServiceCompatSummary::from(&ServiceCompat::Incompatible {
            server_version: "1.4.5".into(),
            required_major: 2,
            required_min: "2.0.3".into(),
        });
        assert!(matches!(
            incompatible,
            ServiceCompatSummary::Incompatible {
                required_major: 2,
                ..
            }
        ));
        assert_eq!(
            ServiceCompatSummary::from(&ServiceCompat::Unknown),
            ServiceCompatSummary::Unknown
        );
        assert!(
            !ServiceStatusSummary::from(&service(ServiceCompat::Unknown, false)).server_reported
        );
    }
}
