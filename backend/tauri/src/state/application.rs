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
