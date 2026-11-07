//! Failures of locating a core binary.

use nyanpasu_config::application::ClashCore;
use serde::Serialize;
use snafu::Snafu;

/// A failure of locating the binary a core is started from.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub))]
pub enum CoreSpecError {
    #[snafu(display("could not find the {core} core binary"))]
    FindCoreBinary {
        #[specta(type = String)]
        core: ClashCore,
        #[serde(skip)]
        source: std::io::Error,
    },
}
