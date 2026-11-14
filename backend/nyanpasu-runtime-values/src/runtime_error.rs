//! Failures of the operations that run on the core and the runtime workflow.
//!
//! One variant per thing that was being done, with what the user needs to
//! recognise it. Failures of the core itself keep their machine-readable kind
//! in [`CoreFailure`]; the frontend localizes both.

use nyanpasu_contracts::{
    core_install::InstallCoreBinaryError, core_version::CoreVersionError,
    service::ServiceCommandError,
};
use nyanpasu_core_metadata::CoreErrorKind;
use nyanpasu_profile_transform::RuntimeBuildError;
use serde::Serialize;
use snafu::Snafu;

use super::{
    check::RuntimeCheckUnavailable, core_spec::CoreSpecError, execution_host::ExecutionHost,
    ports::PortResolveError, publish::PublishRuntimeError,
};

/// The wire mirror of a `CoreError`, which is a foreign type without serde.
/// It is the only place a `CoreError` is unpacked for the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
pub struct CoreFailure {
    /// `None` when the core manager did not classify the failure.
    pub kind: Option<CoreErrorKind>,
    pub message: String,
    pub retryable: bool,
    pub operation_id: Option<String>,
}

impl std::fmt::Display for CoreFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            Some(kind) => write!(f, "{kind}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for CoreFailure {}

/// A failure of an operation on the running core or the runtime workflow.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub))]
pub enum RuntimeError {
    /// Refused at admission: nothing ran.
    #[snafu(display("the application is shutting down"))]
    ShuttingDown,
    /// The workflow took the command and never answered, so it may have run.
    #[snafu(display(
        "the runtime owner did not answer; the outcome of operation {operation_id} is unknown"
    ))]
    OwnerUnresponsive { operation_id: String },

    #[snafu(display("could not apply the runtime configuration: {failure}"))]
    ApplyRuntime {
        #[snafu(source)]
        failure: CoreFailure,
    },
    #[snafu(display("could not stop the core: {failure}"))]
    StopCore {
        #[snafu(source)]
        failure: CoreFailure,
    },
    #[snafu(display("could not recover the core: {failure}"))]
    RecoverRuntime {
        #[snafu(source)]
        failure: CoreFailure,
    },
    #[snafu(display("could not recover the service endpoint: {failure}"))]
    RecoverServiceEndpoint {
        #[snafu(source)]
        failure: CoreFailure,
    },
    #[snafu(display("could not read the core status: {failure}"))]
    RefreshStatus {
        #[snafu(source)]
        failure: CoreFailure,
    },
    #[snafu(display("could not install the service: {failure}"))]
    InstallService {
        #[snafu(source)]
        failure: CoreFailure,
    },
    #[snafu(display("could not start the service: {failure}"))]
    StartService {
        #[snafu(source)]
        failure: CoreFailure,
    },
    #[snafu(display("could not stop the service: {failure}"))]
    StopService {
        #[snafu(source)]
        failure: CoreFailure,
    },
    #[snafu(display("could not restart the service: {failure}"))]
    RestartService {
        #[snafu(source)]
        failure: CoreFailure,
    },
    #[snafu(display("could not uninstall the service: {failure}"))]
    UninstallService {
        #[snafu(source)]
        failure: CoreFailure,
    },
    #[snafu(display("could not move the runtime to the {host:?} execution host: {failure}"))]
    MoveHost {
        host: ExecutionHost,
        #[snafu(source)]
        failure: CoreFailure,
    },
    /// The service hosts the runtime; the local host must take it over first.
    #[snafu(display(
        "the service still hosts the core; hand it to the local host before uninstalling the service"
    ))]
    ServiceHostsCore,

    /// The committed configuration is waiting or has not taken effect.
    #[snafu(display("the committed configuration is not in effect: {reason}"))]
    ConfigurationNotInEffect { reason: String },

    #[snafu(display("could not build the runtime configuration: {source}"))]
    BuildRuntime { source: RuntimeBuildError },
    #[snafu(display("could not publish the runtime configuration: {source}"))]
    PublishRuntime { source: PublishRuntimeError },
    #[snafu(display("could not resolve the ports of the runtime: {source}"))]
    ResolvePort { source: PortResolveError },
    #[snafu(display("could not find the core to run: {source}"))]
    ResolveCoreBinary { source: CoreSpecError },
    #[snafu(display("could not install the core binary: {source}"))]
    InstallCoreBinary { source: InstallCoreBinaryError },
    #[snafu(display("could not prepare the service install command: {source}"))]
    PrepareServiceInstallPrompt { source: ServiceCommandError },
    #[snafu(display("could not read the version of the core: {source}"))]
    ReadCoreVersion { source: CoreVersionError },

    #[snafu(display("the core rejected the configuration: {message}"))]
    CoreRejectedConfig {
        core_kind: Option<CoreErrorKind>,
        message: String,
    },
    #[snafu(display("the configuration could not be checked: {reason:?}"))]
    CheckUnavailable { reason: RuntimeCheckUnavailable },
    /// The core restored its own previous configuration.
    #[snafu(display("the core would not start this configuration and kept the previous one"))]
    CoreRolledBack { reason: Option<String> },
    /// The submission ended unobserved, so it may have taken effect.
    #[snafu(display("the runtime submission is unobserved: {failure}"))]
    SubmissionUnobserved {
        #[snafu(source)]
        failure: CoreFailure,
    },

    /// No runtime configuration has been built yet.
    #[snafu(display("there is no runtime configuration yet"))]
    NoRuntimeConfig,
    #[snafu(display("could not render the runtime configuration"))]
    SerializeRuntimeConfig {
        #[serde(skip)]
        source: yaml_serde::Error,
    },
    #[snafu(display("could not convert the runtime configuration"))]
    ConvertRuntimeConfig {
        #[serde(skip)]
        source: serde_json::Error,
    },
    /// A newer build replaced the snapshot the inspection was opened on.
    #[snafu(display("the runtime snapshot changed; refresh the inspection"))]
    RuntimeSnapshotChanged,
    #[snafu(display("the runtime snapshot has no node {node_id}"))]
    RuntimeNodeNotFound { node_id: u32 },
}

