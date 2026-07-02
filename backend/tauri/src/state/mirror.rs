use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, state::PersistentState,
};

/// Compatibility boundary for typed application config infrastructure.
pub trait VergeLegacyBridge: Send + Sync + 'static {
    fn mirror(&self, snap: &NyanpasuAppConfig) -> anyhow::Result<()>;

    /// Legacy seed hook for takeover phases.
    fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig>;
}

/// Compatibility boundary for typed session/window state infrastructure.
pub trait WindowLegacyBridge: Send + Sync + 'static {
    fn mirror(&self, snap: &PersistentState) -> anyhow::Result<()>;

    /// Legacy seed hook for takeover phases.
    fn snapshot_legacy(&self) -> anyhow::Result<PersistentState>;
}

/// Compatibility boundary for persistent Clash config infrastructure.
/// This is separate from the live runtime Clash API used by `clash_api_get_configs`.
pub trait ClashLegacyBridge: Send + Sync + 'static {
    fn mirror(&self, snap: &ClashConfig) -> anyhow::Result<()>;

    /// Legacy seed hook for takeover phases.
    fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig>;
}
