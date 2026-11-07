//! Failures of publishing a runtime product.

use nyanpasu_contracts::profiles::error::ErrorPath;
use serde::Serialize;
use snafu::Snafu;

/// A failure of publishing the derived runtime config file.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub))]
pub enum PublishRuntimeError {
    #[snafu(display("could not create the runtime directory {path}"))]
    CreateRuntimeDirectory {
        path: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display("could not write the runtime config {path}"))]
    WriteRuntimeConfig {
        path: ErrorPath,
        #[serde(skip)]
        source: atomicwrites::Error<std::io::Error>,
    },
}
