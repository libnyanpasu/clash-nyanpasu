//! Service command failures shared by service-host and application tests.

use super::control::{ServiceCommand, ServiceCommandError};

/// A stand-in for whatever a mocked service host fails with.
pub(crate) fn service_command_error(reason: &'static str) -> ServiceCommandError {
    ServiceCommandError::RunElevated {
        command: ServiceCommand::Start,
        source: std::io::Error::other(reason),
    }
}
