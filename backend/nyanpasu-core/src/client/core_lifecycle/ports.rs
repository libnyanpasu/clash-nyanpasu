use std::sync::Arc;

use async_trait::async_trait;

use super::super::runtime;
use crate::{
    client::{runtime::PublishRuntimeError, runtime_error::RuntimeError},
    control::local_host::CoreSpecError,
};

/// Application-owned runtime preparation; implementations never call the workflow actor.
#[async_trait]
pub(in crate::client) trait RuntimePreparationPort: Send + Sync {
    async fn prepare_latest(&mut self) -> Result<PreparedRuntime, RuntimeError>;
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot)
    -> Result<(), PublishRuntimeError>;
    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> Result<nyanpasu_core_manager::CoreSpec, CoreSpecError>;
}

pub(in crate::client) struct PreparedRuntime {
    pub snapshot: Arc<runtime::RuntimeSnapshot>,
    /// The to-be-committed bytes, serialized exactly once so the advisory
    /// check and the reconcile cannot drift apart.
    pub intent: Arc<crate::control::intent::RuntimeIntent>,
    /// The ports this candidate would bind. Inert: only the receipt of an
    /// apply that succeeded may confirm them.
    pub ports: crate::client::ports::CandidatePortBindings,
    /// The committed target the build came from (`RuntimeInputs::target_key`),
    /// when it has one.
    pub target: Option<String>,
}