impl RuntimeError {
    /// The core's own failure this error carries, if the core produced it.
    fn core_failure(&self) -> Option<&CoreFailure> {
        match self {
            Self::ApplyRuntime { failure }
            | Self::StopCore { failure }
            | Self::RecoverRuntime { failure }
            | Self::RecoverServiceEndpoint { failure }
            | Self::RefreshStatus { failure }
            | Self::InstallService { failure }
            | Self::StartService { failure }
            | Self::StopService { failure }
            | Self::RestartService { failure }
            | Self::UninstallService { failure }
            | Self::MoveHost { failure, .. }
            | Self::SubmissionUnobserved { failure } => Some(failure),
            Self::ShuttingDown
            | Self::OwnerUnresponsive { .. }
            | Self::CoreRejectedConfig { .. }
            | Self::CheckUnavailable { .. }
            | Self::CoreRolledBack { .. }
            | Self::ServiceHostsCore
            | Self::ConfigurationNotInEffect { .. }
            | Self::BuildRuntime { .. }
            | Self::PublishRuntime { .. }
            | Self::ResolvePort { .. }
            | Self::ResolveCoreBinary { .. }
            | Self::InstallCoreBinary { .. }
            | Self::PrepareServiceInstallPrompt { .. }
            | Self::ReadCoreVersion { .. }
            | Self::NoRuntimeConfig
            | Self::SerializeRuntimeConfig { .. }
            | Self::ConvertRuntimeConfig { .. }
            | Self::RuntimeSnapshotChanged
            | Self::RuntimeNodeNotFound { .. } => None,
        }
    }

    /// The classification the workflow decides by. A core binary that cannot
    /// be found is the core's `BinaryNotFound` as far as a refused candidate
    /// is concerned; a failure the core manager left unclassified has none.
    pub fn core_kind(&self) -> Option<CoreErrorKind> {
        match self {
            Self::ResolveCoreBinary { .. } => Some(CoreErrorKind::BinaryNotFound),
            _ => self.core_failure().and_then(|failure| failure.kind),
        }
    }

