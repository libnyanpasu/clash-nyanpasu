use crate::{
    client::application_workflow::{
        impact::{self, MutationHints, RequestedRuntimeFields},
        mutation::ConfigDomain,
        policy::CommandClass,
    },
    core::migration::modules::application::ApplicationFormat,
    state::{
        config_error::{
            ConfigError, LeaveNightlyChannelSnafu, ShuttingDownSnafu, VersionConflictSnafu,
        },
        mutation::MutationCoordinator,
    },
};
use nyanpasu_core_manager::OperationId;

use nyanpasu_config::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};
use nyanpasu_core::state::{PersistentStateManager, ReplaceIfVersionResult, VersionedState};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use snafu::ensure;
use struct_patch::Patch;
use tokio_util::sync::CancellationToken;

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
        hints: MutationHints,
        class: CommandClass,
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
        nyanpasu_config::application::validate_latency_timeout(next.default_latency_timeout_ms)
            .map_err(|reason| ConfigError::InvalidLatencyTimeout {
                reason: reason.into(),
            })?;
        let (version, impact) = {
            let current = state.manager.snapshot_handle().load();
            let impact = impact::runtime_impact(&current.state, &next, &hints, class);
            (current.version, impact)
        };
        let requested = hints.requested_owners.clone();
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
                    .application_committed((&snapshot.state).into(), requested);
                state.settings_changes.send_replace(snapshot.state.clone());
                let (receipt, degradations) = state.mutations.committed(
                    operation,
                    "application",
                    snapshot.version,
                    settlement,
                );
                snapshot.receipt = Some(receipt);
                snapshot.degradations = degradations;
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
                settlement.as_ref(),
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
