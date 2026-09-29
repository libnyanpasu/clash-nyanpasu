//! Failures of the operations that run on the core and the runtime workflow.
//!
//! One variant per thing that was being done, with what the user needs to
//! recognise it. Failures of the core itself keep their machine-readable kind
//! in [`CoreFailure`]; the frontend localizes both.

use nyanpasu_core_manager::{CoreError, CoreErrorKind};
use serde::Serialize;
use snafu::Snafu;

use crate::{
    client::{
        core_lifecycle::ports::InstallCoreBinaryError, ports::PortResolveError,
        runtime::PublishRuntimeError,
    },
    core::actor_v2::local_host::CoreSpecError,
    enhance::RuntimeBuildError,
};

/// The wire mirror of a [`CoreError`], which is a foreign type without serde.
/// It is the only place a `CoreError` is unpacked for the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct CoreFailure {
    /// `None` when the core manager did not classify the failure.
    pub kind: Option<CoreErrorKind>,
    pub message: String,
    pub retryable: bool,
    pub operation_id: Option<String>,
}

impl From<CoreError> for CoreFailure {
    fn from(error: CoreError) -> Self {
        Self {
            kind: error.kind,
            message: error.message,
            retryable: error.retryable,
            operation_id: error.operation_id.map(|id| id.to_string()),
        }
    }
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
#[snafu(visibility(pub(crate)))]
pub enum RuntimeError {
    /// Refused at admission: nothing ran.
    #[snafu(display("the application is shutting down"))]
    ShuttingDown,
    /// Refused at admission: nothing ran.
    #[snafu(display(
        "a previous operation left the runtime unsettled; verify the runtime from the \
         configuration status before anything else"
    ))]
    Isolated,
    /// The workflow took the command and never answered, so it may have run.
    #[snafu(display(
        "the runtime owner did not answer; the outcome of operation {operation_id} is unknown"
    ))]
    OwnerUnresponsive { operation_id: String },

    #[snafu(display("could not apply the runtime configuration: {failure}"))]
    ApplyRuntime {
        #[snafu(source(from(CoreError, CoreFailure::from)))]
        failure: CoreFailure,
    },
    #[snafu(display("could not stop the core: {failure}"))]
    StopCore {
        #[snafu(source(from(CoreError, CoreFailure::from)))]
        failure: CoreFailure,
    },
    #[snafu(display("could not recover the core: {failure}"))]
    RecoverRuntime {
        #[snafu(source(from(CoreError, CoreFailure::from)))]
        failure: CoreFailure,
    },
    #[snafu(display("could not recover the service endpoint: {failure}"))]
    RecoverServiceEndpoint {
        #[snafu(source(from(CoreError, CoreFailure::from)))]
        failure: CoreFailure,
    },
    #[snafu(display("could not read the core status: {failure}"))]
    RefreshStatus {
        #[snafu(source(from(CoreError, CoreFailure::from)))]
        failure: CoreFailure,
    },
    #[snafu(display("could not install the service: {failure}"))]
    InstallService {
        #[snafu(source(from(CoreError, CoreFailure::from)))]
        failure: CoreFailure,
    },
    #[snafu(display("could not start the service: {failure}"))]
    StartService {
        #[snafu(source(from(CoreError, CoreFailure::from)))]
        failure: CoreFailure,
    },
    #[snafu(display("could not stop the service: {failure}"))]
    StopService {
        #[snafu(source(from(CoreError, CoreFailure::from)))]
        failure: CoreFailure,
    },
    #[snafu(display("could not restart the service: {failure}"))]
    RestartService {
        #[snafu(source(from(CoreError, CoreFailure::from)))]
        failure: CoreFailure,
    },
    #[snafu(display("could not uninstall the service: {failure}"))]
    UninstallService {
        #[snafu(source(from(CoreError, CoreFailure::from)))]
        failure: CoreFailure,
    },
    /// The service hosts the runtime; the local host must take it over first.
    #[snafu(display(
        "the service still hosts the core; hand it to the local host before uninstalling the service"
    ))]
    ServiceHostsCore,

    /// An explicit start that could not put the core on its host; `reason` is
    /// the diagnostic text of the convergence that gave up.
    #[snafu(display("the core was not started: {reason}"))]
    CoreNotStarted { reason: String, retryable: bool },
    /// An explicit recovery that could not settle the runtime.
    #[snafu(display("the runtime could not be recovered: {reason}"))]
    RecoveryUnresolved { reason: String },

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
    #[snafu(display("could not render the runtime configuration"))]
    SerializeRuntimeConfig {
        #[serde(skip)]
        source: serde_yaml::Error,
    },
}

impl RuntimeError {
    /// The core's own failure this error carries, if the core produced it.
    pub(crate) fn core_failure(&self) -> Option<&CoreFailure> {
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
            | Self::UninstallService { failure } => Some(failure),
            Self::ShuttingDown
            | Self::Isolated
            | Self::OwnerUnresponsive { .. }
            | Self::ServiceHostsCore
            | Self::CoreNotStarted { .. }
            | Self::RecoveryUnresolved { .. }
            | Self::BuildRuntime { .. }
            | Self::PublishRuntime { .. }
            | Self::ResolvePort { .. }
            | Self::ResolveCoreBinary { .. }
            | Self::InstallCoreBinary { .. }
            | Self::SerializeRuntimeConfig { .. } => None,
        }
    }

    /// The classification the workflow decides by. A core binary that cannot
    /// be found is the core's `BinaryNotFound` as far as a refused candidate
    /// is concerned; a failure the core manager left unclassified has none.
    pub(crate) fn core_kind(&self) -> Option<CoreErrorKind> {
        match self {
            Self::ResolveCoreBinary { .. } => Some(CoreErrorKind::BinaryNotFound),
            _ => self.core_failure().and_then(|failure| failure.kind),
        }
    }

    /// Whether repeating the same operation can plausibly succeed.
    pub(crate) fn retryable(&self) -> bool {
        match self {
            Self::CoreNotStarted { retryable, .. } => *retryable,
            _ => self.core_failure().is_some_and(|failure| failure.retryable),
        }
    }
}
