//! The serial owner of the remote-access settings and the HTTP listener that follows them.
//!
//! It is an actor because one operation spans several awaits (commit the settings, configure
//! the listener, record the port it bound) and must not interleave with another: two concurrent
//! edits would otherwise configure the listener from an older snapshot after a newer commit.
//! The mailbox is the only serialization and the handler runs the whole operation, so a caller
//! that leaves does not cancel it.

use std::sync::Arc;

use nyanpasu_config::application::{NyanpasuAppConfig, RemoteAccessConfig};
use nyanpasu_runtime_values::runtime::{
    Degradation, DegradationPhase, DegradationReason, MutationOutcome,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch as _;
use tokio_util::sync::CancellationToken;

use super::error::SetRemoteAccessError;
use crate::{
    client::{
        application::ApplicationClient,
        http_server::{HttpServerControl, HttpServerControlError},
        local_socket::{LocalSocketControl, LocalSocketControlError},
    },
    state::config_error::ConfigError,
};

type Reply = RpcReplyPort<Result<MutationOutcome<()>, SetRemoteAccessError>>;

pub(super) enum Message {
    /// Commit `config`, then make the listeners follow it.
    Set {
        config: RemoteAccessConfig,
        reply: Reply,
    },
    /// Make the listeners follow the committed settings, as at startup.
    Reconcile { reply: Reply },
}

pub(in crate::client) struct Args {
    pub application: ApplicationClient,
    pub http: Option<Arc<dyn HttpServerControl>>,
    pub local_socket: Option<Arc<dyn LocalSocketControl>>,
    /// Once cancelled, new changes are refused and `post_stop` stops the listeners.
    pub shutdown: CancellationToken,
}

pub(super) struct RemoteAccessActor;

pub(super) struct State {
    application: ApplicationClient,
    http: Option<Arc<dyn HttpServerControl>>,
    local_socket: Option<Arc<dyn LocalSocketControl>>,
    shutdown: CancellationToken,
}

impl State {
    fn ensure_open(&self) -> Result<(), SetRemoteAccessError> {
        if self.shutdown.is_cancelled() {
            return Err(SetRemoteAccessError::OwnerStopped);
        }
        Ok(())
    }

    /// Writes only this section. This actor is its sole writer: the facade's general patch
    /// refuses the section, so reading the snapshot just before a write cannot be stale.
    async fn commit(
        &self,
        config: RemoteAccessConfig,
    ) -> Result<MutationOutcome<()>, SetRemoteAccessError> {
        config.validate().map_err(|reason| {
            crate::client::ClientError::from(ConfigError::InvalidRemoteAccess { reason })
        })?;
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.remote_access = Some(config);
        let snapshot = self
            .application
            .patch(patch)
            .await
            .map_err(crate::client::ClientError::from)?;
        Ok(snapshot.outcome())
    }

    /// Makes the listeners follow the committed settings. Every failure after the commit is a
    /// degradation: the settings stay committed and the outcome says what did not follow.
    async fn follow(
        &self,
        configure_http: bool,
        configure_local: bool,
        mut outcome: MutationOutcome<()>,
    ) -> MutationOutcome<()> {
        let config = self.application.snapshot().state.remote_access;
        if let Err(error) = config.validate() {
            tracing::warn!(%error, "the saved remote access settings are invalid");
            let mut degradations = Vec::new();
            if configure_http && self.http.is_some() {
                degradations.push(http_degradation(
                    "the HTTP server cannot use the saved remote access settings",
                ));
            }
            if configure_local && self.local_socket.is_some() {
                degradations.push(local_socket_degradation(
                    "the local socket cannot use the saved remote access settings",
                ));
            }
            return outcome.extend_degradations(degradations);
        }
        if configure_http && let Some(http) = &self.http {
            outcome = self.follow_http(http, &config, outcome).await;
        }
        if configure_local && let Some(local_socket) = &self.local_socket {
            outcome = self
                .follow_local_socket(local_socket, &config, outcome)
                .await;
        }
        outcome
    }

    async fn follow_http(
        &self,
        http: &Arc<dyn HttpServerControl>,
        config: &RemoteAccessConfig,
        mut outcome: MutationOutcome<()>,
    ) -> MutationOutcome<()> {
        let status = match http.configure(&config.http).await {
            Ok(status) => status,
            Err(error) => {
                tracing::warn!(%error, "the HTTP server could not follow the remote access settings");
                return outcome.extend_degradations(vec![http_degradation(
                    "the HTTP server could not follow the settings",
                )]);
            }
        };
        if let Some(fault) = status.fault {
            return outcome.extend_degradations(vec![http_degradation(format!(
                "the HTTP server is not running: {fault:?}"
            ))]);
        }
        let (true, None, Some(port)) = (config.http.enabled, config.http.port, status.port) else {
            return outcome;
        };
        // Record the port the listener bound, on top of what is committed now.
        let mut recorded = self.application.snapshot().state.remote_access;
        recorded.http.port = Some(port);
        match self.commit(recorded).await {
            Ok(recorded) => outcome = outcome.append_commit_result(recorded),
            Err(error) => {
                tracing::warn!(%error, "failed to record the HTTP port");
                outcome = outcome.extend_degradations(vec![http_degradation(
                    "the HTTP port could not be recorded",
                )]);
            }
        }
        outcome
    }

    async fn follow_local_socket(
        &self,
        local_socket: &Arc<dyn LocalSocketControl>,
        config: &RemoteAccessConfig,
        outcome: MutationOutcome<()>,
    ) -> MutationOutcome<()> {
        let status = match local_socket.configure(&config.local_socket).await {
            Ok(status) => status,
            Err(error) => {
                tracing::warn!(%error, "the local socket could not follow the remote access settings");
                return outcome.extend_degradations(vec![local_socket_degradation(
                    "the local socket could not follow the settings",
                )]);
            }
        };
        if let Some(fault) = status.fault {
            return outcome.extend_degradations(vec![local_socket_degradation(format!(
                "the local socket is not running: {fault:?}"
            ))]);
        }
        outcome
    }
}

fn http_degradation(message: impl Into<String>) -> Degradation {
    Degradation {
        phase: DegradationPhase::SystemEffect,
        reason: DegradationReason::HttpServerUnavailable,
        message: message.into(),
        retryable: true,
    }
}

fn local_socket_degradation(message: impl Into<String>) -> Degradation {
    Degradation {
        phase: DegradationPhase::SystemEffect,
        reason: DegradationReason::LocalSocketUnavailable,
        message: message.into(),
        retryable: true,
    }
}

impl Actor for RemoteAccessActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;

    async fn pre_start(
        &self,
        _: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        Ok(State {
            application: args.application,
            http: args.http,
            local_socket: args.local_socket,
            shutdown: args.shutdown,
        })
    }

    async fn handle(
        &self,
        _: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Set { config, reply } => {
                let result = async {
                    state.ensure_open()?;
                    if state.http.is_none() && state.local_socket.is_none() {
                        return Err(HttpServerControlError::ChangeUnsupported.into());
                    }
                    let current = state.application.snapshot().state.remote_access;
                    let http_changed = config.http != current.http;
                    let local_changed = config.local_socket != current.local_socket;
                    if state.http.is_none() && http_changed {
                        return Err(HttpServerControlError::ChangeUnsupported.into());
                    }
                    if state.local_socket.is_none() && local_changed {
                        return Err(LocalSocketControlError::ChangeUnsupported.into());
                    }
                    let configure_http = state.http.is_some() && (http_changed || !local_changed);
                    let configure_local =
                        state.local_socket.is_some() && (local_changed || !http_changed);
                    let committed = state.commit(config).await?;
                    Ok(state
                        .follow(configure_http, configure_local, committed)
                        .await)
                }
                .await;
                let _ = reply.send(result);
            }
            Message::Reconcile { reply } => {
                let result = async {
                    state.ensure_open()?;
                    if state.http.is_none() && state.local_socket.is_none() {
                        return Err(HttpServerControlError::ChangeUnsupported.into());
                    }
                    Ok(state
                        .follow(
                            state.http.is_some(),
                            state.local_socket.is_some(),
                            MutationOutcome::from_parts((), Vec::new()),
                        )
                        .await)
                }
                .await;
                let _ = reply.send(result);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(http) = &state.http
            && let Err(error) = http.shutdown().await
        {
            tracing::warn!(%error, "HTTP server shutdown failed");
        }
        if let Some(local_socket) = &state.local_socket
            && let Err(error) = local_socket.shutdown().await
        {
            tracing::warn!(%error, "local socket shutdown failed");
        }
        Ok(())
    }
}
