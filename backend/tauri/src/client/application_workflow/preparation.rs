use std::sync::Arc;

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use nyanpasu_core::state::StateSnapshot;
use nyanpasu_core_manager::{CoreError, CoreSpec, LocalIpcPolicy, LocalIpcSettings};

use super::{super::runtime, ports::RuntimeBuildPort};
use crate::client::core_lifecycle::{
    domain_error,
    ports::{PreparedRuntime, RuntimePreparationPort},
};

/// Builds runtime candidates from committed source config. It holds read-only
/// state handles, never a domain client: the workflow is a state participant
/// and must not be able to write a source domain it is applying for.
pub(super) struct RuntimePreparation {
    application: StateSnapshot<NyanpasuAppConfig>,
    clash: StateSnapshot<ClashConfig>,
    profiles: StateSnapshot<Profiles>,
    builder: Arc<dyn RuntimeBuildPort>,
    revisions: runtime::RuntimeRevisionAllocator,
}

impl RuntimePreparation {
    pub fn new(
        application: StateSnapshot<NyanpasuAppConfig>,
        clash: StateSnapshot<ClashConfig>,
        profiles: StateSnapshot<Profiles>,
        builder: Arc<dyn RuntimeBuildPort>,
    ) -> Self {
        Self {
            application,
            clash,
            profiles,
            builder,
            revisions: runtime::RuntimeRevisionAllocator::new(),
        }
    }

    pub async fn prepare(
        &mut self,
        profiles: Arc<Profiles>,
        clash: ClashConfig,
        app: NyanpasuAppConfig,
    ) -> Result<PreparedRuntime, CoreError> {
        let revision = self.revisions.allocate().map_err(domain_error)?;
        let local_ipc = LocalIpcSettings {
            policy: match clash.clash_control_channel {
                nyanpasu_config::clash::config::ClashControlChannel::PreferIpc => {
                    LocalIpcPolicy::Prefer
                }
                nyanpasu_config::clash::config::ClashControlChannel::HttpOnly => {
                    LocalIpcPolicy::Disable
                }
            },
            keep_http_controller: !clash.clash_ipc_disable_http_controller,
        };
        let snapshot = self
            .builder
            .build(revision, profiles, clash, app)
            .await
            .map_err(domain_error)?;
        Ok(PreparedRuntime {
            snapshot,
            local_ipc,
        })
    }

    pub async fn prepare_committed(
        &mut self,
        profiles: Arc<Profiles>,
        clash: ClashConfig,
    ) -> Result<PreparedRuntime, CoreError> {
        let app = self.application.load().state.clone();
        self.prepare(profiles, clash, app).await
    }
}

#[async_trait::async_trait]
impl RuntimePreparationPort for RuntimePreparation {
    async fn prepare_latest(&mut self) -> Result<PreparedRuntime, CoreError> {
        // Independent committed snapshots; changes during a build retain a dirty pass.
        let profiles = Arc::new(self.profiles.load().state.clone());
        let clash = self.clash.load().state.clone();
        self.prepare_committed(profiles, clash).await
    }

    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot) -> anyhow::Result<()> {
        self.builder.publish(snapshot).await
    }

    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> anyhow::Result<CoreSpec> {
        self.builder.core_spec(core)
    }
}
