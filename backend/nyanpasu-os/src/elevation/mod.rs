//! Platform elevation mechanisms and the policy for shells without an authorization prompt.

use nyanpasu_contracts::elevation::{ElevationConsent, ElevationConsentError, ElevationOperation};

pub(crate) mod runner;

/// Refuses to request OS authorization, preserving the requested action.
#[derive(Debug)]
pub struct RefuseElevation;

impl ElevationConsent for RefuseElevation {
    fn authorize(&self, operation: ElevationOperation) -> Result<(), ElevationConsentError> {
        Err(ElevationConsentError::AuthorizeElevation { operation })
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod macos;
