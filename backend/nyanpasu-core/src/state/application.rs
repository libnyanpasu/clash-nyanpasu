use crate::state::{
    config_error::{ConfigError, LeaveNightlyChannelSnafu, ShuttingDownSnafu},
    mutation::{
        DomainChange, MutationCoordinator, impact,
        ports::{CheckPass, RuntimeCandidate},
    },
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
    outcomes: crate::state::mutation::ports::OwnerOutcomes,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ApplicationActorMessage {
    Patch {
        patch: NyanpasuAppConfigPatch,
        attribution: crate::state::mutation::ports::Attribution,
        reply: RpcReplyPort<Result<ApplicationSnapshot, ConfigError>>,
    },
    Replace {
        state: NyanpasuAppConfig,
        attribution: crate::state::mutation::ports::Attribution,
        reply: RpcReplyPort<Result<ApplicationSnapshot, ConfigError>>,
    },
}

pub struct ApplicationActor;

impl ApplicationActor {
    async fn patch(
        state: &mut ApplicationActorState,
        patch: NyanpasuAppConfigPatch,
        attribution: crate::state::mutation::ports::Attribution,
    ) -> Result<ApplicationSnapshot, ConfigError> {
        let mut next = state.manager.snapshot_handle().load().state.clone();
        let requested = crate::client::effects::plan::requested_owners(&patch);
        next.apply(patch);
        Self::commit(state, next, requested, attribution).await
    }

    /// Decides `next` and reports the outcome before anyone hears of it,
    /// so the record follows this owner's order whatever its callers do.
    async fn commit(
        state: &mut ApplicationActorState,
        next: NyanpasuAppConfig,
        requested: Vec<nyanpasu_runtime_values::configuration::EffectKind>,
        attribution: crate::state::mutation::ports::Attribution,
    ) -> Result<ApplicationSnapshot, ConfigError> {
        let before = state.manager.snapshot_handle().load().state.clone();
        let result = Self::decide(state, next, requested).await;
        if state.mutations.ensure_ready().is_ok() {
            let record = state.mutations.outcomes();
            match &result {
                Ok(snapshot) => state.outcomes.committed(
                    record.as_ref(),
                    attribution,
                    crate::state::mutation::ports::TraySetting::changed_application(
                        &before,
                        &snapshot.state,
                    ),
                    snapshot.version,
                ),
                Err(error) => state.outcomes.failed(
                    record.as_ref(),
                    attribution,
                    state.manager.snapshot_handle().load().version,
                    error,
                    error.candidate_snapshot_id(),
                ),
            }
        }
        result
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

    async fn decide(
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
        nyanpasu_config::application::validate_theme_contrast_level(next.theme_contrast_level)
            .map_err(|reason| ConfigError::InvalidThemeContrastLevel {
                reason: reason.into(),
            })?;
        next.network_automation.validate().map_err(|source| {
            ConfigError::InvalidNetworkAutomation {
                reason: source.to_string(),
            }
        })?;
        let current = state.manager.snapshot_handle().load();
        let version = current.version;
        let reaches_runtime = impact::reaches_runtime(&current.state, &next, &[]);
        let operation_id = reaches_runtime.then(OperationId::generate);
        let gate = state.mutations.runtime();
        let pass = if reaches_runtime {
            gate.check(RuntimeCandidate {
                background: None,
                change: DomainChange::Application {
                    candidate: std::sync::Arc::new(next.clone()),
                },
                staged: Default::default(),
            })
            .await
            .map_err(|error| ConfigError::runtime_check_failure(ConfigDomain::Application, error))?
        } else {
            CheckPass::default()
        };
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
            Ok(ReplaceIfVersionResult::Conflict { actual_version }) => Err(pass
                .keep_for(ConfigError::VersionConflict {
                    domain: ConfigDomain::Application,
                    expected: *version.as_ref(),
                    actual: *actual_version.as_ref(),
                    candidate_snapshot_id: None,
                })
                .await),
            Err(error) => Err(pass
                .keep_for(ConfigError::commit_failure(
                    ConfigDomain::Application,
                    error,
                ))
                .await),
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
            outcomes: Default::default(),
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
            ApplicationActorMessage::Patch {
                patch,
                attribution,
                reply,
            } => {
                let _ = reply.send(Self::patch(state, patch, attribution).await);
            }
            ApplicationActorMessage::Replace {
                state: next,
                attribution,
                reply,
            } => {
                let _ =
                    reply.send(Self::commit(state, next, Default::default(), attribution).await);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
