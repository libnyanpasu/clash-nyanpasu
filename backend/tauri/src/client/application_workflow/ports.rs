use super::super::runtime;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub(in crate::client) trait RuntimeBuildPort: Send + Sync + 'static {
    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> anyhow::Result<nyanpasu_core_manager::CoreSpec>;
    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        profiles: Arc<nyanpasu_config::profile::Profiles>,
        clash: nyanpasu_config::clash::config::ClashConfig,
        app: nyanpasu_config::application::NyanpasuAppConfig,
    ) -> anyhow::Result<Arc<runtime::RuntimeSnapshot>>;
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot) -> anyhow::Result<()>;
}
