//! Source-owned boundaries for independent checks and runtime participants.

use nyanpasu_state_engine::{DecisionHandle, StateParticipant};
use tokio::sync::oneshot;

use super::{intent::MutationIntent, value::MutationReceipt};

/// Builds one participant with the source transaction's authoritative decision and settlement.
pub(crate) trait MutationParticipantFactory<T>: Send + Sync + 'static
where
    T: Clone + Send + Sync + 'static,
{
    fn make(
        &self,
        intent: MutationIntent,
        decision: DecisionHandle,
        settle: oneshot::Sender<MutationReceipt>,
    ) -> StateParticipant<T>;
}

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
