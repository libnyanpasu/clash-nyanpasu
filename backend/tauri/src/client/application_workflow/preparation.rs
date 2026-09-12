use std::sync::Arc;

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use nyanpasu_core_manager::{CoreError, CoreSpec, LocalIpcPolicy, LocalIpcSettings};

use super::{
    super::{
        application::ApplicationClient, clash_config::ClashConfigClient, profiles::ProfilesClient,
        runtime,
    },
    ports::RuntimeBuildPort,
};
use crate::client::core_lifecycle::{
    domain_error,
    ports::{PreparedRuntime, RuntimePreparationPort},
};

pub(super) struct RuntimePreparation {
    application: ApplicationClient,
    clash: ClashConfigClient,
    profiles: ProfilesClient,
    builder: Arc<dyn RuntimeBuildPort>,
    revisions: runtime::RuntimeRevisionAllocator,
}

impl RuntimePreparation {
    pub fn new(
        application: ApplicationClient,
        clash: ClashConfigClient,
        profiles: ProfilesClient,
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
        let app = self.application.get().await.map_err(domain_error)?.state;
        self.prepare(profiles, clash, app).await
    }
}

#[async_trait::async_trait]
impl RuntimePreparationPort for RuntimePreparation {
    async fn prepare_latest(&mut self) -> Result<PreparedRuntime, CoreError> {
        // Independent committed snapshots; changes during a build retain a dirty pass.
        let profiles = self.profiles.get().await.map_err(domain_error)?;
        let clash = self.clash.get().await.map_err(domain_error)?.state;
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
