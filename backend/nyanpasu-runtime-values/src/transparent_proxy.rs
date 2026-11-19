//! Read-only transparent proxy observations exposed to the desktop UI.

use nyanpasu_ipc::api::network::transparent_proxy::NetworkTransparentProxyDiagnostics;
use serde::{Deserialize, Serialize};

/// Availability of the Linux service diagnostics endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransparentProxyDiagnostics {
    /// The service answered with its applied scope and observed kernel objects.
    Available {
        diagnostics: NetworkTransparentProxyDiagnostics,
    },
    /// An installed service predates the diagnostics endpoint.
    UpdateRequired,
    /// The host is not Linux or does not run the privileged service.
    Unsupported,
}
