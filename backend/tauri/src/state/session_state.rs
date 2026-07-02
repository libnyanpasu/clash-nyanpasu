use std::sync::Arc;

use anyhow::Context as _;
use nyanpasu_config::state::{PersistentState, PersistentStatePatch};
use nyanpasu_core::state::PersistentStateManager;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

use super::mirror::WindowLegacyBridge;

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
        state
            .manager
            .upsert(next.clone())
            .await
            .context("failed to persist session state")?;
        state
            .bridge
            .mirror(&next)
            .context("failed to sync legacy session mirror")?;
        Ok(Self::snapshot(state))
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
        }
        Ok(())
    }
}
