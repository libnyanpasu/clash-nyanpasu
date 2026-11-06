//! Desktop policy permitting the existing OS authorization dialog.

use nyanpasu_contracts::elevation::{ElevationConsent, ElevationConsentError, ElevationOperation};

pub(crate) struct InteractiveElevationConsent;

impl ElevationConsent for InteractiveElevationConsent {
    fn authorize(&self, _operation: ElevationOperation) -> Result<(), ElevationConsentError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_contracts::service::ServiceCommand;

    #[test]
    fn interactive_policy_adds_no_confirmation() {
        for operation in [
            ElevationOperation::DnsFlush,
            ElevationOperation::CoreBinaryCopy,
            ElevationOperation::Service {
                command: ServiceCommand::Install,
            },
            ElevationOperation::Service {
                command: ServiceCommand::Update,
            },
            ElevationOperation::Service {
                command: ServiceCommand::Uninstall,
            },
            ElevationOperation::Service {
                command: ServiceCommand::Start,
            },
            ElevationOperation::Service {
                command: ServiceCommand::Stop,
            },
            ElevationOperation::Service {
                command: ServiceCommand::Restart,
            },
        ] {
            InteractiveElevationConsent.authorize(operation).unwrap();
        }
    }
}
