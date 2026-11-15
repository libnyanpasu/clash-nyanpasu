//! Remote-access settings, the HTTP listener that follows them, and the authentication owner.
//!
//! Two owners live here. `RemoteAccessActor` writes the settings: a change is committed first;
//! the listener is then configured from what is committed, and the port it bound is recorded
//! when the settings left it open. Everything after the commit reports as a degradation on the
//! outcome.
//!
//! [`RemoteAccessAuthClient`] fronts `RemoteAccessAuthActor`, the serial owner of named access
//! tokens, 60-second single-use launch codes and idle-expiring browser sessions. Only token
//! hashes are persisted, through the injected [`SecretStore`]. Revoking or expiring a token or
//! session is announced on [`RemoteAccessAuthClient::subscribe_revocations`] so transports can
//! end the streams of that principal.

mod actor;
mod auth_actor;
mod auth_error;
mod auth_store;
mod error;

#[cfg(test)]
mod auth_tests;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use nyanpasu_config::application::RemoteAccessConfig;
use nyanpasu_contracts::auth::{
    AccessTokenSummary, AuthClock, AuthRevocation, AuthenticatedCaller, BrowserSessionSummary,
    CreateAccessTokenRequest, EntropySource, EstablishedBrowserSession, IssuedAccessToken,
    IssuedLaunchCode, RemoteAuthPolicy, ScopeSet, SecretStore, SecretString, SessionId, TokenId,
};
use nyanpasu_runtime_values::runtime::MutationOutcome;
use ractor::{Actor, ActorRef, rpc::CallResult};
use tokio::sync::broadcast;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub(super) use self::actor::Args as RemoteAccessArgs;
use self::{
    actor::{Message, RemoteAccessActor},
    auth_actor::{
        Args as AuthArgs, Message as AuthMessage, RemoteAccessAuthActor, Reply as AuthReply,
    },
    auth_store::load_tokens,
};
pub use self::{
    auth_error::RemoteAuthError,
    auth_store::MAX_SECRET_RECORD_BYTES,
    error::{LaunchUrlError, SetRemoteAccessError},
};
use super::{
    application::ApplicationClient,
    http_server::{HttpServerControl, HttpServerControlError, HttpServerStatus},
    local_socket::{LocalSocketControl, LocalSocketControlError, LocalSocketStatus},
};

const REVOCATION_CHANNEL_CAPACITY: usize = 64;

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
    local_socket: Option<Arc<dyn LocalSocketControl>>,
}

