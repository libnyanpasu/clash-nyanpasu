use std::sync::Arc;

use anyhow::Context as _;
use nyanpasu_config::clash::config::{ClashConfig, ClashConfigPatch};
use nyanpasu_core::state::PersistentStateManager;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

use super::mirror::ClashLegacyBridge;

/// Snapshot of the saved Clash configuration domain, not live Clash runtime API state.
#[derive(Debug, Clone)]
pub struct ClashConfigSnapshot {
    pub state: ClashConfig,
    pub version: u64,
}

pub struct ClashConfigActorArgs {
    pub manager: PersistentStateManager<ClashConfig>,
    pub bridge: Arc<dyn ClashLegacyBridge>,
}

pub struct ClashConfigActorState {
    manager: PersistentStateManager<ClashConfig>,
    bridge: Arc<dyn ClashLegacyBridge>,
}

#[derive(Debug)]
pub enum ClashConfigActorMessage {
    Get(RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>),
    Patch {
        patch: ClashConfigPatch,
        reply: RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>,
    },
    Replace {
        state: ClashConfig,
        reply: RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>,
    },
}

/// Actor-owned persistent Clash configuration. Runtime Clash API state stays in the core/API path.
pub struct ClashConfigActor;

impl ClashConfigActor {
    fn snapshot(state: &ClashConfigActorState) -> ClashConfigSnapshot {
        let snapshot = state.manager.snapshot_handle().load();
        ClashConfigSnapshot {
            state: snapshot.state.clone(),
            version: *snapshot.version.as_ref(),
        }
    }

    async fn commit(
        state: &mut ClashConfigActorState,
        next: ClashConfig,
    ) -> anyhow::Result<ClashConfigSnapshot> {
        state
            .manager
            .upsert(next.clone())
            .await
            .context("failed to persist clash config")?;
        state
            .bridge
            .mirror(&next)
            .context("failed to sync legacy clash mirror")?;
        Ok(Self::snapshot(state))
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
            ClashConfigActorMessage::Get(reply) => {
                let _ = reply.send(Ok(Self::snapshot(state)));
            }
            ClashConfigActorMessage::Patch { patch, reply } => {
                let result = async {
                    let mut next = state.manager.snapshot_handle().load().state.clone();
                    next.apply(patch);
                    Self::commit(state, next).await
                }
                .await;
                let _ = reply.send(result);
            }
            ClashConfigActorMessage::Replace { state: next, reply } => {
                let _ = reply.send(Self::commit(state, next).await);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_core::state::PersistentStateManagerSetup;
    use ractor::rpc::CallResult;
    use struct_patch::Patch;
    use tempfile::tempdir;

    /// Test-only double that fails every clash legacy mirror projection.
    /// Pins upsert-then-mirror for ClashConfigActor until S06.
    struct FailingClashMirror;

    impl ClashLegacyBridge for FailingClashMirror {
        fn mirror(&self, _snap: &ClashConfig) -> anyhow::Result<()> {
            anyhow::bail!("injected clash mirror failure");
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
        }
    }

    async fn spawn_actor(
        bridge: Arc<dyn ClashLegacyBridge>,
    ) -> (ActorRef<ClashConfigActorMessage>, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml"))
            .expect("temp path should be UTF-8");
        let manager = PersistentStateManagerSetup::<ClashConfig>::builder()
            .config_path(path)
            .assemble()
            .from_state(ClashConfig::default())
            .await
            .expect("clash config manager should initialize");
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ClashConfigActor,
            ClashConfigActorArgs { manager, bridge },
        )
        .await
        .expect("clash config actor should spawn");
        (actor_ref, dir)
    }

    async fn get_snapshot(
        actor: &ActorRef<ClashConfigActorMessage>,
    ) -> anyhow::Result<ClashConfigSnapshot> {
        match actor.call(ClashConfigActorMessage::Get, None).await? {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
        }
    }

    /// S01 regression contract: current commit path is upsert-then-mirror.
    /// A post-upsert mirror failure still advances typed state/version while
    /// returning Err. S06 must invert this to prepare-before-persist.
    #[tokio::test]
    async fn typed_mirror_failure_after_upsert_leaves_version_advanced() {
        let (actor, _dir) = spawn_actor(Arc::new(FailingClashMirror)).await;

        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");
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

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert!(
            after.state.enable_tun_mode,
            "upsert already committed desired state before mirror failed"
        );
        assert!(
            after.version > before_version,
            "version must advance after successful upsert even when mirror fails (before={before_version}, after={})",
            after.version
        );
    }

    /// Desired S06 invariant kept red until prepare-before-persist lands.
    /// Do not "fix green" by weakening this assertion.
    #[tokio::test]
    #[ignore = "S06 desired invariant: mirror prepare failure must leave state/version unchanged; currently red under upsert-then-mirror"]
    async fn desired_mirror_prepare_failure_leaves_state_and_version_unchanged() {
        let (actor, _dir) = spawn_actor(Arc::new(FailingClashMirror)).await;

        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");

        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let _ = actor
            .call(
                |reply| ClashConfigActorMessage::Patch { patch, reply },
                None,
            )
            .await;

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert_eq!(after.version, before.version);
        assert_eq!(after.state.enable_tun_mode, before.state.enable_tun_mode);
    }
}
