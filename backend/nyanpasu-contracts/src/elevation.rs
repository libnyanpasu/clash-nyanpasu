//! Policy for requesting OS authorization, without performing IO or showing UI.

use serde::Serialize;
use snafu::Snafu;

use crate::service::ServiceCommand;

/// The action requesting authorization, never an executable or command string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ElevationOperation {
    Service { command: ServiceCommand },
    DnsFlush,
    CoreBinaryCopy,
}

impl std::fmt::Display for ElevationOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Service { command } => write!(f, "the service {command} command"),
            Self::DnsFlush => f.write_str("the DNS cache flush"),
            Self::CoreBinaryCopy => f.write_str("the core binary copy"),
        }
    }
}

/// The shell cannot request OS authorization for this action.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ElevationConsentError {
    #[snafu(display("administrator access cannot be requested for {operation}"))]
    AuthorizeElevation { operation: ElevationOperation },
}

/// Decides synchronously, without IO, whether an operation may request OS authorization.
/// Success permits the existing OS prompt; it does not pre-grant OS authorization.
/// Cancelling that prompt remains an execution failure of the consuming mechanism.
pub trait ElevationConsent: Send + Sync + 'static {
    /// Returns a refusal before any authorization helper is prepared or launched.
    ///
    /// # Errors
    /// Returns `AuthorizeElevation` when this shell cannot request authorization.
    fn authorize(&self, operation: ElevationOperation) -> Result<(), ElevationConsentError>;
}
