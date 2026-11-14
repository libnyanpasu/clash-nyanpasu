//! Startup-injected runtime checks and post-commit effect notifications.
mod change;
mod profile_content;
pub(crate) use change::DomainChange;
pub use profile_content::ProfileContentLock;
pub(crate) mod impact;
pub(crate) mod ports;
pub(crate) mod runtime_apply;

use std::sync::Arc;

use nyanpasu_state_engine::ReplaceIfVersionError;
use snafu::{IntoError as _, ensure};
use tokio::sync::watch;

use crate::client::effects::ports::CommitNotifications;
use nyanpasu_runtime_values::mutation::{
    CommitAborted, NotReady, NotReadySnafu, RecoverAfterWriteFailureSnafu, RuntimeAftermath,
    ValidateStateSnafu, WriteConfigSnafu,
};

#[derive(Clone)]
enum Connection {
    Pending,
    Ready {
        effects: Arc<dyn CommitNotifications>,
        runtime: Arc<dyn ports::RuntimeGate>,
    },
    #[cfg(any(test, feature = "test-support"))]
    Isolated,
}

/// Only the composition root completes this connection. Domain actors can be
/// loaded first to supply read-only snapshots, and the effects owner is
/// spawned from them; no production mutation can run before the workflow and
/// the effects owner are connected. The channel carries startup wiring, not
/// shared mutable actor state.
#[derive(Clone)]
pub struct MutationCoordinator(watch::Sender<Connection>);

impl MutationCoordinator {
    pub fn pending() -> Self {
        Self(watch::channel(Connection::Pending).0)
    }

    pub(crate) fn connect(
        &self,
        effects: Arc<dyn CommitNotifications>,
        runtime: Arc<dyn ports::RuntimeGate>,
    ) {
        assert!(matches!(*self.0.borrow(), Connection::Pending));
        self.0.send_replace(Connection::Ready { effects, runtime });
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn isolated() -> Self {
        Self(watch::channel(Connection::Isolated).0)
    }

    /// The effects owner a source hands its own slice to once it committed.
    /// Every write passes [`MutationCoordinator::ensure_ready`] first, so the
    /// connection is complete by then.
    pub fn effects(&self) -> Arc<dyn CommitNotifications> {
        match &*self.0.borrow() {
            Connection::Ready { effects, .. } => effects.clone(),
            #[cfg(any(test, feature = "test-support"))]
            Connection::Isolated => {
                Arc::new(crate::client::effects::ports::NoopCommitNotifications)
            }
            Connection::Pending => unreachable!("a source commits only once connected"),
        }
    }

    pub(crate) fn runtime(&self) -> Arc<dyn ports::RuntimeGate> {
        match &*self.0.borrow() {
            Connection::Ready { runtime, .. } => runtime.clone(),
            #[cfg(any(test, feature = "test-support"))]
            Connection::Isolated => Arc::new(IsolatedRuntimeGate),
            Connection::Pending => unreachable!("a source checks only once connected"),
        }
    }

    pub fn ensure_ready(&self) -> Result<(), NotReady> {
        ensure!(
            !matches!(*self.0.borrow(), Connection::Pending),
            NotReadySnafu
        );
        Ok(())
    }
}

pub(crate) fn classify_commit_aborted(error: ReplaceIfVersionError) -> CommitAborted {
    let runtime = RuntimeAftermath::Untouched;
    match &error {
        ReplaceIfVersionError::WriteConfig(_) | ReplaceIfVersionError::LocalWrite(_) => {
            WriteConfigSnafu { runtime }
                .into_error(Box::new(error) as nyanpasu_contracts::BoxedCause)
        }
        ReplaceIfVersionError::ResourceRecovery { .. } => RecoverAfterWriteFailureSnafu { runtime }
            .into_error(Box::new(error) as nyanpasu_contracts::BoxedCause),
        ReplaceIfVersionError::State(_) => {
            ValidateStateSnafu.into_error(Box::new(error) as nyanpasu_contracts::BoxedCause)
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub(crate) struct IsolatedRuntimeGate;

#[cfg(any(test, feature = "test-support"))]
#[async_trait::async_trait]
impl ports::RuntimeGate for IsolatedRuntimeGate {
    async fn check(
        &self,
        _: ports::RuntimeCandidate,
    ) -> Result<(), Arc<nyanpasu_runtime_values::runtime_error::RuntimeError>> {
        Ok(())
    }
    fn committed(
        &self,
        _: nyanpasu_runtime_values::mutation::ConfigDomain,
        _: nyanpasu_state_engine::Version,
    ) {
    }
}
