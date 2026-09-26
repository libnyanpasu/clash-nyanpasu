use super::super::runtime;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub(in crate::client) trait RuntimeBuildPort: Send + Sync + 'static {
    async fn capture_content(
        &self,
        profiles: &nyanpasu_config::profile::Profiles,
    ) -> anyhow::Result<super::inputs::FrozenProfileContent>;
    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> anyhow::Result<nyanpasu_core_manager::CoreSpec>;
    /// Builds a candidate runtime from explicit inputs. The resolved ports are
    /// one of them: resolving inside the build would hide a probe that races
    /// the running core and would tie the build to a shared port cache.
    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        inputs: super::inputs::RuntimeInputs,
        ports: nyanpasu_config::runtime::executor::ResolvedPortBindings,
        strict_transforms: bool,
    ) -> anyhow::Result<Arc<runtime::RuntimeSnapshot>>;
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot) -> anyhow::Result<()>;
}

/// One advisory check of a candidate runtime. The document is the built
/// `RuntimeIntent`, never a re-serialization: the check and the reconcile have
/// to consume the identical to-be-committed bytes.
pub(in crate::client) struct RuntimeCheckRequest<'a> {
    pub core_spec: nyanpasu_core_manager::CoreSpec,
    pub intent: &'a crate::core::actor_v2::intent::RuntimeIntent,
}

/// Why no check ran. Every variant is a reason, never a verdict: an absent
/// check must not be reported as a passing one (v2 §2.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::client) enum RuntimeCheckUnavailable {
    /// No host owns the runtime, so there is nothing to check against.
    NoEndpoint { reason: String },
    /// The host owning the runtime exposes no check for this request.
    HostUnsupported {
        host: crate::core::actor_v2::endpoint::ExecutionHost,
        reason: String,
    },
    /// The host reads the candidate from disk and it could not be staged.
    CandidateUnavailable { reason: String },
    /// The host has the capability but could not serve it.
    Backend {
        kind: Option<nyanpasu_core_manager::CoreErrorKind>,
        message: String,
        retryable: bool,
    },
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
