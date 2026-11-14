//! Candidate documents shared by source owners and runtime consumers.

use std::sync::Arc;

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use nyanpasu_runtime_values::mutation::ConfigDomain;
use nyanpasu_state_engine::StateChange;

/// One source's proposed document; the other domains are sampled independently.
#[derive(Debug, Clone)]
pub enum DomainChange {
    Application {
        previous: Option<Arc<NyanpasuAppConfig>>,
        candidate: Arc<NyanpasuAppConfig>,
    },
    Clash {
        candidate: Arc<ClashConfig>,
    },
    Profiles {
        previous: Option<Arc<Profiles>>,
        candidate: Arc<Profiles>,
    },
}

impl DomainChange {
    pub fn domain(&self) -> ConfigDomain {
        match self {
            Self::Application { .. } => ConfigDomain::Application,
            Self::Clash { .. } => ConfigDomain::Clash,
            Self::Profiles { .. } => ConfigDomain::Profiles,
        }
    }
}

impl From<StateChange<NyanpasuAppConfig>> for DomainChange {
    fn from(change: StateChange<NyanpasuAppConfig>) -> Self {
        Self::Application {
            previous: change
                .previous
                .map(|previous| Arc::new(previous.state.clone())),
            candidate: change.current,
        }
    }
}

impl From<StateChange<ClashConfig>> for DomainChange {
    fn from(change: StateChange<ClashConfig>) -> Self {
        Self::Clash {
            candidate: change.current,
        }
    }
}

impl From<StateChange<Profiles>> for DomainChange {
    fn from(change: StateChange<Profiles>) -> Self {
        Self::Profiles {
            previous: change
                .previous
                .map(|previous| Arc::new(previous.state.clone())),
            candidate: change.current,
        }
    }
}
