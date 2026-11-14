//! Source-owned boundaries for independent checks and committed-state notifications.

/// A candidate is checked without changing the running core or publishing source state.
pub(crate) struct RuntimeCandidate {
    pub change: super::DomainChange,
    pub staged: std::collections::BTreeMap<String, String>,
}

/// Source-owned boundary for independent checks and committed-state notifications.
#[async_trait::async_trait]
pub(crate) trait RuntimeGate: Send + Sync + 'static {
    async fn check(
        &self,
        candidate: RuntimeCandidate,
    ) -> Result<(), std::sync::Arc<nyanpasu_runtime_values::runtime_error::RuntimeError>>;
    fn committed(
        &self,
        domain: nyanpasu_runtime_values::mutation::ConfigDomain,
        version: nyanpasu_state_engine::Version,
    );
}
