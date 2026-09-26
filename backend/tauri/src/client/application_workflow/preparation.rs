use std::sync::Arc;

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use nyanpasu_core::state::StateSnapshot;
use nyanpasu_core_manager::{CoreError, CoreSpec, LocalIpcPolicy, LocalIpcSettings};

use super::{
    super::{SessionPortResolver, runtime},
    ports::RuntimeBuildPort,
};
use crate::{
    client::core_lifecycle::{
        domain_error,
        ports::{PreparedRuntime, RuntimePreparationPort},
    },
    core::actor_v2::intent::RuntimeIntentBuilder,
};

/// Builds runtime candidates from committed source config. It holds read-only
/// state handles, never a domain client: the workflow is a state participant
/// and must not be able to write a source domain it is applying for.
pub(super) struct RuntimePreparation {
    application: StateSnapshot<NyanpasuAppConfig>,
    clash: StateSnapshot<ClashConfig>,
    profiles: StateSnapshot<Profiles>,
    builder: Arc<dyn RuntimeBuildPort>,
    ports: Arc<SessionPortResolver>,
    revisions: runtime::RuntimeRevisionAllocator,
}

impl RuntimePreparation {
    pub fn new(
        application: StateSnapshot<NyanpasuAppConfig>,
        clash: StateSnapshot<ClashConfig>,
        profiles: StateSnapshot<Profiles>,
        builder: Arc<dyn RuntimeBuildPort>,
        ports: Arc<SessionPortResolver>,
    ) -> Self {
        Self {
            application,
            clash,
            profiles,
            builder,
            ports,
            revisions: runtime::RuntimeRevisionAllocator::new(),
        }
    }

    pub async fn prepare(
        &mut self,
        profiles: Arc<Profiles>,
        clash: ClashConfig,
        app: NyanpasuAppConfig,
    ) -> Result<PreparedRuntime, CoreError> {
        let content = self
            .builder
            .capture_content(&profiles)
            .await
            .map_err(domain_error)?;
        self.prepare_inputs(super::inputs::RuntimeInputs {
            app,
            clash,
            profiles,
            content,
        })
        .await
    }

    pub async fn capture_inputs(
        &self,
        app: NyanpasuAppConfig,
        clash: ClashConfig,
        profiles: Arc<Profiles>,
    ) -> anyhow::Result<super::inputs::RuntimeInputs> {
        let content = self.builder.capture_content(&profiles).await?;
        Ok(super::inputs::RuntimeInputs {
            app,
            clash,
            profiles,
            content,
        })
    }

    pub async fn prepare_inputs(
        &mut self,
        inputs: super::inputs::RuntimeInputs,
    ) -> Result<PreparedRuntime, CoreError> {
        self.prepare_inputs_with_policy(inputs, false).await
    }

    pub async fn prepare_candidate_inputs(
        &mut self,
        inputs: super::inputs::RuntimeInputs,
    ) -> Result<PreparedRuntime, CoreError> {
        self.prepare_inputs_with_policy(inputs, true).await
    }

    async fn prepare_inputs_with_policy(
        &mut self,
        inputs: super::inputs::RuntimeInputs,
        strict_transforms: bool,
    ) -> Result<PreparedRuntime, CoreError> {
        let revision = self.revisions.allocate().map_err(domain_error)?;
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
            .map_err(domain_error)?;
        let core_type: nyanpasu_utils::core::CoreType = (&inputs.app.core).into();
        let snapshot = self
            .builder
            .build(
                revision,
                inputs,
                ports.bindings().clone(),
                strict_transforms,
            )
            .await
            .map_err(domain_error)?;
        // Serialized once, here: the check and the reconcile both consume this
        // value, so "same bytes" holds by construction rather than by
        // convention.
        let intent = RuntimeIntentBuilder::build(core_type, &snapshot.config, local_ipc).map_err(
            |error| {
                CoreError::new(
                    nyanpasu_core_manager::CoreErrorKind::InvalidConfig,
                    format!("failed to serialize runtime config: {error}"),
                    false,
                )
            },
        )?;
        Ok(PreparedRuntime {
            snapshot,
            intent: Arc::new(intent),
            ports,
        })
    }

    pub async fn prepare_committed(
        &mut self,
        profiles: Arc<Profiles>,
        clash: ClashConfig,
    ) -> Result<PreparedRuntime, CoreError> {
        let app = self.application.load().state.clone();
        self.prepare(profiles, clash, app).await
    }
}

#[async_trait::async_trait]
impl RuntimePreparationPort for RuntimePreparation {
    async fn prepare_latest(&mut self) -> Result<PreparedRuntime, CoreError> {
        // Independent committed snapshots; changes during a build retain a dirty pass.
        let profiles = Arc::new(self.profiles.load().state.clone());
        let clash = self.clash.load().state.clone();
        self.prepare_committed(profiles, clash).await
    }

    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot) -> anyhow::Result<()> {
        self.builder.publish(snapshot).await
    }

    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> anyhow::Result<CoreSpec> {
        self.builder.core_spec(core)
    }
}