    /// Whether repeating the same operation can plausibly succeed.
    pub fn retryable(&self) -> bool {
        self.core_failure().is_some_and(|failure| failure.retryable)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CoreFailure, RuntimeError};
    use crate::{check::RuntimeCheckUnavailable, execution_host::ExecutionHost};
    use nyanpasu_core_metadata::CoreErrorKind;

    #[test]
    fn committed_configuration_cause_names_what_is_not_in_effect() {
        let error = RuntimeError::ConfigurationNotInEffect {
            reason: "waiting for a stable runtime".into(),
        };
        assert_eq!(
            error.to_string(),
            "the committed configuration is not in effect: waiting for a stable runtime"
        );
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({
                "kind": "configuration_not_in_effect",
                "reason": "waiting for a stable runtime",
            })
        );
    }

    #[test]
    fn runtime_error_serialization_preserves_existing_tags_and_fields() {
        let failure = CoreFailure {
            kind: Some(CoreErrorKind::ApplyFailed),
            message: "apply failed".into(),
            retryable: true,
            operation_id: Some("operation-1".into()),
        };
        let wire_failure = json!({
            "kind": "apply_failed", "message": "apply failed", "retryable": true,
            "operation_id": "operation-1",
        });
        for (error, kind) in [
            (
                RuntimeError::ApplyRuntime {
                    failure: failure.clone(),
                },
                "apply_runtime",
            ),
            (
                RuntimeError::StopCore {
                    failure: failure.clone(),
                },
                "stop_core",
            ),
            (
                RuntimeError::RecoverRuntime {
                    failure: failure.clone(),
                },
                "recover_runtime",
            ),
            (
                RuntimeError::RecoverServiceEndpoint {
                    failure: failure.clone(),
                },
                "recover_service_endpoint",
            ),
            (
                RuntimeError::RefreshStatus {
                    failure: failure.clone(),
                },
                "refresh_status",
            ),
            (
                RuntimeError::InstallService {
                    failure: failure.clone(),
                },
                "install_service",
            ),
            (
                RuntimeError::StartService {
                    failure: failure.clone(),
                },
                "start_service",
            ),
            (
                RuntimeError::StopService {
                    failure: failure.clone(),
                },
                "stop_service",
            ),
            (
                RuntimeError::RestartService {
                    failure: failure.clone(),
                },
                "restart_service",
            ),
            (
                RuntimeError::UninstallService {
                    failure: failure.clone(),
                },
                "uninstall_service",
            ),
            (
                RuntimeError::SubmissionUnobserved {
                    failure: failure.clone(),
                },
                "submission_unobserved",
            ),
        ] {
            assert_eq!(
                serde_json::to_value(error).unwrap(),
                json!({ "kind": kind, "failure": wire_failure })
            );
        }
        assert_eq!(
            serde_json::to_value(RuntimeError::MoveHost {
                host: ExecutionHost::Service,
                failure: failure.clone()
            })
            .unwrap(),
            json!({"kind": "move_host", "host": "service", "failure": wire_failure})
        );
        assert_eq!(
            serde_json::to_value(RuntimeError::CheckUnavailable {
                reason: RuntimeCheckUnavailable::Backend {
                    kind: Some(CoreErrorKind::ApplyFailed),
                    message: "check failed".into(),
                    retryable: false
                },
            })
            .unwrap(),
            json!({"kind": "check_unavailable", "reason": {"cause": "backend", "kind": "apply_failed", "message": "check failed", "retryable": false}})
        );
        let source = yaml_serde::from_str::<yaml_serde::Value>("[").unwrap_err();
        assert_eq!(
            serde_json::to_value(RuntimeError::SerializeRuntimeConfig { source }).unwrap(),
            json!({"kind": "serialize_runtime_config"})
        );
        assert_eq!(
            serde_json::to_value(CoreFailure {
                kind: None,
                message: "unclassified".into(),
                retryable: false,
                operation_id: None
            })
            .unwrap(),
            json!({"kind": null, "message": "unclassified", "retryable": false, "operation_id": null})
        );
    }
}
