use std::sync::Arc;

use anyhow::Context as _;
use nyanpasu_config::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};
use nyanpasu_core::state::{PersistentStateManager, ReplaceIfVersionResult, Version};
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
}

pub struct ApplicationActorArgs {
    pub manager: PersistentStateManager<NyanpasuAppConfig>,
    pub bridge: Arc<dyn VergeLegacyBridge>,
}

pub struct ApplicationActorState {
    manager: PersistentStateManager<NyanpasuAppConfig>,
    bridge: Arc<dyn VergeLegacyBridge>,
}

#[derive(Debug)]
pub enum ApplicationActorMessage {
    Get(RpcReplyPort<anyhow::Result<ApplicationSnapshot>>),
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
    fn snapshot(state: &ApplicationActorState) -> ApplicationSnapshot {
        let snapshot = state.manager.snapshot_handle().load();
        ApplicationSnapshot {
            state: snapshot.state.clone(),
            version: *snapshot.version.as_ref(),
        }
    }

    fn prepare_replace(
        state: &ApplicationActorState,
        next: NyanpasuAppConfig,
    ) -> anyhow::Result<PreparedTypedReplace<NyanpasuAppConfig>> {
        let mirror = state
            .bridge
            .prepare(&next)
            .context("failed to prepare legacy application mirror")?;
        Ok(PreparedTypedReplace::new(next, mirror))
    }

    async fn commit(
        state: &mut ApplicationActorState,
        next: NyanpasuAppConfig,
    ) -> anyhow::Result<ApplicationSnapshot> {
        let (next, mirror) = Self::prepare_replace(state, next)?.into_parts();
        state
            .manager
            .upsert(next)
            .await
            .context("failed to persist application config")?;
        mirror.apply();
        Ok(Self::snapshot(state))
    }

    async fn replace_prepared_if_version(
        state: &mut ApplicationActorState,
        expected_version: u64,
        prepared: PreparedTypedReplace<NyanpasuAppConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ApplicationSnapshot>> {
        let (next, mirror) = prepared.into_parts();
        match state
            .manager
            .replace_if_version(Version::new(expected_version), next)
            .await
            .context("failed to conditionally persist application config")?
        {
            ReplaceIfVersionResult::Replaced => {
                mirror.apply();
                Ok(ConditionalReplaceResult::Replaced(Self::snapshot(state)))
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
            ApplicationActorMessage::Get(reply) => {
                let _ = reply.send(Ok(Self::snapshot(state)));
            }
            ApplicationActorMessage::Patch { patch, reply } => {
                let result = async {
                    let mut next = state.manager.snapshot_handle().load().state.clone();
                    next.apply(patch);
                    Self::commit(state, next).await
                }
                .await;
                let _ = reply.send(result);
            }
            ApplicationActorMessage::Replace { state: next, reply } => {
                let _ = reply.send(Self::commit(state, next).await);
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
    use nyanpasu_core::state::PersistentStateManagerSetup;
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
    ) -> (ActorRef<ApplicationActorMessage>, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("application.yaml"))
            .expect("temp path should be UTF-8");
        let manager = PersistentStateManagerSetup::<NyanpasuAppConfig>::builder()
            .config_path(path)
            .assemble()
            .from_state(NyanpasuAppConfig::default())
            .await
            .expect("application manager should initialize");
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ApplicationActor,
            ApplicationActorArgs { manager, bridge },
        )
        .await
        .expect("application actor should spawn");
        (actor_ref, dir)
    }

    async fn get_snapshot(
        actor: &ActorRef<ApplicationActorMessage>,
    ) -> anyhow::Result<ApplicationSnapshot> {
        match actor.call(ApplicationActorMessage::Get, None).await? {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("application actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("application actor call timed out"),
        }
    }

    #[tokio::test]
    async fn mirror_prepare_failure_leaves_state_and_version_unchanged() {
        let (actor, _dir) = spawn_actor(Arc::new(FailingVergeMirror)).await;

        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");

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

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert_eq!(after.version, before.version);
        assert_eq!(
            after.state.enable_system_proxy,
            before.state.enable_system_proxy
        );
    }
}
