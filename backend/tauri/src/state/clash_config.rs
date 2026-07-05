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
