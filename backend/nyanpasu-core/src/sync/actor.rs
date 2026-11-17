//! The sync owner: settings, status and one cycle at a time.
//!
//! It is an actor because it owns long-lived mutable state (settings, the
//! status revision, whether a cycle runs) and a long-running operation. The
//! mailbox is what keeps two triggers from starting two cycles; planning and
//! merging stay in the pure engine.

use std::sync::Arc;

use nyanpasu_contracts::sync::{
    ProviderKind, ProviderSecrets, RemoteError, RemoteStoreFactory, SyncSettings,
};
use nyanpasu_runtime_values::sync::{
    ConflictChoice, SyncAvailability, SyncConflict, SyncError, SyncOutcome, SyncPhase, SyncStatus,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio_util::sync::CancellationToken;

use super::{
    engine::{CycleError, CycleReport, open_store, run_cycle},
    ports::SyncPorts,
    state::SyncMeta,
};

pub(super) enum Message {
    Status(RpcReplyPort<SyncStatus>),
    Settings(RpcReplyPort<SettingsReply>),
    Configure {
        settings: SyncSettings,
        secrets: Option<ProviderSecrets>,
        reply: RpcReplyPort<Result<(), SyncFailure>>,
    },
    Enable(RpcReplyPort<Result<CycleReport, SyncFailure>>),
    Disable(RpcReplyPort<Result<(), SyncFailure>>),
    SyncNow(RpcReplyPort<Result<CycleReport, SyncFailure>>),
    TestConnection {
        settings: SyncSettings,
        secrets: Option<ProviderSecrets>,
        reply: RpcReplyPort<Result<(), SyncFailure>>,
    },
    ResolveConflict {
        conflict_id: String,
        choice: ConflictChoice,
        reply: RpcReplyPort<Result<CycleReport, SyncFailure>>,
    },
    Conflicts(RpcReplyPort<Vec<SyncConflict>>),
    RestoreSnapshot {
        snapshot_id: String,
        reply: RpcReplyPort<Result<(), SyncFailure>>,
    },
}

/// What `sync_settings` answers: the settings and whether a secret exists.
#[derive(Debug, Clone)]
pub struct SettingsReply {
    pub settings: SyncSettings,
    pub credentials_present: bool,
    pub provider_kinds: Vec<ProviderKind>,
}

/// A failure the actor reports to a caller.
#[derive(Debug, Clone, PartialEq, Eq, snafu::Snafu)]
#[snafu(visibility(pub))]
pub enum SyncFailure {
    #[snafu(display("sync is not available in this build"))]
    Unsupported,
    #[snafu(display("sync is not enabled"))]
    NotConfigured,
    #[snafu(display("the remote is unreachable"))]
    Unreachable,
    #[snafu(display("the remote rejected the credentials"))]
    Unauthorized,
    #[snafu(display("the vault was written by a newer application"))]
    VaultTooNew { found: u32, supported: u32 },
    #[snafu(display("sync failed: {reason}"))]
    Failed { reason: String },
}

pub struct Args {
    pub(crate) ports: SyncPorts,
    pub(crate) settings: SyncSettings,
    /// `None` when the shell supplied no sync adapters.
    pub(crate) adapters: Option<Arc<dyn RemoteStoreFactory>>,
    pub(crate) shutdown: CancellationToken,
}

pub(super) struct State {
    ports: SyncPorts,
    settings: SyncSettings,
    adapters: Option<Arc<dyn RemoteStoreFactory>>,
    shutdown: CancellationToken,
    status: SyncStatus,
    meta: SyncMeta,
    conflicts: Vec<SyncConflict>,
}

pub(super) struct SyncActor;

impl State {
    fn publish(&mut self) {
        self.status.revision = self.status.revision.wrapping_add(1);
        self.ports.status.publish(&self.status);
    }

    fn fail(&mut self, failure: &SyncFailure) {
        self.status.last_result = Some(SyncOutcome::Failed);
        self.status.last_error = Some(to_error(failure));
        self.status.phase = SyncPhase::Idle;
        self.status.progress = None;
        self.publish();
    }
}

impl SyncActor {
    fn availability(state: &State) -> SyncAvailability {
        if state.adapters.is_none() {
            SyncAvailability::Unsupported
        } else if state.settings.enabled {
            SyncAvailability::Enabled
        } else {
            SyncAvailability::Disabled
        }
    }

    fn secrets(state: &State) -> ProviderSecrets {
        state
            .ports
            .secrets
            .load()
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    async fn cycle(state: &mut State) -> Result<CycleReport, SyncFailure> {
        let adapters = state.adapters.clone().ok_or(SyncFailure::Unsupported)?;
        if !state.settings.enabled {
            return Err(SyncFailure::NotConfigured);
        }
        if state.shutdown.is_cancelled() {
            return Err(SyncFailure::Failed {
                reason: "the application is shutting down".to_owned(),
            });
        }
        let secrets = Self::secrets(state);
        let store =
            open_store(&adapters, &state.settings, &secrets).map_err(|error| to_failure(&error))?;
        let mut status = state.status.clone();
        status.availability = SyncAvailability::Enabled;
        let report = run_cycle(super::engine::CycleInput {
            ports: &state.ports,
            store,
            meta: &state.meta,
            settings: &state.settings,
            status: &mut status,
        })
        .await
        .map_err(|error| to_failure(&error))?;
        state.status = status;
        state.status.availability = SyncAvailability::Enabled;
        state.conflicts = merge_resolutions(&state.conflicts, report.conflicts.clone());
        state.status.open_conflicts = state
            .conflicts
            .iter()
            .filter(|conflict| conflict.resolution.is_none())
            .count() as u32;
        if report.head.is_some() {
            state.status.last_success_at = Some(super::clock::now_unix());
        }
        state.publish();
        Ok(report)
    }

    fn settings_reply(state: &State) -> SettingsReply {
        SettingsReply {
            settings: state.settings.clone(),
            credentials_present: state
                .ports
                .secrets
                .load()
                .ok()
                .flatten()
                .is_some_and(|secrets| !secrets.is_empty()),
            provider_kinds: state
                .adapters
                .as_ref()
                .map(|adapters| adapters.kinds())
                .unwrap_or_default(),
        }
    }
}

/// Keeps a recorded choice when the same conflict reappears after a cycle.
fn merge_resolutions(previous: &[SyncConflict], current: Vec<SyncConflict>) -> Vec<SyncConflict> {
    current
        .into_iter()
        .map(|mut conflict| {
            if conflict.resolution.is_none()
                && let Some(recorded) = previous
                    .iter()
                    .find(|old| old.id == conflict.id)
                    .and_then(|old| old.resolution)
            {
                conflict.resolution = Some(recorded);
            }
            conflict
        })
        .collect()
}

impl Actor for SyncActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let loaded = args.ports.state.load().ok();
        let availability = if args.adapters.is_some() {
            if args.settings.enabled {
                SyncAvailability::Enabled
            } else {
                SyncAvailability::Disabled
            }
        } else {
            SyncAvailability::Unsupported
        };
        let mut status = SyncStatus {
            availability,
            ..SyncStatus::default()
        };
        if let Some(loaded) = &loaded {
            status.open_conflicts = loaded
                .conflicts
                .iter()
                .filter(|conflict| conflict.resolution.is_none())
                .count() as u32;
            status.last_success_at = loaded.meta.last_success_at;
        }
        let meta = loaded
            .as_ref()
            .map(|loaded| loaded.meta.clone())
            .unwrap_or_default();
        let conflicts = loaded.map(|loaded| loaded.conflicts).unwrap_or_default();
        Ok(State {
            ports: args.ports,
            settings: args.settings,
            adapters: args.adapters,
            shutdown: args.shutdown,
            status,
            meta,
            conflicts,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Status(reply) => {
                let _ = reply.send(state.status.clone());
            }
            Message::Settings(reply) => {
                let _ = reply.send(Self::settings_reply(state));
            }
            Message::Configure {
                settings,
                secrets,
                reply,
            } => {
                let result =
                    (|| {
                        if let Some(secrets) = &secrets {
                            state.ports.secrets.save(secrets).map_err(|error| {
                                SyncFailure::Failed {
                                    reason: error.to_string(),
                                }
                            })?;
                        }
                        state.ports.settings.save(&settings).map_err(|error| {
                            SyncFailure::Failed {
                                reason: error.to_string(),
                            }
                        })?;
                        state.settings = settings;
                        state.status.availability = Self::availability(state);
                        state.publish();
                        Ok(())
                    })();
                let _ = reply.send(result);
            }
            Message::Enable(reply) => {
                let mut settings = state.settings.clone();
                settings.enabled = true;
                state.settings = settings.clone();
                state.status.availability = Self::availability(state);
                let result = match state.ports.settings.save(&settings) {
                    Ok(()) => Self::cycle(state).await,
                    Err(error) => Err(SyncFailure::Failed {
                        reason: error.to_string(),
                    }),
                };
                if let Err(failure) = &result {
                    state.fail(failure);
                }
                let _ = reply.send(result);
            }
            Message::Disable(reply) => {
                let mut settings = state.settings.clone();
                settings.enabled = false;
                state.settings = settings.clone();
                state.status.availability = Self::availability(state);
                let result =
                    state
                        .ports
                        .settings
                        .save(&settings)
                        .map_err(|error| SyncFailure::Failed {
                            reason: error.to_string(),
                        });
                state.publish();
                let _ = reply.send(result);
            }
            Message::SyncNow(reply) => {
                let result = Self::cycle(state).await;
                if let Err(failure) = &result {
                    state.fail(failure);
                }
                let _ = reply.send(result);
            }
            Message::TestConnection {
                settings,
                secrets,
                reply,
            } => {
                let result = (|| {
                    let adapters = state.adapters.as_ref().ok_or(SyncFailure::Unsupported)?;
                    let secrets = secrets
                        .or_else(|| state.ports.secrets.load().ok().flatten())
                        .unwrap_or_default();
                    let store = open_store(adapters, &settings, &secrets)
                        .map_err(|error| to_failure(&error))?;
                    Ok(store)
                })();
                let result = match result {
                    Ok(store) => {
                        let check = store.check().await;
                        check.map_err(|source| to_failure(&CycleError::Remote { source }))
                    }
                    Err(failure) => Err(failure),
                };
                let _ = reply.send(result);
            }
            Message::ResolveConflict {
                conflict_id,
                choice,
                reply,
            } => {
                let mut found = false;
                for conflict in &mut state.conflicts {
                    if conflict.id == conflict_id {
                        conflict.resolution = Some(choice);
                        found = true;
                    }
                }
                if !found {
                    let failure = SyncFailure::Failed {
                        reason: format!("no open conflict {conflict_id:?}"),
                    };
                    let _ = reply.send(Err(failure));
                    return Ok(());
                }
                state.status.open_conflicts = state
                    .conflicts
                    .iter()
                    .filter(|conflict| conflict.resolution.is_none())
                    .count() as u32;
                if let Err(error) = state.ports.state.save_conflicts(&state.conflicts) {
                    let failure = SyncFailure::Failed {
                        reason: error.to_string(),
                    };
                    state.fail(&failure);
                    let _ = reply.send(Err(failure));
                    return Ok(());
                }
                state.publish();
                let result = Self::cycle(state).await;
                if let Err(failure) = &result {
                    state.fail(failure);
                }
                let _ = reply.send(result);
            }
            Message::Conflicts(reply) => {
                let _ = reply.send(state.conflicts.clone());
            }
            Message::RestoreSnapshot { snapshot_id, reply } => {
                let result = state
                    .ports
                    .snapshots
                    .restore(&snapshot_id)
                    .map_err(|error| SyncFailure::Failed {
                        reason: error.to_string(),
                    });
                let _ = reply.send(result);
            }
        }
        Ok(())
    }
}

fn to_failure(error: &CycleError) -> SyncFailure {
    match error {
        CycleError::Remote { source } => match source {
            RemoteError::Unauthorized { .. } => SyncFailure::Unauthorized,
            RemoteError::Unavailable { .. } => SyncFailure::Unreachable,
            RemoteError::Unsupported { .. } => SyncFailure::Unsupported,
            other => SyncFailure::Failed {
                reason: other.to_string(),
            },
        },
        CycleError::VaultTooNew { found, supported } => SyncFailure::VaultTooNew {
            found: *found,
            supported: *supported,
        },
        other => SyncFailure::Failed {
            reason: other.to_string(),
        },
    }
}

fn to_error(failure: &SyncFailure) -> SyncError {
    match failure {
        SyncFailure::Unauthorized => SyncError::Unauthorized,
        SyncFailure::Unreachable | SyncFailure::Unsupported | SyncFailure::NotConfigured => {
            SyncError::Unreachable
        }
        SyncFailure::VaultTooNew { found, supported } => SyncError::VaultTooNew {
            found: *found,
            supported: *supported,
        },
        SyncFailure::Failed { reason } => SyncError::Storage {
            reason: reason.clone(),
        },
    }
}
