//! Conclusions sampled by response waiters without involving the workflow mailbox.
use nyanpasu_core_manager::OperationId;
use nyanpasu_runtime_values::{
    mutation::ConfigDomain, runtime::Degradation, runtime_error::RuntimeError,
};
use nyanpasu_state_engine::Version;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) enum Conclusion {
    Converged,
    Inactive,
    NotInEffect { cause: Arc<RuntimeError> },
    Waiting { cause: Arc<RuntimeError> },
    Unknown { cause: Arc<RuntimeError> },
    Closed,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EngineVersions {
    pub application: Version,
    pub clash: Version,
    pub profiles: Version,
}
impl EngineVersions {
    pub(crate) fn of(&self, domain: ConfigDomain) -> Version {
        match domain {
            ConfigDomain::Application => self.application,
            ConfigDomain::Clash => self.clash,
            ConfigDomain::Profiles => self.profiles,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ReconcilerStatus {
    pub seq: u64,
    pub active: Option<OperationId>,
    pub seen: EngineVersions,
    pub conclusion: Conclusion,
    pub degradations: Vec<Degradation>,
}
impl Default for ReconcilerStatus {
    fn default() -> Self {
        Self {
            seq: 0,
            active: None,
            seen: EngineVersions::default(),
            conclusion: Conclusion::Unknown {
                cause: Arc::new(RuntimeError::Isolated),
            },
            degradations: Vec::new(),
        }
    }
}
