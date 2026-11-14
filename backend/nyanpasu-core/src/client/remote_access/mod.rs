//! Remote-access settings, HTTP listener lifecycle, and authentication owners.
//!
//! The settings are written only here. A change is committed first; the listener is then
//! configured from what is committed, and the port it bound is recorded when the settings left
//! it open. Everything after the commit reports as a degradation on the outcome.
//!
//! Credential verification, sessions, and resource leases live in an explicitly owned sibling
//! `RemoteAccessAuthActor` so HTTP drain/reconfiguration waits on `RemoteAccessActor` never
//! stall or deadlock admission and revocation paths.

mod actor;
mod auth_actor;
mod auth_store;
mod error;
mod rate_limit;

#[cfg(test)]
mod auth_tests;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use nyanpasu_config::application::RemoteAccessConfig;
use nyanpasu_contracts::auth::{
    AccessTokenSummary, AuthContext, AuthPeer, AuthRevocation, AuthenticatedCaller,
    BrowserSessionSummary, CreateAccessTokenRequest, EstablishedBrowserSession,
    EstablishedResourceSession, IssuedAccessToken, IssuedLaunchCode, ResolvedResourceSession,
    ResourceSessionId, ScopeSet, SecretString, SessionId, TlsMaterialBlob, TlsMaterialSummary,
    TokenId,
};
use nyanpasu_runtime_values::runtime::MutationOutcome;
use ractor::{Actor, ActorRef, rpc::CallResult};
use tokio::sync::broadcast;
use tokio_util::task::TaskTracker;

pub(super) use self::actor::Args as RemoteAccessArgs;
use self::{
    actor::{Message, RemoteAccessActor},
    auth_actor::RemoteAccessAuthClient,
};
pub use self::{
    auth_actor::{
        DEFAULT_BROWSER_SESSION_IDLE_MS, DEFAULT_LAUNCH_CODE_TTL_MS,
        DEFAULT_RESOURCE_SESSION_IDLE_MS, DEFAULT_WALL_EXPIRY_RECHECK_MS, RemoteAccessAuthPorts,
        RemoteAuthPolicy,
    },
    error::{RemoteAuthError, SetRemoteAccessError},
    rate_limit::PeerRateLimitConfig,
};
use super::{
    application::ApplicationClient,
    http_server::{HttpServerControl, HttpServerControlError, HttpServerStatus},
};

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("failed to spawn actor"))]
    SpawnActor { source: ractor::SpawnErr },
    #[snafu(display("failed to initialize remote authentication owner: {source}"))]
    SpawnAuthOwner { source: RemoteAuthError },
}

#[derive(Clone)]
pub(super) struct RemoteAccessClient {
    actor: ActorRef<Message>,
    auth: Option<RemoteAccessAuthClient>,
    application: ApplicationClient,
    http: Option<Arc<dyn HttpServerControl>>,
}

