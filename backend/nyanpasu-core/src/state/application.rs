use crate::state::{
    config_error::{
        ConfigError, LeaveNightlyChannelSnafu, ShuttingDownSnafu, VersionConflictSnafu,
    },
    mutation::{DomainChange, MutationCoordinator, impact, ports::RuntimeCandidate},
};
use nyanpasu_core_manager::OperationId;
use nyanpasu_runtime_values::mutation::ConfigDomain;

use nyanpasu_config::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};
use nyanpasu_migration::modules::application::ApplicationFormat;
use nyanpasu_state_engine::{PersistentStateManager, ReplaceIfVersionResult, VersionedState};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use snafu::ensure;
use struct_patch::Patch;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct ApplicationSnapshot {
    pub state: NyanpasuAppConfig,
    /// The engine version published before this owner answered.
    pub version: nyanpasu_state_engine::Version,
    /// Whether this commit changed an input consumed by the runtime.
    pub reaches_runtime: bool,
    pub(crate) operation_id: Option<OperationId>,
}

impl ApplicationSnapshot {
    pub(crate) fn from_versioned(versioned: &VersionedState<NyanpasuAppConfig>) -> Self {
        Self {
            reaches_runtime: false,
            operation_id: None,
            state: versioned.state.clone(),
            version: versioned.version,
        }
    }
}

pub struct ApplicationActorArgs {
    pub(crate) mutations: MutationCoordinator,
    pub manager: PersistentStateManager<NyanpasuAppConfig, ApplicationFormat>,
    /// The channel of the installed build; only a nightly build is held to it.
    pub build_channel: nyanpasu_config::application::ReleaseChannel,
    /// Once cancelled, every write is refused.
    pub shutdown: CancellationToken,
    pub settings_changes: tokio::sync::watch::Sender<NyanpasuAppConfig>,
}

pub struct ApplicationActorState {
    mutations: MutationCoordinator,
    manager: PersistentStateManager<NyanpasuAppConfig, ApplicationFormat>,
    build_channel: nyanpasu_config::application::ReleaseChannel,
    shutdown: CancellationToken,
    settings_changes: tokio::sync::watch::Sender<NyanpasuAppConfig>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ApplicationActorMessage {
    Patch {
        patch: NyanpasuAppConfigPatch,
        reply: RpcReplyPort<Result<ApplicationSnapshot, ConfigError>>,
    },
    Replace {
        state: NyanpasuAppConfig,
        reply: RpcReplyPort<Result<ApplicationSnapshot, ConfigError>>,
    },
}

pub struct ApplicationActor;

impl ApplicationActor {
    async fn patch(
        state: &mut ApplicationActorState,
        patch: NyanpasuAppConfigPatch,
    ) -> Result<ApplicationSnapshot, ConfigError> {
        let mut next = state.manager.snapshot_handle().load().state.clone();
        let requested = crate::client::effects::plan::requested_owners(&patch);
        next.apply(patch);
        Self::commit(state, next, requested).await
    }

    fn snapshot(state: &ApplicationActorState) -> ApplicationSnapshot {
        ApplicationSnapshot::from_versioned(&state.manager.snapshot_handle().load())
    }

    fn validate_channel(
        state: &ApplicationActorState,
        next: &mut NyanpasuAppConfig,
    ) -> Result<(), ConfigError> {
        use nyanpasu_config::application::ReleaseChannel as Channel;
        let current = state.manager.snapshot_handle().load();
        next.release_channel = next.release_channel.or(current.state.release_channel);
        // A Nightly preference saved by a nightly build does not bind a stable
        // or beta build installed over it.
        if let (Channel::Nightly, Some(to)) = (state.build_channel, next.release_channel) {
            ensure!(to == Channel::Nightly, LeaveNightlyChannelSnafu { to });
        }
        Ok(())
    }

    async fn commit(
        state: &mut ApplicationActorState,
        mut next: NyanpasuAppConfig,
        requested: Vec<nyanpasu_runtime_values::configuration::EffectKind>,
    ) -> Result<ApplicationSnapshot, ConfigError> {
        state.mutations.ensure_ready()?;
        Self::validate_channel(state, &mut next)?;
        next.core_logs
            .validate()
            .map_err(|reason| ConfigError::InvalidCoreLogs {
                reason: reason.into(),
            })?;
        nyanpasu_config::application::validate_update_sources(&next.update_sources).map_err(
            |reason| ConfigError::InvalidUpdateSources {
                reason: reason.into(),
            },
        )?;
        nyanpasu_config::application::validate_public_ip_probe_sources(
            &next.stun_servers,
            &next.ip_lookup_providers,
        )
        .map_err(|reason| ConfigError::InvalidPublicIpProbeSources {
            reason: reason.into(),
        })?;
        nyanpasu_config::application::validate_latency_timeout(next.default_latency_timeout_ms)
            .map_err(|reason| ConfigError::InvalidLatencyTimeout {
                reason: reason.into(),
            })?;
        let current = state.manager.snapshot_handle().load();
        let version = current.version;
        let reaches_runtime = impact::reaches_runtime(&current.state, &next, &[]);
        let operation_id = reaches_runtime.then(OperationId::generate);
        let gate = state.mutations.runtime();
        if reaches_runtime {
            gate.check(RuntimeCandidate {
                change: DomainChange::Application {
                    candidate: std::sync::Arc::new(next.clone()),
                },
                staged: Default::default(),
            })
            .await
            .map_err(|error| {
                ConfigError::runtime_check_failure(ConfigDomain::Application, error)
            })?;
        }
        let result = state.manager.replace_if_version(version, next).await;
        match result {
            Ok(ReplaceIfVersionResult::Replaced) => {
                let mut snapshot = Self::snapshot(state);
                state
                    .mutations
                    .effects()
                    .application_committed((&snapshot.state).into(), requested);
                state.settings_changes.send_replace(snapshot.state.clone());
                snapshot.reaches_runtime = reaches_runtime;
                snapshot.operation_id = operation_id;
                if reaches_runtime {
                    gate.committed(ConfigDomain::Application, snapshot.version);
                }
                Ok(snapshot)
            }
            Ok(ReplaceIfVersionResult::Conflict { actual_version }) => VersionConflictSnafu {
                domain: ConfigDomain::Application,
                expected: *version.as_ref(),
                actual: *actual_version.as_ref(),
            }
            .fail(),
            Err(error) => Err(ConfigError::commit_failure(
                ConfigDomain::Application,
                error,
            )),
        }
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
            build_channel: args.build_channel,
            shutdown: args.shutdown,
            settings_changes: args.settings_changes,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ApplicationActorMessage::Patch { reply, .. }
            | ApplicationActorMessage::Replace { reply, .. }
                if state.shutdown.is_cancelled() =>
            {
                let _ = reply.send(
                    ShuttingDownSnafu {
                        domain: ConfigDomain::Application,
                    }
                    .fail(),
                );
            }
            ApplicationActorMessage::Patch { patch, reply } => {
                let _ = reply.send(Self::patch(state, patch).await);
            }
            ApplicationActorMessage::Replace { state: next, reply } => {
                let _ = reply.send(Self::commit(state, next, Default::default()).await);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
