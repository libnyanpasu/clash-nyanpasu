//! Remote-access authentication owner.
//!
//! [`RemoteAccessAuthClient`] fronts `RemoteAccessAuthActor`, the serial owner of named access
//! tokens, 60-second single-use launch codes and idle-expiring browser sessions. Only token
//! hashes are persisted, through the injected [`SecretStore`]. Revoking or expiring a token or
//! session is announced on [`RemoteAccessAuthClient::subscribe_revocations`] so transports can
//! end the streams of that principal.

mod auth_actor;
mod auth_store;
mod error;

#[cfg(test)]
mod auth_tests;

use std::sync::Arc;

use nyanpasu_contracts::auth::{
    AccessTokenSummary, AuthClock, AuthRevocation, AuthenticatedCaller, BrowserSessionSummary,
    CreateAccessTokenRequest, EntropySource, EstablishedBrowserSession, IssuedAccessToken,
    IssuedLaunchCode, RemoteAuthPolicy, ScopeSet, SecretStore, SecretString, SessionId, TokenId,
};
use ractor::{Actor, ActorRef, rpc::CallResult};
use tokio::sync::broadcast;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use self::{
    auth_actor::{Args, Message, RemoteAccessAuthActor, Reply},
    auth_store::load_tokens,
};
pub use self::{auth_store::MAX_SECRET_RECORD_BYTES, error::RemoteAuthError};

const REVOCATION_CHANNEL_CAPACITY: usize = 64;

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

/// Typed client of the authentication owner.
///
/// Every operation waits for the owner's real answer; dropping the future does not cancel work
/// the owner already accepted.
#[derive(Clone)]
pub struct RemoteAccessAuthClient {
    actor: ActorRef<Message>,
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
        let args = Args {
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
        self.call(|reply| Message::CreateAccessToken { request, reply })
            .await
    }

    /// Every token's non-secret metadata.
    pub async fn list_access_tokens(&self) -> Result<Vec<AccessTokenSummary>, RemoteAuthError> {
        self.call(|reply| Message::ListAccessTokens { reply }).await
    }

    /// Revokes a token and closes the browser sessions it backs.
    ///
    /// # Errors
    /// `NotFound` for an unknown token. `PersistenceFailed` means the token is revoked in
    /// memory but its removal is not yet durable.
    pub async fn revoke_access_token(&self, id: TokenId) -> Result<(), RemoteAuthError> {
        self.call(|reply| Message::RevokeAccessToken { id, reply })
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
        self.call(|reply| Message::AuthenticateToken { token, reply })
            .await
    }

    /// Mints a single-use code that opens a browser session holding `scopes`.
    pub async fn issue_launch_code(
        &self,
        scopes: ScopeSet,
    ) -> Result<IssuedLaunchCode, RemoteAuthError> {
        self.call(|reply| Message::IssueLaunchCode { scopes, reply })
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
        self.call(|reply| Message::ExchangeLaunchCode { code, reply })
            .await
    }

    /// Opens a browser session holding a token's scopes. It ends when the token does.
    pub async fn login_with_token(
        &self,
        token: SecretString,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        self.call(|reply| Message::LoginWithToken { token, reply })
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
        self.call(|reply| Message::ResolveBrowserSession { cookie, reply })
            .await
    }

    /// Every open browser session's non-secret metadata.
    pub async fn list_browser_sessions(
        &self,
    ) -> Result<Vec<BrowserSessionSummary>, RemoteAuthError> {
        self.call(|reply| Message::ListBrowserSessions { reply })
            .await
    }

    /// Closes a session by its public id.
    ///
    /// # Errors
    /// `NotFound` for an unknown session.
    pub async fn revoke_browser_session(&self, id: SessionId) -> Result<(), RemoteAuthError> {
        self.call(|reply| Message::RevokeBrowserSession { id, reply })
            .await
    }

    /// Closes the session a cookie names.
    ///
    /// # Errors
    /// `Unauthorized` when the cookie does not name an open session.
    pub async fn logout(&self, cookie: SecretString) -> Result<(), RemoteAuthError> {
        self.call(|reply| Message::Logout { cookie, reply }).await
    }

    async fn call<T: Send + 'static>(
        &self,
        make: impl FnOnce(Reply<T>) -> Message,
    ) -> Result<T, RemoteAuthError> {
        match self.actor.call(make, None).await {
            Ok(CallResult::Success(result)) => result,
            _ => Err(RemoteAuthError::OwnerStopped),
        }
    }
}
