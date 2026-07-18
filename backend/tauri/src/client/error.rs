use crate::core::storage::StorageOperationError;

pub type Result<T = ()> = std::result::Result<T, ClientError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyVergeDomain {
    Application,
    Session,
    Clash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompensationFailure {
    Conflict {
        domain: LegacyVergeDomain,
        expected_version: u64,
        actual_version: u64,
    },
    Error {
        domain: LegacyVergeDomain,
        message: String,
    },
    LegacyStateUncertain {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "legacy verge saga partially committed after {primary_error}; committed={committed_domains:?}, compensated={compensated_domains:?}, failed_compensations={failed_compensations:?}"
)]
pub struct PartialCommit {
    pub(crate) primary_error: String,
    pub(crate) committed_domains: Vec<LegacyVergeDomain>,
    pub(crate) compensated_domains: Vec<LegacyVergeDomain>,
    pub(crate) failed_compensations: Vec<CompensationFailure>,
}

impl PartialCommit {
    pub(crate) fn new(
        primary: &ClientError,
        committed_domains: Vec<LegacyVergeDomain>,
        compensated_domains: Vec<LegacyVergeDomain>,
        failed_compensations: Vec<CompensationFailure>,
    ) -> Self {
        Self {
            primary_error: format!("{primary:#}"),
            committed_domains,
            compensated_domains,
            failed_compensations,
        }
    }

    pub(crate) fn with_legacy_state_uncertain(mut self, message: String) -> Self {
        if let Some(CompensationFailure::LegacyStateUncertain { message: current }) = self
            .failed_compensations
            .iter_mut()
            .find(|failure| matches!(failure, CompensationFailure::LegacyStateUncertain { .. }))
        {
            current.push_str("; ");
            current.push_str(&message);
        } else {
            self.failed_compensations
                .push(CompensationFailure::LegacyStateUncertain { message });
        }
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    SerdeYaml(#[from] serde_yaml::Error),
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),
    #[error(transparent)]
    Storage(#[from] StorageOperationError),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    #[error(transparent)]
    Profiles(#[from] crate::state::profiles::actor::ProfilesError),
    #[error(transparent)]
    PartialCommit(#[from] PartialCommit),
    #[error("{0}")]
    Custom(String),
}

impl From<String> for ClientError {
    fn from(value: String) -> Self {
        Self::Custom(value)
    }
}

impl From<&str> for ClientError {
    fn from(value: &str) -> Self {
        Self::Custom(value.to_string())
    }
}
