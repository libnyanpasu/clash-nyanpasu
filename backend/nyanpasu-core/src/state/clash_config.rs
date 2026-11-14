use crate::state::{
    config_error::{ConfigError, ShuttingDownSnafu, VersionConflictSnafu},
    mutation::{DomainChange, MutationCoordinator, impact, ports::RuntimeCandidate},
};
use nyanpasu_core_manager::OperationId;
use nyanpasu_runtime_values::mutation::ConfigDomain;

use nyanpasu_config::clash::config::{
    ClashConfig, ClashConfigPatch, overrides::ClashGuardOverridesPatch,
};
use nyanpasu_migration::modules::clash_config::ClashConfigFormat;
use nyanpasu_state_engine::{PersistentStateManager, ReplaceIfVersionResult, VersionedState};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;
use tokio_util::sync::CancellationToken;

/// Snapshot of the saved Clash configuration domain, not live Clash runtime API state.
#[derive(Debug, Clone)]
pub struct ClashConfigSnapshot {
    pub state: ClashConfig,
    /// The engine version published before this owner answered.
    pub version: nyanpasu_state_engine::Version,
    /// Whether this commit changed an input consumed by the runtime.
    pub reaches_runtime: bool,
    pub(crate) operation_id: Option<OperationId>,
}

impl ClashConfigSnapshot {
    pub(crate) fn from_versioned(versioned: &VersionedState<ClashConfig>) -> Self {
        Self {
            reaches_runtime: false,
            operation_id: None,
            state: versioned.state.clone(),
            version: versioned.version,
        }
    }
}

pub struct ClashConfigActorArgs {
    pub(crate) mutations: MutationCoordinator<ClashConfig>,
    pub manager: PersistentStateManager<ClashConfig, ClashConfigFormat>,
    /// Once cancelled, every write is refused.
    pub shutdown: CancellationToken,
}

pub struct ClashConfigActorState {
    mutations: MutationCoordinator<ClashConfig>,
    manager: PersistentStateManager<ClashConfig, ClashConfigFormat>,
    shutdown: CancellationToken,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ClashConfigActorMessage {
    Patch {
        patch: ClashConfigPatch,
        reply: RpcReplyPort<Result<ClashConfigSnapshot, ConfigError>>,
    },
    PatchOverrides {
        patch: ClashGuardOverridesPatch,
        reply: RpcReplyPort<Result<ClashConfigSnapshot, ConfigError>>,
    },
    Replace {
        state: ClashConfig,
        reply: RpcReplyPort<Result<ClashConfigSnapshot, ConfigError>>,
    },
}

/// Actor-owned persistent Clash configuration. Runtime Clash API state stays in the core/API path.
pub struct ClashConfigActor;

impl ClashConfigActor {
    async fn patch(
        state: &mut ClashConfigActorState,
        patch: ClashConfigPatch,
    ) -> Result<ClashConfigSnapshot, ConfigError> {
        let mut next = state.manager.snapshot_handle().load().state.clone();
        next.apply(patch);
        Self::commit(state, next).await
    }

    fn snapshot(state: &ClashConfigActorState) -> ClashConfigSnapshot {
        ClashConfigSnapshot::from_versioned(&state.manager.snapshot_handle().load())
    }

    async fn commit(
        state: &mut ClashConfigActorState,
        next: ClashConfig,
    ) -> Result<ClashConfigSnapshot, ConfigError> {
        state.mutations.ensure_ready()?;
        let current = state.manager.snapshot_handle().load();
        let version = current.version;
        let reaches_runtime = impact::clash_target(&current.state) != impact::clash_target(&next);
        let operation_id = reaches_runtime.then(OperationId::generate);
        let gate = state.mutations.runtime();
        if reaches_runtime {
            gate.check(RuntimeCandidate {
                change: DomainChange::Clash {
                    candidate: std::sync::Arc::new(next.clone()),
                },
                staged: Default::default(),
            })
            .await
            .map_err(|error| ConfigError::runtime_check_failure(ConfigDomain::Clash, error))?;
        }
        let result = state.manager.replace_if_version(version, next).await;
        match result {
            Ok(ReplaceIfVersionResult::Replaced) => {
                let mut snapshot = Self::snapshot(state);
                state
                    .mutations
                    .effects()
                    .clash_committed((&snapshot.state).into());
                snapshot.reaches_runtime = reaches_runtime;
                snapshot.operation_id = operation_id;
                if reaches_runtime {
                    gate.committed(ConfigDomain::Clash, snapshot.version);
                }
                Ok(snapshot)
            }
            Ok(ReplaceIfVersionResult::Conflict { actual_version }) => VersionConflictSnafu {
                domain: ConfigDomain::Clash,
                expected: *version.as_ref(),
                actual: *actual_version.as_ref(),
            }
            .fail(),
            Err(error) => Err(ConfigError::commit_failure(
                ConfigDomain::Clash,
                error,
                None,
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
                let _ = reply.send(
                    ShuttingDownSnafu {
                        domain: ConfigDomain::Clash,
                    }
                    .fail(),
                );
            }
            ClashConfigActorMessage::Patch { patch, reply } => {
                let _ = reply.send(Self::patch(state, patch).await);
            }
            ClashConfigActorMessage::PatchOverrides { patch, reply } => {
                let mut next = state.manager.snapshot_handle().load().state.clone();
                next.overrides.apply(patch);
                let _ = reply.send(Self::commit(state, next).await);
            }
            ClashConfigActorMessage::Replace { state: next, reply } => {
                let _ = reply.send(Self::commit(state, next).await);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
