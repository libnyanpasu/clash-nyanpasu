//! Desktop status projection: core convergence plus the committed window session version.
use super::session_state::SessionStateClient;
use nyanpasu_core::{
    NyanpasuClient,
    client::configuration_status::{EffectConvergence, OperationStatus, RuntimeConvergence},
    state::profiles::sources::SourceStatus,
};

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ConfigurationStatus {
    pub event_seq: u64,
    pub maintenance: Option<String>,
    pub source_versions: SourceVersions,
    pub runtime: RuntimeConvergence,
    pub effects: Vec<EffectConvergence>,
    /// The latest background-source receipt per profile.
    pub sources: Vec<SourceStatus>,
    pub active: Option<String>,
    pub recent_operations: Vec<OperationStatus>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct SourceVersions {
    pub application: u64,
    pub clash: u64,
    pub session: u64,
    pub profiles: u64,
}

#[derive(Clone)]
pub struct ConfigurationStatusClient {
    core: NyanpasuClient,
    session: SessionStateClient,
}

impl ConfigurationStatusClient {
    pub(crate) fn new(core: NyanpasuClient, session: SessionStateClient) -> Self {
        Self {
            core: core,
            session: session,
        }
    }

    pub fn configuration_status(&self) -> ConfigurationStatus {
        let status = self.core.configuration_status();
        // Capture once: the payload version and sequence must describe the same commit.
        let session_version = self.session.snapshot().version;
        ConfigurationStatus {
            event_seq: status.event_seq + session_version,
            maintenance: status.maintenance,
            source_versions: SourceVersions {
                application: status.source_versions.application,
                clash: status.source_versions.clash,
                session: session_version,
                profiles: status.source_versions.profiles,
            },
            runtime: status.runtime,
            effects: status.effects,
            sources: status.sources,
            active: status.active,
            recent_operations: status.recent_operations,
        }
    }
}
