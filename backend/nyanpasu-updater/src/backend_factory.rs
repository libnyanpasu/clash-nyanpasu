//! Deferred construction after the session's proxy-port source exists.

use std::{io, sync::Arc};

use nyanpasu_contracts::net::SelfProxyPortSource;
use snafu::Snafu;

use crate::UpdaterBackend;

/// Builds a backend against the active session's proxy ports at client setup time.
pub type BackendFactory = dyn Fn(Arc<dyn SelfProxyPortSource>) -> Result<Arc<dyn UpdaterBackend>, BackendFactoryError>
    + Send
    + Sync;

/// Failures resolving the executable directory for an update backend.
///
/// The facade maps each variant to its own setup error rather than wrapping
/// it, so these messages show only where the factory is called elsewhere.
#[derive(Debug, Snafu)]
pub enum BackendFactoryError {
    #[snafu(display("failed to locate executable"))]
    LocateExecutable { source: io::Error },
    #[snafu(display("executable has no parent directory"))]
    LocateExecutableParent,
}
