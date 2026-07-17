use std::sync::Arc;

use anyhow::Context as _;
use nyanpasu_config::state::{PersistentState, PersistentStatePatch};
use nyanpasu_core::state::{PersistentStateManager, ReplaceIfVersionResult, Version};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

use super::{
    ConditionalReplaceResult,
    mirror::{PreparedTypedReplace, WindowLegacyBridge},
};

#[derive(Debug, Clone)]
pub struct SessionStateSnapshot {
    pub state: PersistentState,
    pub version: u64,
}

pub struct SessionStateActorArgs {
    pub manager: PersistentStateManager<PersistentState>,
    pub bridge: Arc<dyn WindowLegacyBridge>,
}

pub struct SessionStateActorState {
    manager: PersistentStateManager<PersistentState>,
    bridge: Arc<dyn WindowLegacyBridge>,
}

#[derive(Debug)]
pub enum SessionStateActorMessage {
    Get(RpcReplyPort<anyhow::Result<SessionStateSnapshot>>),
    Patch {
        patch: PersistentStatePatch,
        reply: RpcReplyPort<anyhow::Result<SessionStateSnapshot>>,
    },
    Replace {
        state: PersistentState,
        reply: RpcReplyPort<anyhow::Result<SessionStateSnapshot>>,
    },
    PrepareReplace {
        state: PersistentState,
        reply: RpcReplyPort<anyhow::Result<PreparedTypedReplace<PersistentState>>>,
    },
    ReplacePreparedIfVersion {
        expected_version: u64,
        prepared: PreparedTypedReplace<PersistentState>,
        reply: RpcReplyPort<anyhow::Result<ConditionalReplaceResult<SessionStateSnapshot>>>,
    },
}

pub struct SessionStateActor;

impl SessionStateActor {
    fn snapshot(state: &SessionStateActorState) -> SessionStateSnapshot {
        let snapshot = state.manager.snapshot_handle().load();
        SessionStateSnapshot {
            state: snapshot.state.clone(),
            version: *snapshot.version.as_ref(),
        }
    }

    async fn commit(
        state: &mut SessionStateActorState,
        next: PersistentState,
    ) -> anyhow::Result<SessionStateSnapshot> {
        let (next, mirror) = Self::prepare_replace(state, next)?.into_parts();
        state
            .manager
            .upsert(next)
            .await
            .context("failed to persist session state")?;
        mirror.apply();
        Ok(Self::snapshot(state))
    }

    fn prepare_replace(
        state: &SessionStateActorState,
        next: PersistentState,
    ) -> anyhow::Result<PreparedTypedReplace<PersistentState>> {
        let mirror = state
            .bridge
            .prepare(&next)
            .context("failed to prepare legacy session mirror")?;
        Ok(PreparedTypedReplace::new(next, mirror))
    }

    async fn replace_prepared_if_version(
        state: &mut SessionStateActorState,
        expected_version: u64,
        prepared: PreparedTypedReplace<PersistentState>,
    ) -> anyhow::Result<ConditionalReplaceResult<SessionStateSnapshot>> {
        let (next, mirror) = prepared.into_parts();
        match state
            .manager
            .replace_if_version(Version::new(expected_version), next)
            .await
            .context("failed to conditionally persist session state")?
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

impl Actor for SessionStateActor {
    type Msg = SessionStateActorMessage;
    type State = SessionStateActorState;
    type Arguments = SessionStateActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(SessionStateActorState {
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
            SessionStateActorMessage::Get(reply) => {
                let _ = reply.send(Ok(Self::snapshot(state)));
            }
            SessionStateActorMessage::Patch { patch, reply } => {
                let result = async {
                    let mut next = state.manager.snapshot_handle().load().state.clone();
                    next.apply(patch);
                    Self::commit(state, next).await
                }
                .await;
                let _ = reply.send(result);
            }
            SessionStateActorMessage::Replace { state: next, reply } => {
                let _ = reply.send(Self::commit(state, next).await);
            }
            SessionStateActorMessage::PrepareReplace { state: next, reply } => {
                let _ = reply.send(Self::prepare_replace(state, next));
            }
            SessionStateActorMessage::ReplacePreparedIfVersion {
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
    use nyanpasu_config::state::window::{WindowLabel, WindowState};
    use nyanpasu_core::state::PersistentStateManagerSetup;
    use ractor::rpc::CallResult;
    use std::collections::BTreeMap;
    use struct_patch::Patch;
    use tempfile::tempdir;

    /// Test-only double that fails every session/window mirror preparation.
    struct FailingWindowMirror;

    impl WindowLegacyBridge for FailingWindowMirror {
        fn prepare(
            &self,
            _snap: &PersistentState,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            anyhow::bail!("injected session mirror prepare failure");
        }

        fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
            Ok(PersistentState::default())
        }
    }

    async fn spawn_actor(
        bridge: Arc<dyn WindowLegacyBridge>,
    ) -> (ActorRef<SessionStateActorMessage>, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("session-state.yaml"))
            .expect("temp path should be UTF-8");
        let manager = PersistentStateManagerSetup::<PersistentState>::builder()
            .config_path(path)
            .assemble()
            .from_state(PersistentState::default())
            .await
            .expect("session manager should initialize");
        let (actor_ref, _handle) = Actor::spawn(
            None,
            SessionStateActor,
            SessionStateActorArgs { manager, bridge },
        )
        .await
        .expect("session state actor should spawn");
        (actor_ref, dir)
    }

    async fn get_snapshot(
        actor: &ActorRef<SessionStateActorMessage>,
    ) -> anyhow::Result<SessionStateSnapshot> {
        match actor.call(SessionStateActorMessage::Get, None).await? {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("session state actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("session state actor call timed out"),
        }
    }

    #[tokio::test]
    async fn mirror_prepare_failure_returns_error_without_commit() {
        let (actor, _dir) = spawn_actor(Arc::new(FailingWindowMirror)).await;

        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");
        assert!(before.state.window_state.is_empty());
        let before_version = before.version;

        let label = WindowLabel("main".into());
        let window = WindowState {
            width: 1024,
            height: 768,
            x: 10,
            y: 20,
            maximized: false,
            fullscreen: false,
        };
        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(BTreeMap::from([(label.clone(), window.clone())]));

        let err = match actor
            .call(
                |reply| SessionStateActorMessage::Patch { patch, reply },
                None,
            )
            .await
            .expect("actor call should complete")
        {
            CallResult::Success(result) => result
                .expect_err("mirror failure after upsert must surface as Err under current defect"),
            CallResult::SenderError => panic!("session state actor reply dropped"),
            CallResult::Timeout => panic!("session state actor call timed out"),
        };
        assert!(
            err.to_string().contains("legacy session mirror")
                || err.to_string().contains("injected session mirror failure"),
            "unexpected error: {err:#}"
        );

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert_eq!(after.state.window_state, before.state.window_state);
        assert_eq!(after.version, before_version);
    }

    #[tokio::test]
    async fn mirror_prepare_failure_leaves_state_and_version_unchanged() {
        let (actor, _dir) = spawn_actor(Arc::new(FailingWindowMirror)).await;

        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");

        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(BTreeMap::from([(
            WindowLabel("main".into()),
            WindowState {
                width: 1024,
                height: 768,
                x: 10,
                y: 20,
                maximized: false,
                fullscreen: false,
            },
        )]));
        let _ = actor
            .call(
                |reply| SessionStateActorMessage::Patch { patch, reply },
                None,
            )
            .await;

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert_eq!(after.version, before.version);
        assert_eq!(after.state.window_state, before.state.window_state);
    }
}
