use async_trait::async_trait;
use nyanpasu_config::application::ClashCore;
use nyanpasu_contracts::core_version::CoreVersionError;

#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait]
pub trait CoreVersionReader: Send + Sync + 'static {
    async fn read(&self, core: ClashCore) -> Result<String, CoreVersionError>;
}
