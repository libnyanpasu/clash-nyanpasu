use std::sync::Arc;

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use nyanpasu_core_manager::{CoreSpec, LocalIpcPolicy, LocalIpcSettings};
use snafu::ResultExt;

use super::{
    super::{SessionPortResolver, runtime},
    ports::RuntimeBuildPort,
};
use crate::{
    actor_v2::intent::RuntimeIntentBuilder,
    client::{
        candidate_inspection::CandidateEvidence,
        core_lifecycle::ports::{PreparedRuntime, RuntimePreparationPort},
    },
};
use nyanpasu_runtime_values::{
    core_spec::CoreSpecError,
    publish::PublishRuntimeError,
    runtime_error::{ResolvePortSnafu, RuntimeError},
};

/// A preparation that failed, with the candidate its build had produced.
#[derive(Debug)]
pub(in crate::client) struct PrepareFailure {
    pub error: RuntimeError,
    pub candidate: Option<Box<CandidateEvidence>>,
}

impl From<RuntimeError> for PrepareFailure {
    fn from(error: RuntimeError) -> Self {
        Self {
            error,
            candidate: None,
        }
    }
}

/// Builds runtime candidates from committed source config. It holds read-only
/// state handles, never a domain client: build capture must not write the source
/// domains it reads.
pub(super) struct RuntimePreparation {
    profile_content: crate::state::mutation::ProfileContentReader,
    builder: Arc<dyn RuntimeBuildPort>,
    ports: Arc<SessionPortResolver>,
    revisions: runtime::RuntimeRevisionAllocator,
}

impl RuntimePreparation {
    pub fn new(
        profile_content: crate::state::mutation::ProfileContentReader,
        builder: Arc<dyn RuntimeBuildPort>,
        ports: Arc<SessionPortResolver>,
    ) -> Self {
        Self {
            profile_content,
            builder,
            ports,
            revisions: runtime::RuntimeRevisionAllocator::new(),
        }
    }

    pub(super) fn with_revisions(mut self, revisions: runtime::RuntimeRevisionAllocator) -> Self {
        self.revisions = revisions;
        self
    }

    #[cfg(test)]
    pub async fn prepare(
        &self,
        profiles: Arc<Profiles>,
        clash: ClashConfig,
        app: NyanpasuAppConfig,
    ) -> Result<PreparedRuntime, RuntimeError> {
        let inputs = self.capture_inputs(app, clash, profiles).await;
        self.prepare_inputs(inputs)
            .await
            .map_err(|failure| failure.error)
    }

    pub async fn capture_inputs(
        &self,
        app: NyanpasuAppConfig,
        clash: ClashConfig,
        profiles: Arc<Profiles>,
    ) -> super::inputs::RuntimeInputs {
        let content = self
            .profile_content
            .candidate(self.builder.capture_content(&profiles))
            .await;
        super::inputs::RuntimeInputs {
            app,
            clash,
            profiles,
            content,
        }
    }

    pub async fn capture_committed_inputs(
        &self,
        app: NyanpasuAppConfig,
        clash: ClashConfig,
    ) -> (nyanpasu_state_engine::Version, super::inputs::RuntimeInputs) {
        self.profile_content
            .committed(|profiles| async move {
                let content = self.builder.capture_content(&profiles.state).await;
                (
                    profiles.version,
                    super::inputs::RuntimeInputs {
                        app,
                        clash,
                        profiles: Arc::new(profiles.state.clone()),
                        content,
                    },
                )
            })
            .await
    }

    /// Prepares a committed target, keeping failed transforms lenient. A
    /// failure keeps the candidate its build had produced.
    pub(in crate::client) async fn prepare_inputs(
        &self,
        inputs: super::inputs::RuntimeInputs,
    ) -> Result<PreparedRuntime, PrepareFailure> {
        self.prepare_inputs_with_policy(inputs, false).await
    }

    /// Prepares a candidate for the check, refusing failed transforms. A
    /// failure keeps the candidate its build had produced.
    pub async fn prepare_candidate_inputs(
        &self,
        inputs: super::inputs::RuntimeInputs,
    ) -> Result<PreparedRuntime, PrepareFailure> {
        self.prepare_inputs_with_policy(inputs, true).await
    }

    async fn prepare_inputs_with_policy(
        &self,
        inputs: super::inputs::RuntimeInputs,
        strict_transforms: bool,
    ) -> Result<PreparedRuntime, PrepareFailure> {
        let revision = self.revisions.allocate();
        let local_ipc = LocalIpcSettings {
            policy: match inputs.clash.clash_control_channel {
                nyanpasu_config::clash::config::ClashControlChannel::PreferIpc => {
                    LocalIpcPolicy::Prefer
                }
                nyanpasu_config::clash::config::ClashControlChannel::HttpOnly => {
                    LocalIpcPolicy::Disable
                }
            },
            keep_http_controller: !inputs.clash.clash_ipc_disable_http_controller,
        };
        // A candidate resolution, not an active one: nothing here touches the
        // confirmed binding, so a build that is never applied leaves the
        // running instance's ports alone (v2 §6.2).
        let ports = self
            .ports
            .resolve_candidate(&inputs.clash)
            .context(ResolvePortSnafu)
            .map_err(PrepareFailure::from)?;
        let core_type: nyanpasu_utils::core::CoreType = (&inputs.app.core).into();
        let target = inputs.target_key();
        let snapshot = self
            .builder
            .build(
                revision,
                inputs,
                ports.bindings().clone(),
                strict_transforms,
            )
            .await
            .map_err(|failure| PrepareFailure {
                error: RuntimeError::BuildRuntime {
                    source: failure.error,
                    candidate_snapshot_id: None,
                },
                candidate: failure.candidate,
            })?;
        // Serialized once, here: the check and the reconcile both consume this
        // value, so "same bytes" holds by construction rather than by
        // convention.
        let intent =
            RuntimeIntentBuilder::build(core_type, snapshot.config_text.clone(), local_ipc);
        Ok(PreparedRuntime {
            snapshot,
            intent: Arc::new(intent),
            ports,
            target,
        })
    }
}

#[async_trait::async_trait]
impl RuntimePreparationPort for RuntimePreparation {
    async fn publish(
        &self,
        snapshot: &runtime::RuntimeSnapshot,
    ) -> Result<(), PublishRuntimeError> {
        self.builder.publish(snapshot).await
    }

    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> Result<CoreSpec, CoreSpecError> {
        self.builder.core_spec(core)
    }
}
