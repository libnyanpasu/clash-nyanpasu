//! Public port resolution failures.

use nyanpasu_config::clash::config::clash_strategy::port::PickPortError;
use serde::Serialize;
use snafu::Snafu;

/// The port a resolution was picking for.
#[derive(serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PortField {
    Mixed,
    Http,
    Socks,
    ExternalController,
}

/// A failure of resolving the ports a candidate runtime would bind.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub))]
pub enum PortResolveError {
    #[snafu(display("failed to resolve the {field:?} port: {source}"))]
    ResolvePort {
        field: PortField,
        source: PickPortError,
    },
}
