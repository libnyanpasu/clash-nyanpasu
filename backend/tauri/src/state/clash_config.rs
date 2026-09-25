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
use nyanpasu_config::clash::config::{
    ClashConfig, ClashConfigPatch, overrides::ClashGuardOverridesPatch,
};
use nyanpasu_core::state::{
    PersistentStateManager, ReplaceIfVersionResult, Version, VersionedState,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

use super::{
    ConditionalReplaceResult,
    mirror::{ClashLegacyBridge, PreparedTypedReplace},
};

/// Snapshot of the saved Clash configuration domain, not live Clash runtime API state.
#[derive(Debug, Clone)]
pub struct ClashConfigSnapshot {
    pub state: ClashConfig,
    pub version: u64,
    pub(crate) receipt: Option<crate::client::runtime::CommitReceipt>,
    pub(crate) degradations: Vec<crate::client::runtime::Degradation>,
}

impl ClashConfigSnapshot {
    pub(crate) fn outcome(self) -> crate::client::runtime::MutationOutcome<()> {
        let outcome = crate::client::runtime::MutationOutcome::from_parts((), self.degradations);
        match self.receipt {
            Some(receipt) => outcome.with_commit(receipt),
            None => outcome,
        }
    }

    pub(crate) fn from_versioned(versioned: &VersionedState<ClashConfig>) -> Self {
        Self {
            receipt: None,
            degradations: Vec::new(),
            state: versioned.state.clone(),
            version: *versioned.version.as_ref(),
        }
    }
}

pub struct ClashConfigActorArgs {
    pub(crate) mutations: MutationCoordinator,
    pub manager: PersistentStateManager<ClashConfig>,
    pub bridge: Arc<dyn ClashLegacyBridge>,
}

pub struct ClashConfigActorState {
    mutations: MutationCoordinator,
    manager: PersistentStateManager<ClashConfig>,
    bridge: Arc<dyn ClashLegacyBridge>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ClashConfigActorMessage {
    Patch {
        patch: ClashConfigPatch,
        reply: RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>,
    },
    PatchOverrides {
        patch: ClashGuardOverridesPatch,
        reply: RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>,
    },
    Replace {
        state: ClashConfig,
        reply: RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>,
    },
    PrepareReplace {
        state: ClashConfig,
        reply: RpcReplyPort<anyhow::Result<PreparedTypedReplace<ClashConfig>>>,
    },
    ReplacePreparedIfVersion {
        expected_version: u64,
        prepared: PreparedTypedReplace<ClashConfig>,
        reply: RpcReplyPort<anyhow::Result<ConditionalReplaceResult<ClashConfigSnapshot>>>,
    },
}

/// Actor-owned persistent Clash configuration. Runtime Clash API state stays in the core/API path.
pub struct ClashConfigActor;

impl ClashConfigActor {
    async fn patch(
        state: &mut ClashConfigActorState,
        patch: ClashConfigPatch,
    ) -> anyhow::Result<ClashConfigSnapshot> {
        let mut next = state.manager.snapshot_handle().load().state.clone();
        let hints = MutationHints {
            requested: RequestedRuntimeFields::of_clash(&patch),
            ..Default::default()
        };
        let class = CommandClass::Save;
        next.apply(patch);
        Self::commit(state, next, hints, class).await
    }

    fn snapshot(state: &ClashConfigActorState) -> ClashConfigSnapshot {
        ClashConfigSnapshot::from_versioned(&state.manager.snapshot_handle().load())
    }

    fn prepare_replace(
        state: &ClashConfigActorState,
        next: ClashConfig,
    ) -> anyhow::Result<PreparedTypedReplace<ClashConfig>> {
        let mirror = state
            .bridge
            .prepare(&next)
            .context("failed to prepare legacy clash mirror")?;
        Ok(PreparedTypedReplace::new(next, mirror))
    }

    async fn commit(
        state: &mut ClashConfigActorState,
        next: ClashConfig,
        hints: MutationHints,
        class: CommandClass,
    ) -> anyhow::Result<ClashConfigSnapshot> {
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
            .context("failed to persist clash config")?;
        mirror.apply();
        let mut snapshot = Self::snapshot(state);
        let (receipt, degradations) = state
            .mutations
            .finish(operation, "clash", snapshot.version)
            .await;
        snapshot.receipt = Some(receipt);
        snapshot.degradations = degradations;
        Ok(snapshot)
    }

    async fn replace_prepared_if_version(
        state: &mut ClashConfigActorState,
        expected_version: u64,
        prepared: PreparedTypedReplace<ClashConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ClashConfigSnapshot>> {
        let (next, mirror) = prepared.into_parts();
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
            .context("failed to conditionally persist clash config")?
        {
            ReplaceIfVersionResult::Replaced => {
                mirror.apply();
                let mut snapshot = Self::snapshot(state);
                let (receipt, degradations) = state
                    .mutations
                    .finish(operation, "clash", snapshot.version)
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

impl Actor for ClashConfigActor {
    type Msg = ClashConfigActorMessage;
    type State = ClashConfigActorState;
    type Arguments = ClashConfigActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ClashConfigActorState {
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
            ClashConfigActorMessage::Patch { patch, reply } => {
                let _ = reply.send(Self::patch(state, patch).await);
            }
            ClashConfigActorMessage::PatchOverrides { patch, reply } => {
                let mut next = state.manager.snapshot_handle().load().state.clone();
                let hints = MutationHints {
                    mode_requested: patch.mode.is_some(),
                    requested: RequestedRuntimeFields::of_clash_overrides(&patch),
                    ..Default::default()
                };
                next.overrides.apply(patch);
                let _ = reply.send(Self::commit(state, next, hints, CommandClass::Save).await);
            }
            ClashConfigActorMessage::Replace { state: next, reply } => {
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
            ClashConfigActorMessage::PrepareReplace { state: next, reply } => {
                let _ = reply.send(Self::prepare_replace(state, next));
            }
            ClashConfigActorMessage::ReplacePreparedIfVersion {
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

    /// Test-only double that fails every Clash legacy mirror preparation.
    struct FailingClashMirror;

    impl ClashLegacyBridge for FailingClashMirror {
        fn prepare(&self, _snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            anyhow::bail!("injected clash mirror prepare failure");
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
        }
    }

    async fn spawn_actor(
        bridge: Arc<dyn ClashLegacyBridge>,
    ) -> (
        ActorRef<ClashConfigActorMessage>,
        StateSnapshot<ClashConfig>,
        tempfile::TempDir,
    ) {
        let dir = tempdir().expect("tempdir should be created");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml"))
            .expect("temp path should be UTF-8");
        let manager = PersistentStateManagerSetup::<ClashConfig>::builder()
            .config_path(path)
            .assemble()
            .from_state(ClashConfig::default())
            .await
            .expect("clash config manager should initialize");
        let snapshot = manager.snapshot_handle();
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ClashConfigActor,
            ClashConfigActorArgs {
                manager,
                bridge,
                mutations: MutationCoordinator::isolated(),
            },
        )
        .await
        .expect("clash config actor should spawn");
        (actor_ref, snapshot, dir)
    }

    #[tokio::test]
    async fn mirror_prepare_failure_returns_error_without_commit() {
        let (actor, snapshot, _dir) = spawn_actor(Arc::new(FailingClashMirror)).await;

        let before = ClashConfigSnapshot::from_versioned(&snapshot.load());
        assert!(!before.state.enable_tun_mode);
        let before_version = before.version;

        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let err = match actor
            .call(
                |reply| ClashConfigActorMessage::Patch { patch, reply },
                None,
            )
            .await
            .expect("actor call should complete")
        {
            CallResult::Success(result) => result
                .expect_err("mirror failure after upsert must surface as Err under current defect"),
            CallResult::SenderError => panic!("clash config actor reply dropped"),
            CallResult::Timeout => panic!("clash config actor call timed out"),
        };
        assert!(
            err.to_string().contains("legacy clash mirror")
                || err.to_string().contains("injected clash mirror failure"),
            "unexpected error: {err:#}"
        );

        let after = ClashConfigSnapshot::from_versioned(&snapshot.load());
        assert_eq!(after.state.enable_tun_mode, before.state.enable_tun_mode);
        assert_eq!(after.version, before_version);
    }

    #[tokio::test]
    async fn mirror_prepare_failure_leaves_state_and_version_unchanged() {
        let (actor, snapshot, _dir) = spawn_actor(Arc::new(FailingClashMirror)).await;

        let before = ClashConfigSnapshot::from_versioned(&snapshot.load());

        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let _ = actor
            .call(
                |reply| ClashConfigActorMessage::Patch { patch, reply },
                None,
            )
            .await;

        let after = ClashConfigSnapshot::from_versioned(&snapshot.load());
        assert_eq!(after.version, before.version);
        assert_eq!(after.state.enable_tun_mode, before.state.enable_tun_mode);
    }
}
