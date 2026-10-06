use nyanpasu_config::state::{PersistentState, PersistentStatePatch, window::WindowLabel};
use nyanpasu_core::state::PersistentStateManager;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use snafu::ResultExt as _;
use struct_patch::Patch;

use super::{SessionStateError, client::SessionStateSnapshot, error::PersistSessionStateSnafu};

pub(super) struct SessionStateActorArgs {
    pub manager: PersistentStateManager<PersistentState>,
    pub main_window_label: WindowLabel,
}

pub(super) struct SessionStateActorState {
    manager: PersistentStateManager<PersistentState>,
    main_window_label: WindowLabel,
}

#[derive(Debug)]
pub(super) enum SessionStateActorMessage {
    SaveMainWindow {
        geometry: nyanpasu_config::state::window::WindowState,
        /// `None` for a queued save: nobody waits, so a failure is logged.
        reply: Option<RpcReplyPort<Result<SessionStateSnapshot, SessionStateError>>>,
    },
    Patch {
        patch: PersistentStatePatch,
        reply: RpcReplyPort<Result<SessionStateSnapshot, SessionStateError>>,
    },
    Replace {
        state: PersistentState,
        reply: RpcReplyPort<Result<SessionStateSnapshot, SessionStateError>>,
    },
}

pub(super) struct SessionStateActor;

impl SessionStateActor {
    fn snapshot(state: &SessionStateActorState) -> SessionStateSnapshot {
        SessionStateSnapshot::from_versioned(&state.manager.snapshot_handle().load())
    }

    async fn commit(
        state: &mut SessionStateActorState,
        next: PersistentState,
    ) -> Result<SessionStateSnapshot, SessionStateError> {
        state
            .manager
            .upsert(next)
            .await
            .context(PersistSessionStateSnafu)?;
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
            main_window_label: args.main_window_label,
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
                next.window_state
                    .insert(state.main_window_label.clone(), geometry);
                let result = Self::commit(state, next).await;
                match reply {
                    Some(reply) => {
                        let _ = reply.send(result);
                    }
                    None => {
                        if let Err(error) = result {
                            tracing::warn!(
                                "failed to save the main window geometry: {}",
                                snafu::Report::from_error(error)
                            );
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
