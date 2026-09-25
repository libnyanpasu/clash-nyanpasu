use crate::{
    client::application_workflow::{
        impact::{MutationHints, RequestedRuntimeFields},
        policy::CommandClass,
    },
    state::mutation::MutationCoordinator,
};
use nyanpasu_core_manager::OperationId;
use std::sync::Arc;

use anyhow::Context as _;
use nyanpasu_config::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};
use nyanpasu_core::state::{
    PersistentStateManager, ReplaceIfVersionResult, Version, VersionedState,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

use super::{
    ConditionalReplaceResult,
    mirror::{PreparedTypedReplace, VergeLegacyBridge},
};

#[derive(Debug, Clone)]
pub struct ApplicationSnapshot {
    pub state: NyanpasuAppConfig,
    pub version: u64,
    pub(crate) receipt: Option<crate::client::runtime::CommitReceipt>,
    pub(crate) degradations: Vec<crate::client::runtime::Degradation>,
}

impl ApplicationSnapshot {
    pub(crate) fn outcome(self) -> crate::client::runtime::MutationOutcome<()> {
        let outcome = crate::client::runtime::MutationOutcome::from_parts((), self.degradations);
        match self.receipt {
            Some(receipt) => outcome.with_commit(receipt),
            None => outcome,
        }
    }

    pub(crate) fn from_versioned(versioned: &VersionedState<NyanpasuAppConfig>) -> Self {
        Self {
            receipt: None,
            degradations: Vec::new(),
            state: versioned.state.clone(),
            version: *versioned.version.as_ref(),
        }
    }
}

pub struct ApplicationActorArgs {
    pub(crate) mutations: MutationCoordinator,
    pub manager: PersistentStateManager<NyanpasuAppConfig>,
    pub bridge: Arc<dyn VergeLegacyBridge>,
}

pub struct ApplicationActorState {
    mutations: MutationCoordinator,
    manager: PersistentStateManager<NyanpasuAppConfig>,
    bridge: Arc<dyn VergeLegacyBridge>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ApplicationActorMessage {
    Patch {
        patch: NyanpasuAppConfigPatch,
        reply: RpcReplyPort<anyhow::Result<ApplicationSnapshot>>,
    },
    Replace {
        state: NyanpasuAppConfig,
        reply: RpcReplyPort<anyhow::Result<ApplicationSnapshot>>,
    },
    PrepareReplace {
        state: NyanpasuAppConfig,
        reply: RpcReplyPort<anyhow::Result<PreparedTypedReplace<NyanpasuAppConfig>>>,
    },
    ReplacePreparedIfVersion {
        expected_version: u64,
        prepared: PreparedTypedReplace<NyanpasuAppConfig>,
        reply: RpcReplyPort<anyhow::Result<ConditionalReplaceResult<ApplicationSnapshot>>>,
    },
}

pub struct ApplicationActor;

impl ApplicationActor {
    async fn patch(
        state: &mut ApplicationActorState,
        patch: NyanpasuAppConfigPatch,
    ) -> anyhow::Result<ApplicationSnapshot> {
        let mut next = state.manager.snapshot_handle().load().state.clone();
        let hints = MutationHints {
            requested: RequestedRuntimeFields::of_application(&patch),
            requested_owners: crate::client::effects::plan::requested_owners(&patch),
            ..Default::default()
        };
        let class = if patch.core.is_some() || patch.enable_service_mode.is_some() {
            CommandClass::ExplicitSwitch
        } else {
            CommandClass::Save
        };
        next.apply(patch);
        Self::commit(state, next, hints, class).await
    }

    fn snapshot(state: &ApplicationActorState) -> ApplicationSnapshot {
        ApplicationSnapshot::from_versioned(&state.manager.snapshot_handle().load())
    }

    fn validate_channel(
        state: &ApplicationActorState,
        next: &mut NyanpasuAppConfig,
    ) -> anyhow::Result<()> {
        use nyanpasu_config::application::ReleaseChannel as Channel;
        let current = state.manager.snapshot_handle().load();
        next.release_channel = next.release_channel.or(current.state.release_channel);
        anyhow::ensure!(
            current.state.release_channel != Some(Channel::Nightly)
                || next.release_channel == Some(Channel::Nightly),
            "cannot leave the nightly release channel"
        );
        Ok(())
    }

    fn prepare_replace(
        state: &ApplicationActorState,
        mut next: NyanpasuAppConfig,
    ) -> anyhow::Result<PreparedTypedReplace<NyanpasuAppConfig>> {
        Self::validate_channel(state, &mut next)?;
        let mirror = state
            .bridge
            .prepare(&next)
            .context("failed to prepare legacy application mirror")?;
        Ok(PreparedTypedReplace::new(next, mirror))
    }

    async fn commit(
        state: &mut ApplicationActorState,
        next: NyanpasuAppConfig,
        hints: MutationHints,
        class: CommandClass,
    ) -> anyhow::Result<ApplicationSnapshot> {
        let (next, mirror) = Self::prepare_replace(state, next)?.into_parts();
        let version = state.manager.snapshot_handle().load().version;
        let operation = OperationId::generate();
        let participant = state.mutations.participant(operation, hints, class)?;
        state
            .manager
            .replace_if_version_with_participant(
                version,
                next,
                participant,
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await
            .context("failed to persist application config")?;
        mirror.apply();
        let mut snapshot = Self::snapshot(state);
        let (receipt, degradations) = state
            .mutations
            .finish(operation, "application", snapshot.version)
            .await;
        snapshot.receipt = Some(receipt);
        snapshot.degradations = degradations;
        Ok(snapshot)
    }

    async fn replace_prepared_if_version(
        state: &mut ApplicationActorState,
        expected_version: u64,
        prepared: PreparedTypedReplace<NyanpasuAppConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ApplicationSnapshot>> {
        let (mut next, mirror) = prepared.into_parts();
        Self::validate_channel(state, &mut next)?;
        let operation = OperationId::generate();
        let participant = state.mutations.participant(
            operation,
            MutationHints {
                requested: RequestedRuntimeFields::whole_document(),
                ..Default::default()
            },
            CommandClass::Save,
        )?;
        match state
            .manager
            .replace_if_version_with_participant(
                Version::new(expected_version),
                next,
                participant,
                || async { Ok(()) },
                || async { Ok(()) },
            )
            .await
            .context("failed to conditionally persist application config")?
        {
            ReplaceIfVersionResult::Replaced => {
                mirror.apply();
                let mut snapshot = Self::snapshot(state);
                let (receipt, degradations) = state
                    .mutations
                    .finish(operation, "application", snapshot.version)
                    .await;
                snapshot.receipt = Some(receipt);
                snapshot.degradations = degradations;
                Ok(ConditionalReplaceResult::Replaced(snapshot))
            }
            ReplaceIfVersionResult::Conflict { actual_version } => {
                Ok(ConditionalReplaceResult::Conflict {
                    actual_version: *actual_version.as_ref(),
                })
            }
        }
    }
}

impl Actor for ApplicationActor {
    type Msg = ApplicationActorMessage;
    type State = ApplicationActorState;
    type Arguments = ApplicationActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ApplicationActorState {
            mutations: args.mutations,
            manager: args.manager,
            bridge: args.bridge,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ApplicationActorMessage::Patch { patch, reply } => {
                let _ = reply.send(Self::patch(state, patch).await);
            }
            ApplicationActorMessage::Replace { state: next, reply } => {
                let _ = reply.send(
                    Self::commit(
                        state,
                        next,
                        MutationHints {
                            requested: RequestedRuntimeFields::whole_document(),
                            ..Default::default()
                        },
                        CommandClass::Save,
                    )
                    .await,
                );
            }
            ApplicationActorMessage::PrepareReplace { state: next, reply } => {
                let _ = reply.send(Self::prepare_replace(state, next));
            }
            ApplicationActorMessage::ReplacePreparedIfVersion {
                expected_version,
                prepared,
                reply,
            } => {
                let _ = reply.send(
                    Self::replace_prepared_if_version(state, expected_version, prepared).await,
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::PreparedLegacyMirror;
    use nyanpasu_core::state::{PersistentStateManagerSetup, StateSnapshot};
    use ractor::rpc::CallResult;
    use struct_patch::Patch;
    use tempfile::tempdir;

    /// Test-only double that fails every mirror preparation.
    struct FailingVergeMirror;

    impl VergeLegacyBridge for FailingVergeMirror {
        fn prepare(
            &self,
            _snap: &NyanpasuAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            anyhow::bail!("injected application mirror prepare failure");
        }

        fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
            Ok(NyanpasuAppConfig::default())
        }
    }

    async fn spawn_actor(
        bridge: Arc<dyn VergeLegacyBridge>,
    ) -> (
        ActorRef<ApplicationActorMessage>,
        StateSnapshot<NyanpasuAppConfig>,
        tempfile::TempDir,
    ) {
        let dir = tempdir().expect("tempdir should be created");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("application.yaml"))
            .expect("temp path should be UTF-8");
        let manager = PersistentStateManagerSetup::<NyanpasuAppConfig>::builder()
            .config_path(path)
            .assemble()
            .from_state(NyanpasuAppConfig::default())
            .await
            .expect("application manager should initialize");
        let snapshot = manager.snapshot_handle();
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ApplicationActor,
            ApplicationActorArgs {
                manager,
                bridge,
                mutations: MutationCoordinator::isolated(),
            },
        )
        .await
        .expect("application actor should spawn");
        (actor_ref, snapshot, dir)
    }

    #[tokio::test]
    async fn mirror_prepare_failure_leaves_state_and_version_unchanged() {
        let (actor, snapshot, _dir) = spawn_actor(Arc::new(FailingVergeMirror)).await;

        let before = ApplicationSnapshot::from_versioned(&snapshot.load());

        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_system_proxy = Some(true);
        let result = actor
            .call(
                |reply| ApplicationActorMessage::Patch { patch, reply },
                None,
            )
            .await
            .expect("actor call should complete");
        match result {
            CallResult::Success(result) => {
                let error = result.expect_err("mirror prepare must reject the mutation");
                assert!(error.to_string().contains("application mirror"));
            }
            CallResult::SenderError => panic!("application actor reply dropped"),
            CallResult::Timeout => panic!("application actor call timed out"),
        }

        let after = ApplicationSnapshot::from_versioned(&snapshot.load());
        assert_eq!(after.version, before.version);
        assert_eq!(
            after.state.enable_system_proxy,
            before.state.enable_system_proxy
        );
    }
}
