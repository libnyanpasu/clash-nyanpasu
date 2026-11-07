//! The source-owned boundary for constructing a runtime mutation participant.

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
