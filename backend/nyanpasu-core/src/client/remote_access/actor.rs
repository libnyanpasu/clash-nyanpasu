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
    },
    state::config_error::ConfigError,
};

type Reply = RpcReplyPort<Result<MutationOutcome<()>, SetRemoteAccessError>>;

pub(super) enum Message {
    /// Commit `config`, then make the listener follow it.
    Set {
        config: RemoteAccessConfig,
        reply: Reply,
    },
    /// Make the listener follow the committed settings, as at startup.
    Reconcile { reply: Reply },
}

pub(in crate::client) struct Args {
    pub application: ApplicationClient,
    pub http: Option<Arc<dyn HttpServerControl>>,
    /// Once cancelled, new changes are refused and `post_stop` stops the listener.
    pub shutdown: CancellationToken,
}

pub(super) struct RemoteAccessActor;

pub(super) struct State {
    application: ApplicationClient,
    http: Option<Arc<dyn HttpServerControl>>,
    shutdown: CancellationToken,
}

impl State {
    fn http(&self) -> Result<&Arc<dyn HttpServerControl>, SetRemoteAccessError> {
        if self.shutdown.is_cancelled() {
            return Err(SetRemoteAccessError::OwnerStopped);
        }
        self.http
            .as_ref()
            .ok_or(HttpServerControlError::ChangeUnsupported.into())
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

    /// Makes the listener follow the committed settings. Every failure after the commit is a
    /// degradation: the settings stay committed and the outcome says what did not follow.
    async fn follow(
        &self,
        http: &Arc<dyn HttpServerControl>,
        mut outcome: MutationOutcome<()>,
    ) -> MutationOutcome<()> {
        let config = self.application.snapshot().state.remote_access;
        if let Err(error) = config.validate() {
            tracing::warn!(%error, "the saved remote access settings are invalid");
            return outcome.extend_degradations(vec![degradation(
                "the HTTP server cannot use the saved remote access settings",
            )]);
        }
        let status = match http.configure(&config.http).await {
            Ok(status) => status,
            Err(error) => {
                tracing::warn!(%error, "the HTTP server could not follow the remote access settings");
                return outcome.extend_degradations(vec![degradation(
                    "the HTTP server could not follow the settings",
                )]);
            }
        };
        if let Some(fault) = status.fault {
            return outcome.extend_degradations(vec![degradation(format!(
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
                outcome = outcome
                    .extend_degradations(vec![degradation("the HTTP port could not be recorded")]);
            }
        }
        outcome
    }
}

fn degradation(message: impl Into<String>) -> Degradation {
    Degradation {
        phase: DegradationPhase::SystemEffect,
        reason: DegradationReason::HttpServerUnavailable,
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
                    let http = state.http()?;
                    let committed = state.commit(config).await?;
                    Ok(state.follow(http, committed).await)
                }
                .await;
                let _ = reply.send(result);
            }
            Message::Reconcile { reply } => {
                let result = async {
                    let http = state.http()?;
                    Ok(state
                        .follow(http, MutationOutcome::from_parts((), Vec::new()))
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
        Ok(())
    }
}
