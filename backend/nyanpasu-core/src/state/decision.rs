//! One authoritative, write-once source outcome, including local resource settlement.

use super::version::Version;
use std::sync::{Arc, OnceLock};
use tokio::sync::Notify;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistenceIncident {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbortResourceState {
    Restored,
    NeedsRecovery(PersistenceIncident),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateDecision {
    Undecided,
    Committed { version: Version },
    Aborted { resources: AbortResourceState },
}

#[derive(Debug, Default)]
struct DecisionCell {
    // Shared only to publish a single immutable result from the source owner.
    outcome: OnceLock<StateDecision>,
    ready: Notify,
}

#[derive(Debug, Clone)]
pub struct DecisionHandle(Arc<DecisionCell>);

impl DecisionHandle {
    pub fn decision(&self) -> StateDecision {
        self.0
            .outcome
            .get()
            .cloned()
            .unwrap_or(StateDecision::Undecided)
    }

    pub async fn wait(&self) -> StateDecision {
        loop {
            let ready = self.0.ready.notified();
            tokio::pin!(ready);
            ready.as_mut().enable();
            if let Some(outcome) = self.0.outcome.get() {
                return outcome.clone();
            }
            ready.await;
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DecisionWriter(Arc<DecisionCell>);

impl DecisionWriter {
    pub(crate) fn new() -> Self {
        Self(Arc::new(DecisionCell::default()))
    }

    pub(crate) fn handle(&self) -> DecisionHandle {
        DecisionHandle(Arc::clone(&self.0))
    }

    fn publish(&self, outcome: StateDecision) {
        if self.0.outcome.set(outcome).is_ok() {
            self.0.ready.notify_waiters();
        }
    }

    pub(crate) fn commit(&self, version: Version) {
        self.publish(StateDecision::Committed { version });
    }

    pub(crate) fn abort(&self, resources: AbortResourceState) {
        self.publish(StateDecision::Aborted { resources });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_first_complete_outcome_wins_and_wakes_all_readers() {
        let writer = DecisionWriter::new();
        let handle = writer.handle();
        assert_eq!(handle.decision(), StateDecision::Undecided);
        let a = handle.wait();
        let b = handle.wait();
        let publish = async {
            writer.commit(Version::new(7));
            writer.abort(AbortResourceState::NeedsRecovery(PersistenceIncident {
                message: "late abort".into(),
            }));
        };
        let (a, b, ()) = tokio::join!(a, b, publish);
        let expected = StateDecision::Committed {
            version: Version::new(7),
        };
        assert_eq!(a, expected);
        assert_eq!(b, expected);
        assert_eq!(handle.wait().await, expected);
    }

    #[tokio::test]
    async fn abort_publishes_resource_evidence_atomically() {
        let writer = DecisionWriter::new();
        let resources = AbortResourceState::NeedsRecovery(PersistenceIncident {
            message: "resource restore failed".into(),
        });
        writer.abort(resources.clone());
        writer.commit(Version::new(7));
        assert_eq!(
            writer.handle().wait().await,
            StateDecision::Aborted { resources }
        );
    }
}
