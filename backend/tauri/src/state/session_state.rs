use anyhow::Context as _;
use nyanpasu_config::state::{PersistentState, PersistentStatePatch};
use nyanpasu_core::state::{PersistentStateManager, VersionedState};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

#[derive(Debug, Clone)]
pub struct SessionStateSnapshot {
    pub state: PersistentState,
    pub version: u64,
}

impl SessionStateSnapshot {
    pub(crate) fn from_versioned(versioned: &VersionedState<PersistentState>) -> Self {
        Self {
            state: versioned.state.clone(),
            version: *versioned.version.as_ref(),
        }
    }
}

pub struct SessionStateActorArgs {
    pub manager: PersistentStateManager<PersistentState>,
}

pub struct SessionStateActorState {
    manager: PersistentStateManager<PersistentState>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum SessionStateActorMessage {
    SaveMainWindow {
        geometry: nyanpasu_config::state::window::WindowState,
        /// `None` for a queued save: nobody waits, so a failure is logged.
        reply: Option<RpcReplyPort<anyhow::Result<SessionStateSnapshot>>>,
    },
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
        SessionStateSnapshot::from_versioned(&state.manager.snapshot_handle().load())
    }

    async fn commit(
        state: &mut SessionStateActorState,
        next: PersistentState,
    ) -> anyhow::Result<SessionStateSnapshot> {
        state
            .manager
            .upsert(next)
            .await
            .context("failed to persist session state")?;
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
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            SessionStateActorMessage::SaveMainWindow { geometry, reply } => {
                let mut next = Self::snapshot(state).state;
                next.window_state.insert(
                    nyanpasu_config::state::window::WindowLabel("main".into()),
                    geometry,
                );
                let result = Self::commit(state, next).await;
                match reply {
                    Some(reply) => {
                        let _ = reply.send(result);
                    }
                    None => {
                        if let Err(error) = result {
                            tracing::warn!("failed to save the main window geometry: {error:#}");
                        }
                    }
                }
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
