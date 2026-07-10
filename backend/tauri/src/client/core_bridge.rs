//! Boundary adapter for "apply the regenerated runtime to the running core"
//! (PR-3 T07). The facade depends on this trait so it stays testable; the
//! production impl concentrates the legacy-global touches behind two
//! documented bridges.

use async_trait::async_trait;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RunningCoreBridge: Send + Sync + 'static {
    async fn apply_config(&self) -> anyhow::Result<()>;
    async fn on_profile_change(&self);
}

pub struct LegacyCoreBridge;

#[async_trait]
impl RunningCoreBridge for LegacyCoreBridge {
    async fn apply_config(&self) -> anyhow::Result<()> {
        // TODO(actor-migration): temporary bridge to CoreManager::global().
        // Reason: core lifecycle is PR-5 (CoreActor).
        // Remove when: PR-5 lands CoreActor and the facade owns core apply.
        crate::core::CoreManager::global().apply_config().await
    }

    async fn on_profile_change(&self) {
        // TODO(actor-migration): connection interruption still reads Config::verge()
        // inside the service. Reason: break_when_* options + clash api client are
        // PR-4/PR-6 scope. Remove when: interruption reads typed
        // ClashConfig.break_connection via an injected client.
        let _ =
            crate::core::connection_interruption::ConnectionInterruptionService::on_profile_change(
            )
            .await;
    }
}
