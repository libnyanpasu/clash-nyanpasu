//! Remote-access settings and the HTTP listener that follows them, behind one actor.
//!
//! The settings are written only here. A change is committed first; the listener is then
//! configured from what is committed, and the port it bound is recorded when the settings left
//! it open. Everything after the commit reports as a degradation on the outcome.

mod actor;
mod error;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use nyanpasu_config::application::RemoteAccessConfig;
use nyanpasu_runtime_values::runtime::MutationOutcome;
use ractor::{Actor, ActorRef, rpc::CallResult};
use tokio_util::task::TaskTracker;

pub(super) use self::actor::Args as RemoteAccessArgs;
use self::actor::{Message, RemoteAccessActor};
pub use self::error::SetRemoteAccessError;
use super::{
    application::ApplicationClient,
    http_server::{HttpServerControl, HttpServerControlError, HttpServerStatus},
};

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("failed to spawn actor"))]
    SpawnActor { source: ractor::SpawnErr },
}

#[derive(Clone)]
pub(super) struct RemoteAccessClient {
    actor: ActorRef<Message>,
    application: ApplicationClient,
    http: Option<Arc<dyn HttpServerControl>>,
}

impl RemoteAccessClient {
    /// The actor stops the listener in `post_stop` once `args.shutdown` is cancelled, after
    /// the changes already queued; `tasks` waits for that.
    pub(super) async fn spawn(
        args: RemoteAccessArgs,
        tasks: &TaskTracker,
    ) -> std::result::Result<Self, Error> {
        let shutdown = args.shutdown.clone();
        let (application, http) = (args.application.clone(), args.http.clone());
        let (actor, _handle) = Actor::spawn(None, RemoteAccessActor, args)
            .await
            .map_err(|source| Error::SpawnActor { source })?;
        nyanpasu_owner_support::shutdown::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self {
            actor,
            application,
            http,
        })
    }

    /// The committed settings.
    pub(super) fn config(&self) -> RemoteAccessConfig {
        self.application.snapshot().state.remote_access
    }

    /// Commits `config`, then makes the listener follow it.
    ///
    /// The owner runs the whole operation once it accepts it; dropping this future neither
    /// cancels it nor leaves the settings committed without a listener attempt.
    ///
    /// # Errors
    /// Returns an error only when nothing was committed.
    pub(super) async fn set(
        &self,
        config: RemoteAccessConfig,
    ) -> Result<MutationOutcome<()>, SetRemoteAccessError> {
        self.call(|reply| Message::Set { config, reply }).await
    }

    /// Makes the listener follow the committed settings, as at startup.
    ///
    /// # Errors
    /// Returns an error when the host has no HTTP server or the owner stopped.
    pub(super) async fn reconcile(&self) -> Result<MutationOutcome<()>, SetRemoteAccessError> {
        self.call(|reply| Message::Reconcile { reply }).await
    }

    pub(super) async fn status(&self) -> Result<HttpServerStatus, HttpServerControlError> {
        let http = self
            .http
            .as_ref()
            .ok_or(HttpServerControlError::ReadStatusUnsupported)?;
        http.status()
            .await
            .map_err(|source| HttpServerControlError::ReadStatus { source })
    }

    /// A URL that logs the caller into the running server. It is a credential.
    pub(super) async fn launch_url(&self) -> Result<Option<String>, HttpServerControlError> {
        let http = self
            .http
            .as_ref()
            .ok_or(HttpServerControlError::ReadStatusUnsupported)?;
        http.launch_url()
            .await
            .map_err(|source| HttpServerControlError::ReadStatus { source })
    }

    async fn call(
        &self,
        make: impl FnOnce(
            ractor::RpcReplyPort<Result<MutationOutcome<()>, SetRemoteAccessError>>,
        ) -> Message,
    ) -> Result<MutationOutcome<()>, SetRemoteAccessError> {
        match self.actor.call(make, None).await {
            Ok(CallResult::Success(result)) => result,
            _ => Err(SetRemoteAccessError::OwnerStopped),
        }
    }
}