impl RemoteAccessClient {
    /// The actor stops the listeners in `post_stop` once `args.shutdown` is cancelled, after
    /// the changes already queued; `tasks` waits for that.
    pub(super) async fn spawn(
        args: RemoteAccessArgs,
        tasks: &TaskTracker,
    ) -> std::result::Result<Self, Error> {
        let shutdown = args.shutdown.clone();
        let (application, http, local_socket) = (
            args.application.clone(),
            args.http.clone(),
            args.local_socket.clone(),
        );
        let (actor, _handle) = Actor::spawn(None, RemoteAccessActor, args)
            .await
            .map_err(|source| Error::SpawnActor { source })?;
        nyanpasu_owner_support::shutdown::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self {
            actor,
            application,
            http,
            local_socket,
        })
    }

    /// The committed settings.
    pub(super) fn config(&self) -> RemoteAccessConfig {
        self.application.snapshot().state.remote_access
    }

    /// Commits `config`, then makes the listeners follow it.
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

    /// Makes the listeners follow the committed settings, as at startup.
    ///
    /// # Errors
    /// Returns an error when the host has no listeners or the owner stopped.
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

    pub(super) async fn local_socket_status(
        &self,
    ) -> Result<LocalSocketStatus, LocalSocketControlError> {
        let local_socket = self
            .local_socket
            .as_ref()
            .ok_or(LocalSocketControlError::ReadStatusUnsupported)?;
        local_socket
            .status()
            .await
            .map_err(|source| LocalSocketControlError::ReadStatus { source })
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

/// The injected infrastructure of the authentication owner.
#[derive(Clone)]
pub struct RemoteAccessAuthPorts {
    /// Wall and monotonic time.
    pub clock: Arc<dyn AuthClock>,
    /// Cryptographic randomness.
    pub entropy: Arc<dyn EntropySource>,
    /// Durable owner-only record of the token hashes.
    pub store: Arc<dyn SecretStore>,
    /// Bounds and lifetimes.
    pub policy: RemoteAuthPolicy,
}

impl RemoteAccessAuthPorts {
    /// Assembles the authentication infrastructure with the default policy.
    #[must_use]
    pub fn new(
        clock: impl AuthClock + 'static,
        entropy: impl EntropySource + 'static,
        store: impl SecretStore + 'static,
    ) -> Self {
        Self {
            clock: Arc::new(clock),
            entropy: Arc::new(entropy),
            store: Arc::new(store),
            policy: RemoteAuthPolicy::default(),
        }
    }
}

/// Typed client of the authentication owner.
///
/// Every operation waits for the owner's real answer; dropping the future does not cancel work
/// the owner already accepted.
#[derive(Clone)]
pub struct RemoteAccessAuthClient {
    actor: ActorRef<AuthMessage>,
    revocations: broadcast::Sender<AuthRevocation>,
}

impl RemoteAccessAuthClient {
    /// Loads the persisted tokens and starts the owner, which stops with `shutdown`.
    ///
    /// # Errors
    /// Fails closed when the policy is invalid or the persisted record cannot be read, is
    /// corrupt, or exceeds the policy bounds.
    pub async fn spawn(
        ports: RemoteAccessAuthPorts,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Result<Self, RemoteAuthError> {
        if !ports.policy.is_valid() {
            return Err(RemoteAuthError::InvalidInput {
                reason: "policy bounds and lifetimes must be at least 1",
            });
        }
        let tokens = load_tokens(ports.store.as_ref(), ports.policy.max_tokens).await?;
        let (revocations, _) = broadcast::channel(REVOCATION_CHANNEL_CAPACITY);
        let args = AuthArgs {
            clock: ports.clock,
            entropy: ports.entropy,
            store: ports.store,
            policy: ports.policy,
            tokens,
            revocations: revocations.clone(),
        };
        let (actor, _handle) = Actor::spawn(None, RemoteAccessAuthActor, args)
            .await
            .map_err(|_| RemoteAuthError::OwnerStopped)?;
        nyanpasu_owner_support::shutdown::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self { actor, revocations })
    }

    /// Revocations and expiries of tokens and sessions, as they happen.
    ///
    /// A receiver that lags loses events; treat that as "revalidate everything".
    #[must_use]
    pub fn subscribe_revocations(&self) -> broadcast::Receiver<AuthRevocation> {
        self.revocations.subscribe()
    }

    /// Mints a named token. The raw secret is returned once and never stored.
    ///
    /// # Errors
    /// `CapacityExceeded` at the token limit, `InvalidInput` for an expiry in the past, and
    /// `PersistenceFailed` when the token could not be made durable (nothing is created).
    pub async fn create_access_token(
        &self,
        request: CreateAccessTokenRequest,
    ) -> Result<IssuedAccessToken, RemoteAuthError> {
        self.call(|reply| AuthMessage::CreateAccessToken { request, reply })
            .await
    }

    /// Every token's non-secret metadata.
    pub async fn list_access_tokens(&self) -> Result<Vec<AccessTokenSummary>, RemoteAuthError> {
        self.call(|reply| AuthMessage::ListAccessTokens { reply })
            .await
    }

    /// Revokes a token and closes the browser sessions it backs.
    ///
    /// # Errors
    /// `NotFound` for an unknown token. `PersistenceFailed` means the token is revoked in
    /// memory but its removal is not yet durable.
    pub async fn revoke_access_token(&self, id: TokenId) -> Result<(), RemoteAuthError> {
        self.call(|reply| AuthMessage::RevokeAccessToken { id, reply })
            .await
    }

    /// Verifies a bearer token.
    ///
    /// # Errors
    /// `Unauthorized` for any malformed, unknown, revoked or expired token.
    pub async fn authenticate_token(
        &self,
        token: SecretString,
    ) -> Result<AuthenticatedCaller, RemoteAuthError> {
        self.call(|reply| AuthMessage::AuthenticateToken { token, reply })
            .await
    }

    /// Mints a single-use code that opens a browser session holding `scopes`.
    pub async fn issue_launch_code(
        &self,
        scopes: ScopeSet,
    ) -> Result<IssuedLaunchCode, RemoteAuthError> {
        self.call(|reply| AuthMessage::IssueLaunchCode { scopes, reply })
            .await
    }

    /// Spends a launch code for a browser session.
    ///
    /// # Errors
    /// `Unauthorized` when the code is unknown, already spent or past its 60 seconds.
    pub async fn exchange_launch_code(
        &self,
        code: SecretString,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        self.call(|reply| AuthMessage::ExchangeLaunchCode { code, reply })
            .await
    }

    /// Opens a browser session holding a token's scopes. It ends when the token does.
    pub async fn login_with_token(
        &self,
        token: SecretString,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        self.call(|reply| AuthMessage::LoginWithToken { token, reply })
            .await
    }

    /// Verifies a session cookie and extends its idle deadline.
    ///
    /// # Errors
    /// `Unauthorized` for an unknown, closed or idle-expired session.
    pub async fn resolve_browser_session(
        &self,
        cookie: SecretString,
    ) -> Result<AuthenticatedCaller, RemoteAuthError> {
        self.call(|reply| AuthMessage::ResolveBrowserSession { cookie, reply })
            .await
    }

    /// Every open browser session's non-secret metadata.
    pub async fn list_browser_sessions(
        &self,
    ) -> Result<Vec<BrowserSessionSummary>, RemoteAuthError> {
        self.call(|reply| AuthMessage::ListBrowserSessions { reply })
            .await
    }

    /// Closes a session by its public id.
    ///
    /// # Errors
    /// `NotFound` for an unknown session.
    pub async fn revoke_browser_session(&self, id: SessionId) -> Result<(), RemoteAuthError> {
        self.call(|reply| AuthMessage::RevokeBrowserSession { id, reply })
            .await
    }

    /// Closes the session a cookie names.
    ///
    /// # Errors
    /// `Unauthorized` when the cookie does not name an open session.
    pub async fn logout(&self, cookie: SecretString) -> Result<(), RemoteAuthError> {
        self.call(|reply| AuthMessage::Logout { cookie, reply })
            .await
    }

    async fn call<T: Send + 'static>(
        &self,
        make: impl FnOnce(AuthReply<T>) -> AuthMessage,
    ) -> Result<T, RemoteAuthError> {
        match self.actor.call(make, None).await {
            Ok(CallResult::Success(result)) => result,
            _ => Err(RemoteAuthError::OwnerStopped),
        }
    }
}
