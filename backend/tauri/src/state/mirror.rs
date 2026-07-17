use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, state::PersistentState,
};

/// A fully prepared, single-use legacy projection.
pub trait PreparedLegacyMirror: Send {
    /// Apply only the prepared in-memory projection. This must not fail or perform IO.
    fn apply(self: Box<Self>);
}

pub(crate) struct PreparedTypedReplace<T> {
    state: T,
    mirror: Box<dyn PreparedLegacyMirror>,
}

impl<T> PreparedTypedReplace<T> {
    pub(crate) fn new(state: T, mirror: Box<dyn PreparedLegacyMirror>) -> Self {
        Self { state, mirror }
    }

    pub(crate) fn into_parts(self) -> (T, Box<dyn PreparedLegacyMirror>) {
        (self.state, self.mirror)
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for PreparedTypedReplace<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedTypedReplace")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
pub(crate) struct NoopPreparedLegacyMirror;

#[cfg(test)]
impl PreparedLegacyMirror for NoopPreparedLegacyMirror {
    fn apply(self: Box<Self>) {}
}

/// Compatibility boundary for typed application config infrastructure.
pub trait VergeLegacyBridge: Send + Sync + 'static {
    fn prepare(&self, snap: &NyanpasuAppConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>>;

    /// Legacy seed hook for takeover phases.
    fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig>;
}

/// Compatibility boundary for typed session/window state infrastructure.
pub trait WindowLegacyBridge: Send + Sync + 'static {
    fn prepare(&self, snap: &PersistentState) -> anyhow::Result<Box<dyn PreparedLegacyMirror>>;

    /// Legacy seed hook for takeover phases.
    fn snapshot_legacy(&self) -> anyhow::Result<PersistentState>;
}

/// Compatibility boundary for persistent Clash config infrastructure.
/// This is separate from the live runtime Clash API used by `clash_api_get_configs`.
pub trait ClashLegacyBridge: Send + Sync + 'static {
    fn prepare(&self, snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>>;

    /// Legacy seed hook for takeover phases.
    fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig>;
}
