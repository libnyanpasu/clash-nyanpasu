use super::super::runtime;
use nyanpasu_runtime_values::check::RuntimeCheckUnavailable;

use async_trait::async_trait;
use nyanpasu_config::runtime::executor::{PortError, ScriptRunner};
use nyanpasu_profile_transform::RuntimeBuildError;
use nyanpasu_runtime_values::{core_spec::CoreSpecError, publish::PublishRuntimeError};
use std::sync::Arc;

/// Builds a script runner on the blocking thread that will use and drop it.
pub type ScriptRunnerFactory = dyn Fn() -> Result<Box<dyn ScriptRunner>, PortError> + Send + Sync;

#[async_trait]
pub trait RuntimeBuildPort: Send + Sync + 'static {
    async fn capture_content(
        &self,
        profiles: &nyanpasu_config::profile::Profiles,
    ) -> super::inputs::FrozenProfileContent;
    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> Result<nyanpasu_core_manager::CoreSpec, CoreSpecError>;
    /// Builds a candidate runtime from explicit inputs. The resolved ports are
    /// one of them: resolving inside the build would hide a probe that races
    /// the running core and would tie the build to a shared port cache.
    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        inputs: super::inputs::RuntimeInputs,
        ports: nyanpasu_config::runtime::executor::ResolvedPortBindings,
        strict_transforms: bool,
    ) -> Result<Arc<runtime::RuntimeSnapshot>, RuntimeBuildError>;
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot)
    -> Result<(), PublishRuntimeError>;
}

/// One advisory check of a candidate runtime. The document is the built
/// `RuntimeIntent`, never a re-serialization: the check and the reconcile have
/// to consume the identical to-be-committed bytes.
pub(in crate::client) struct RuntimeCheckRequest<'a> {
    pub core_spec: nyanpasu_core_manager::CoreSpec,
    pub intent: &'a crate::actor_v2::intent::RuntimeIntent,
}

/// What the core said about a candidate document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::client) enum RuntimeCheckOutcome {
    /// The core's own validation accepted the document.
    Passed,
    /// The core rejected it. Deterministic: the same bytes fail again.
    Rejected {
        kind: Option<nyanpasu_core_manager::CoreErrorKind>,
        message: String,
    },
    Unavailable(RuntimeCheckUnavailable),
}

/// The check capability, as the application consumes it.
#[async_trait]
pub(in crate::client) trait RuntimeValidatorPort: Send + Sync + 'static {
    async fn check(&self, request: RuntimeCheckRequest<'_>) -> RuntimeCheckOutcome;
}
