use crate::{
    client::application_workflow::{
        impact::{self, MutationHints, RequestedRuntimeFields},
        policy::CommandClass,
    },
    state::mutation::{self, MutationCoordinator},
};
use nyanpasu_core_manager::OperationId;

use nyanpasu_config::clash::config::{
    ClashConfig, ClashConfigPatch, overrides::ClashGuardOverridesPatch,
};
use nyanpasu_core::state::{PersistentStateManager, ReplaceIfVersionResult, VersionedState};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;
use tokio_util::sync::CancellationToken;

/// Snapshot of the saved Clash configuration domain, not live Clash runtime API state.
#[derive(Debug, Clone)]
pub struct ClashConfigSnapshot {
    pub state: ClashConfig,
    pub version: u64,
    pub(crate) receipt: Option<crate::client::runtime::CommitReceipt>,
    pub(crate) degradations: Vec<crate::client::runtime::Degradation>,
}

impl ClashConfigSnapshot {
    pub(crate) fn outcome(self) -> crate::client::runtime::MutationOutcome<()> {
        let outcome = crate::client::runtime::MutationOutcome::from_parts((), self.degradations);
        match self.receipt {
            Some(receipt) => outcome.with_commit(receipt),
            None => outcome,
        }
    }

    pub(crate) fn from_versioned(versioned: &VersionedState<ClashConfig>) -> Self {
        Self {
            receipt: None,
            degradations: Vec::new(),
            state: versioned.state.clone(),
            version: *versioned.version.as_ref(),
        }
    }
}

pub struct ClashConfigActorArgs {
    pub(crate) mutations: MutationCoordinator,
    pub manager: PersistentStateManager<ClashConfig>,
    /// Once cancelled, every write is refused.
    pub shutdown: CancellationToken,
}

pub struct ClashConfigActorState {
    mutations: MutationCoordinator,
    manager: PersistentStateManager<ClashConfig>,
    shutdown: CancellationToken,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ClashConfigActorMessage {
    Patch {
        patch: ClashConfigPatch,
        reply: RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>,
    },
    PatchOverrides {
        patch: ClashGuardOverridesPatch,
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
    async fn patch(
        state: &mut ClashConfigActorState,
        patch: ClashConfigPatch,
    ) -> anyhow::Result<ClashConfigSnapshot> {
        let mut next = state.manager.snapshot_handle().load().state.clone();
        let hints = MutationHints {
            requested: RequestedRuntimeFields::of_clash(&patch),
            ..Default::default()
        };
        let class = CommandClass::Save;
        next.apply(patch);
        Self::commit(state, next, hints, class).await
    }

    fn snapshot(state: &ClashConfigActorState) -> ClashConfigSnapshot {
        ClashConfigSnapshot::from_versioned(&state.manager.snapshot_handle().load())
    }

    async fn commit(
        state: &mut ClashConfigActorState,
        next: ClashConfig,
        hints: MutationHints,
        class: CommandClass,
    ) -> anyhow::Result<ClashConfigSnapshot> {
        state.mutations.ensure_ready()?;
        let (version, impact) = {
            let current = state.manager.snapshot_handle().load();
            let impact = impact::runtime_impact(&current.state, &next, &hints, class);
            (current.version, impact)
        };
        // Only a mutation that reaches the runtime takes the Runtime into its
        // transaction; any other save commits on its own.
        let (operation, result, settlement) = match impact {
            Some(impact) => {
                let operation = OperationId::generate();
                let (participant, settlement) = state
                    .mutations
                    .participant(operation, hints, class, impact)?;
                let result = state
                    .manager
                    .replace_if_version_with_participant(
                        version,
                        next,
                        participant,
                        || async { Ok(()) },
                        || async { Ok(()) },
                    )
                    .await;
                (Some(operation), result, settlement.await.ok())
            }
            None => (
                None,
                state.manager.replace_if_version(version, next).await,
                None,
            ),
        };
        match result {
            Ok(ReplaceIfVersionResult::Replaced) => {
                let mut snapshot = Self::snapshot(state);
                state
                    .mutations
                    .effects()
                    .clash_committed((&snapshot.state).into());
                let (receipt, degradations) =
                    state
                        .mutations
                        .committed(operation, "clash", snapshot.version, settlement);
                snapshot.receipt = Some(receipt);
                snapshot.degradations = degradations;
                Ok(snapshot)
            }
            Ok(ReplaceIfVersionResult::Conflict { actual_version }) => Err(anyhow::anyhow!(
                "clash config version conflict: expected {}, actual {}",
                version.as_ref(),
                actual_version.as_ref()
            )),
            Err(error) => Err(anyhow::anyhow!(
                "failed to persist clash config: {}",
                snafu::Report::from_error(mutation::CommitAborted::classify(
                    error,
                    settlement.as_ref()
                ))
            )),
        }
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
            mutations: args.mutations,
            manager: args.manager,
            shutdown: args.shutdown,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ClashConfigActorMessage::Patch { reply, .. }
            | ClashConfigActorMessage::PatchOverrides { reply, .. }
            | ClashConfigActorMessage::Replace { reply, .. }
                if state.shutdown.is_cancelled() =>
            {
                let _ = reply.send(Err(anyhow::anyhow!(
                    "the clash config is closed: the app is shutting down"
                )));
            }
            ClashConfigActorMessage::Patch { patch, reply } => {
                let _ = reply.send(Self::patch(state, patch).await);
            }
            ClashConfigActorMessage::PatchOverrides { patch, reply } => {
                let mut next = state.manager.snapshot_handle().load().state.clone();
                let hints = MutationHints {
                    mode_requested: patch.mode.is_some(),
                    requested: RequestedRuntimeFields::of_clash_overrides(&patch),
                    ..Default::default()
                };
                next.overrides.apply(patch);
                let _ = reply.send(Self::commit(state, next, hints, CommandClass::Save).await);
            }
            ClashConfigActorMessage::Replace { state: next, reply } => {
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