impl RemoteAccessClient {
    /// The actor stops the listener in `post_stop` once `args.shutdown` is cancelled, after
    /// the changes already queued; `tasks` waits for that.
    pub(super) async fn spawn(
        args: RemoteAccessArgs,
        remote_auth: Option<RemoteAccessAuthPorts>,
        tasks: &TaskTracker,
    ) -> std::result::Result<Self, Error> {
        let shutdown = args.shutdown.clone();
        let (application, http) = (args.application.clone(), args.http.clone());
        let auth = match remote_auth {
            Some(ports) => Some(
                RemoteAccessAuthClient::spawn(ports, shutdown.child_token(), tasks)
                    .await
                    .map_err(|source| Error::SpawnAuthOwner { source })?,
            ),
            None => None,
        };
        let (actor, _handle) = Actor::spawn(None, RemoteAccessActor, args)
            .await
            .map_err(|source| Error::SpawnActor { source })?;
        nyanpasu_owner_support::shutdown::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self {
            actor,
            auth,
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

    fn require_auth(&self) -> Result<&RemoteAccessAuthClient, RemoteAuthError> {
        self.auth.as_ref().ok_or(RemoteAuthError::NotConfigured)
    }

    pub(super) async fn create_access_token(
        &self,
        request: CreateAccessTokenRequest,
    ) -> Result<IssuedAccessToken, RemoteAuthError> {
        self.require_auth()?.create_access_token(request).await
    }

    pub(super) async fn list_access_tokens(
        &self,
    ) -> Result<Vec<AccessTokenSummary>, RemoteAuthError> {
        self.require_auth()?.list_access_tokens().await
    }

    pub(super) async fn revoke_access_token(&self, id: TokenId) -> Result<(), RemoteAuthError> {
        self.require_auth()?.revoke_access_token(id).await
    }

    pub(super) async fn authenticate_token(
        &self,
        peer: AuthPeer,
        token: SecretString,
    ) -> Result<AuthenticatedCaller, RemoteAuthError> {
        self.require_auth()?.authenticate_token(peer, token).await
    }

    pub(super) async fn issue_launch_code(
        &self,
        scopes: ScopeSet,
    ) -> Result<IssuedLaunchCode, RemoteAuthError> {
        self.require_auth()?.issue_launch_code(scopes).await
    }

    pub(super) async fn exchange_launch_code(
        &self,
        peer: AuthPeer,
        code: SecretString,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        self.require_auth()?.exchange_launch_code(peer, code).await
    }

    pub(super) async fn login_with_token(
        &self,
        peer: AuthPeer,
        token: SecretString,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        self.require_auth()?.login_with_token(peer, token).await
    }

    pub(super) async fn resolve_browser_session(
        &self,
        peer: AuthPeer,
        cookie: SecretString,
    ) -> Result<AuthenticatedCaller, RemoteAuthError> {
        self.require_auth()?
            .resolve_browser_session(peer, cookie)
            .await
    }

    pub(super) async fn list_browser_sessions(
        &self,
    ) -> Result<Vec<BrowserSessionSummary>, RemoteAuthError> {
        self.require_auth()?.list_browser_sessions().await
    }

    pub(super) async fn revoke_browser_session(
        &self,
        id: SessionId,
    ) -> Result<(), RemoteAuthError> {
        self.require_auth()?.revoke_browser_session(id).await
    }

    pub(super) async fn admit_caller(
        &self,
        caller: AuthenticatedCaller,
    ) -> Result<AuthenticatedCaller, RemoteAuthError> {
        self.require_auth()?.admit_caller(caller).await
    }

    pub(super) async fn create_resource_session(
        &self,
        caller: AuthContext,
    ) -> Result<EstablishedResourceSession, RemoteAuthError> {
        self.require_auth()?.create_resource_session(caller).await
    }

    pub(super) async fn resolve_resource_session(
        &self,
        caller: AuthContext,
        cookie: SecretString,
    ) -> Result<ResolvedResourceSession, RemoteAuthError> {
        self.require_auth()?
            .resolve_resource_session(caller, cookie)
            .await
    }

    pub(super) async fn release_resource_session(
        &self,
        id: ResourceSessionId,
    ) -> Result<(), RemoteAuthError> {
        self.require_auth()?.release_resource_session(id).await
    }

    pub(super) fn subscribe_revocations(
        &self,
    ) -> Result<broadcast::Receiver<AuthRevocation>, RemoteAuthError> {
        Ok(self.require_auth()?.subscribe_revocations())
    }

    pub(super) async fn set_tls_material(
        &self,
        blob: TlsMaterialBlob,
    ) -> Result<TlsMaterialSummary, RemoteAuthError> {
        self.require_auth()?.set_tls_material(blob).await
    }

    pub(super) async fn tls_material_status(
        &self,
    ) -> Result<Option<TlsMaterialSummary>, RemoteAuthError> {
        self.require_auth()?.tls_material_status().await
    }

    pub(super) async fn load_tls_material(
        &self,
    ) -> Result<Option<TlsMaterialBlob>, RemoteAuthError> {
        self.require_auth()?.load_tls_material().await
    }
}
