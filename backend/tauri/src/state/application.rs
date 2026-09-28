use crate::{
    client::application_workflow::{
        impact::{MutationHints, RequestedRuntimeFields},
        policy::CommandClass,
    },
    state::mutation::MutationCoordinator,
};
use nyanpasu_core_manager::OperationId;

use anyhow::Context as _;
use nyanpasu_config::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};
use nyanpasu_core::state::{PersistentStateManager, VersionedState};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

#[derive(Debug, Clone)]
pub struct ApplicationSnapshot {
    pub state: NyanpasuAppConfig,
    pub version: u64,
    pub(crate) receipt: Option<crate::client::runtime::CommitReceipt>,
    pub(crate) degradations: Vec<crate::client::runtime::Degradation>,
}

impl ApplicationSnapshot {
    pub(crate) fn outcome(self) -> crate::client::runtime::MutationOutcome<()> {
        let outcome = crate::client::runtime::MutationOutcome::from_parts((), self.degradations);
        match self.receipt {
            Some(receipt) => outcome.with_commit(receipt),
            None => outcome,
        }
    }

    pub(crate) fn from_versioned(versioned: &VersionedState<NyanpasuAppConfig>) -> Self {
        Self {
            receipt: None,
            degradations: Vec::new(),
            state: versioned.state.clone(),
            version: *versioned.version.as_ref(),
        }
    }
}

pub struct ApplicationActorArgs {
    pub(crate) mutations: MutationCoordinator,
    pub manager: PersistentStateManager<NyanpasuAppConfig>,
}

pub struct ApplicationActorState {
    mutations: MutationCoordinator,
    manager: PersistentStateManager<NyanpasuAppConfig>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ApplicationActorMessage {
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
    async fn patch(
        state: &mut ApplicationActorState,
        patch: NyanpasuAppConfigPatch,
    ) -> anyhow::Result<ApplicationSnapshot> {
        let mut next = state.manager.snapshot_handle().load().state.clone();
        let hints = MutationHints {
            requested: RequestedRuntimeFields::of_application(&patch),
            requested_owners: crate::client::effects::plan::requested_owners(&patch),
            ..Default::default()
        };
        let class = if patch.core.is_some() || patch.enable_service_mode.is_some() {
            CommandClass::ExplicitSwitch
        } else {
            CommandClass::Save
        };
        next.apply(patch);
        Self::commit(state, next, hints, class).await
    }

    fn snapshot(state: &ApplicationActorState) -> ApplicationSnapshot {
        ApplicationSnapshot::from_versioned(&state.manager.snapshot_handle().load())
    }

    fn validate_channel(
        state: &ApplicationActorState,
        next: &mut NyanpasuAppConfig,
    ) -> anyhow::Result<()> {
        use nyanpasu_config::application::ReleaseChannel as Channel;
        let current = state.manager.snapshot_handle().load();
        next.release_channel = next.release_channel.or(current.state.release_channel);
        anyhow::ensure!(
            current.state.release_channel != Some(Channel::Nightly)
                || next.release_channel == Some(Channel::Nightly),
            "cannot leave the nightly release channel"
        );
        Ok(())
    }

    async fn commit(
        state: &mut ApplicationActorState,
        mut next: NyanpasuAppConfig,
        hints: MutationHints,
        class: CommandClass,
    ) -> anyhow::Result<ApplicationSnapshot> {
        Self::validate_channel(state, &mut next)?;
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
            .context("failed to persist application config")?;
        let mut snapshot = Self::snapshot(state);
        let (receipt, degradations) = state
            .mutations
            .finish(operation, "application", snapshot.version)
            .await;
        snapshot.receipt = Some(receipt);
        snapshot.degradations = degradations;
        Ok(snapshot)
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
            mutations: args.mutations,
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
            ApplicationActorMessage::Patch { patch, reply } => {
                let _ = reply.send(Self::patch(state, patch).await);
            }
            ApplicationActorMessage::Replace { state: next, reply } => {
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
        }
        Ok(())
    }
}
