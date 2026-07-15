use std::sync::Arc;

use anyhow::Context as _;
use nyanpasu_config::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};
use nyanpasu_core::state::PersistentStateManager;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

use super::mirror::VergeLegacyBridge;

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

    async fn commit(
        state: &mut ApplicationActorState,
        next: NyanpasuAppConfig,
    ) -> anyhow::Result<ApplicationSnapshot> {
        state
            .manager
            .upsert(next.clone())
            .await
            .context("failed to persist application config")?;
        state
            .bridge
            .mirror(&next)
            .context("failed to sync legacy application mirror")?;
        Ok(Self::snapshot(state))
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

    /// Test-only double that fails every mirror projection.
    /// Used to pin the upsert-then-mirror ordering defect until S06.
    struct FailingVergeMirror;

    impl VergeLegacyBridge for FailingVergeMirror {
        fn mirror(&self, _snap: &NyanpasuAppConfig) -> anyhow::Result<()> {
            anyhow::bail!("injected application mirror failure");
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

    /// S01 regression contract: current commit path is upsert-then-mirror.
    /// A post-upsert mirror failure still advances typed state/version while
    /// returning Err. S06 must invert this to prepare-before-persist.
    #[tokio::test]
    async fn typed_mirror_failure_after_upsert_leaves_version_advanced() {
        let (actor, _dir) = spawn_actor(Arc::new(FailingVergeMirror)).await;

        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");
        assert!(!before.state.enable_system_proxy);
        let before_version = before.version;

        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_system_proxy = Some(true);
        let err = match actor
            .call(
                |reply| ApplicationActorMessage::Patch { patch, reply },
                None,
            )
            .await
            .expect("actor call should complete")
        {
            CallResult::Success(result) => result
                .expect_err("mirror failure after upsert must surface as Err under current defect"),
            CallResult::SenderError => panic!("application actor reply dropped"),
            CallResult::Timeout => panic!("application actor call timed out"),
        };
        assert!(
            err.to_string().contains("legacy application mirror")
                || err
                    .to_string()
                    .contains("injected application mirror failure"),
            "unexpected error: {err:#}"
        );

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        // Current defective behavior: typed state and version already advanced.
        assert!(
            after.state.enable_system_proxy,
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
        let (actor, _dir) = spawn_actor(Arc::new(FailingVergeMirror)).await;

        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");

        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_system_proxy = Some(true);
        let _ = actor
            .call(
                |reply| ApplicationActorMessage::Patch { patch, reply },
                None,
            )
            .await;

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
