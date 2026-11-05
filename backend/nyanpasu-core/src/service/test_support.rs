//! Service command failures shared by service-host and application tests.

use nyanpasu_contracts::service::{ServiceCommand, ServiceCommandError};

/// A stand-in for whatever a mocked service host fails with.
pub fn service_command_error(reason: &'static str) -> ServiceCommandError {
    ServiceCommandError::RunElevated {
        command: ServiceCommand::Start,
        source: std::io::Error::other(reason),
    }
}
