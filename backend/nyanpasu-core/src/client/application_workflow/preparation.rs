use std::sync::Arc;

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use nyanpasu_core_manager::{CoreSpec, LocalIpcPolicy, LocalIpcSettings};
use nyanpasu_state_engine::StateSnapshot;
use snafu::ResultExt;

use super::{
    super::{SessionPortResolver, runtime},
    ports::RuntimeBuildPort,
};
use crate::{
    actor_v2::intent::RuntimeIntentBuilder,
    client::core_lifecycle::ports::{PreparedRuntime, RuntimePreparationPort},
};
use nyanpasu_runtime_values::{
    core_spec::CoreSpecError,
    publish::PublishRuntimeError,
    runtime_error::{BuildRuntimeSnafu, ResolvePortSnafu, RuntimeError},
};

/// Builds runtime candidates from committed source config. It holds read-only
/// state handles, never a domain client: build capture must not write the source
/// domains it reads.
pub(super) struct RuntimePreparation {
    application: StateSnapshot<NyanpasuAppConfig>,
    clash: StateSnapshot<ClashConfig>,
    profiles: StateSnapshot<Profiles>,
    content_lock: crate::state::mutation::ProfileContentLock,
    builder: Arc<dyn RuntimeBuildPort>,
    ports: Arc<SessionPortResolver>,
    revisions: runtime::RuntimeRevisionAllocator,
}

impl RuntimePreparation {
    pub fn new(
        application: StateSnapshot<NyanpasuAppConfig>,
        clash: StateSnapshot<ClashConfig>,
        profiles: StateSnapshot<Profiles>,
        content_lock: crate::state::mutation::ProfileContentLock,
        builder: Arc<dyn RuntimeBuildPort>,
        ports: Arc<SessionPortResolver>,
    ) -> Self {
        Self {
            application,
            clash,
            profiles,
            content_lock,
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
        self.prepare_inputs(inputs).await
    }

    pub async fn capture_inputs(
        &self,
        app: NyanpasuAppConfig,
        clash: ClashConfig,
        profiles: Arc<Profiles>,
    ) -> super::inputs::RuntimeInputs {
        let _guard = self.content_lock.read().await;
        let content = self.builder.capture_content(&profiles).await;
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
        let _guard = self.content_lock.read().await;
        let profiles = self.profiles.load();
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
    }

    pub async fn prepare_inputs(
        &self,
        inputs: super::inputs::RuntimeInputs,
    ) -> Result<PreparedRuntime, RuntimeError> {
        self.prepare_inputs_with_policy(inputs, false).await
    }

    pub async fn prepare_candidate_inputs(
        &self,
        inputs: super::inputs::RuntimeInputs,
    ) -> Result<PreparedRuntime, RuntimeError> {
        self.prepare_inputs_with_policy(inputs, true).await
    }

    async fn prepare_inputs_with_policy(
        &self,
        inputs: super::inputs::RuntimeInputs,
        strict_transforms: bool,
    ) -> Result<PreparedRuntime, RuntimeError> {
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
            .context(ResolvePortSnafu)?;
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
            .context(BuildRuntimeSnafu)?;
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

    #[cfg(test)]
    pub async fn prepare_committed(
        &self,
        profiles: Arc<Profiles>,
        clash: ClashConfig,
    ) -> Result<PreparedRuntime, RuntimeError> {
        let app = self.application.load().state.clone();
        self.prepare(profiles, clash, app).await
    }
}

#[async_trait::async_trait]
impl RuntimePreparationPort for RuntimePreparation {
    async fn prepare_latest(&self) -> Result<PreparedRuntime, RuntimeError> {
        let (_, inputs) = self
            .capture_committed_inputs(
                self.application.load().state.clone(),
                self.clash.load().state.clone(),
            )
            .await;
        self.prepare_inputs(inputs).await
    }

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
