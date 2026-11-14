//! HTTP authentication port, late-bound seam, scope clamping, and stream revocation registry.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use nyanpasu_contracts::auth::{
    AuthRevocation, AuthenticatedCaller, EstablishedBrowserSession, Principal, Scope, ScopeSet,
    SecretString, SessionId, TokenId,
};
use nyanpasu_core::client::{NyanpasuClient, RemoteAuthError, WeakNyanpasuClient};
use parking_lot::Mutex;
use snafu::Snafu;
use tokio::sync::{Notify, broadcast};
use tokio_util::sync::CancellationToken;

use crate::registry::SetupError;

/// Error returned by an [`HttpAuthenticator`] operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Snafu)]
pub enum HttpAuthError {
    #[snafu(display("authentication required"))]
    Unauthorized,
    #[snafu(display("authentication service unavailable"))]
    Unavailable,
}

/// Consumer-owned authentication port used by the HTTP transport middleware and login endpoints.
#[async_trait::async_trait]
pub trait HttpAuthenticator: Send + Sync + 'static {
    async fn authenticate_token(
        &self,
        token: SecretString,
    ) -> Result<AuthenticatedCaller, HttpAuthError>;

    async fn exchange_launch_code(
        &self,
        code: SecretString,
    ) -> Result<EstablishedBrowserSession, HttpAuthError>;

    async fn login_with_token(
        &self,
        token: SecretString,
    ) -> Result<EstablishedBrowserSession, HttpAuthError>;

    async fn resolve_browser_session(
        &self,
        cookie: SecretString,
    ) -> Result<AuthenticatedCaller, HttpAuthError>;

    async fn logout(&self, cookie: SecretString) -> Result<(), HttpAuthError>;

    async fn subscribe_revocations(
        &self,
    ) -> Result<broadcast::Receiver<AuthRevocation>, HttpAuthError>;
}

/// Weak-reference [`HttpAuthenticator`] adapter backed by [`NyanpasuClient`].
pub struct NyanpasuClientHttpAuthenticator(WeakNyanpasuClient);

impl NyanpasuClientHttpAuthenticator {
    /// Construct an HTTP authenticator referencing `client` without forming a strong cycle.
    #[must_use]
    pub fn new(client: &NyanpasuClient) -> Self {
        Self(client.downgrade())
    }

    fn upgrade(&self) -> Result<NyanpasuClient, HttpAuthError> {
        self.0.upgrade().ok_or(HttpAuthError::Unavailable)
    }
}

fn map_remote_auth_error(error: RemoteAuthError) -> HttpAuthError {
    match error {
        RemoteAuthError::Unauthorized => HttpAuthError::Unauthorized,
        other => {
            tracing::warn!(
                target: "nyanpasu::remote_access",
                error = %other,
                "remote authentication backend unavailable"
            );
            HttpAuthError::Unavailable
        }
    }
}

#[async_trait::async_trait]
impl HttpAuthenticator for NyanpasuClientHttpAuthenticator {
    async fn authenticate_token(
        &self,
        token: SecretString,
    ) -> Result<AuthenticatedCaller, HttpAuthError> {
        self.upgrade()?
            .authenticate_remote_token(token)
            .await
            .map_err(map_remote_auth_error)
    }

    async fn exchange_launch_code(
        &self,
        code: SecretString,
    ) -> Result<EstablishedBrowserSession, HttpAuthError> {
        self.upgrade()?
            .exchange_launch_code(code)
            .await
            .map_err(map_remote_auth_error)
    }

    async fn login_with_token(
        &self,
        token: SecretString,
    ) -> Result<EstablishedBrowserSession, HttpAuthError> {
        self.upgrade()?
            .login_with_remote_token(token)
            .await
            .map_err(map_remote_auth_error)
    }

    async fn resolve_browser_session(
        &self,
        cookie: SecretString,
    ) -> Result<AuthenticatedCaller, HttpAuthError> {
        self.upgrade()?
            .resolve_browser_session(cookie)
            .await
            .map_err(map_remote_auth_error)
    }

    async fn logout(&self, cookie: SecretString) -> Result<(), HttpAuthError> {
        self.upgrade()?
            .logout_browser_session(cookie)
            .await
            .map_err(map_remote_auth_error)
    }

    async fn subscribe_revocations(
        &self,
    ) -> Result<broadcast::Receiver<AuthRevocation>, HttpAuthError> {
        Ok(self.upgrade()?.subscribe_auth_revocations())
    }
}

/// Filled once by the composition root after [`NyanpasuClient`] exists.
/// Mirrors [`crate::http::routes::RpcHttpRoutes`] so the HTTP server actor can be wired before
/// the client facade is fully constructed without creating a strong reference cycle.
#[derive(Default)]
pub struct RpcHttpAuthenticator {
    inner: OnceLock<Arc<dyn HttpAuthenticator>>,
    installed: Notify,
}

impl RpcHttpAuthenticator {
    /// Install the concrete authenticator once at composition time.
    pub fn install(&self, authenticator: Arc<dyn HttpAuthenticator>) -> Result<(), SetupError> {
        self.inner
            .set(authenticator)
            .map_err(|_| SetupError::InstallAuthenticator)?;
        self.installed.notify_waiters();
        Ok(())
    }

    fn get(&self) -> Result<&Arc<dyn HttpAuthenticator>, HttpAuthError> {
        self.inner.get().ok_or(HttpAuthError::Unavailable)
    }
}

#[async_trait::async_trait]
impl HttpAuthenticator for RpcHttpAuthenticator {
    async fn authenticate_token(
        &self,
        token: SecretString,
    ) -> Result<AuthenticatedCaller, HttpAuthError> {
        self.get()?.authenticate_token(token).await
    }

    async fn exchange_launch_code(
        &self,
        code: SecretString,
    ) -> Result<EstablishedBrowserSession, HttpAuthError> {
        self.get()?.exchange_launch_code(code).await
    }

    async fn login_with_token(
        &self,
        token: SecretString,
    ) -> Result<EstablishedBrowserSession, HttpAuthError> {
        self.get()?.login_with_token(token).await
    }

    async fn resolve_browser_session(
        &self,
        cookie: SecretString,
    ) -> Result<AuthenticatedCaller, HttpAuthError> {
        self.get()?.resolve_browser_session(cookie).await
    }

    async fn logout(&self, cookie: SecretString) -> Result<(), HttpAuthError> {
        self.get()?.logout(cookie).await
    }

    async fn subscribe_revocations(
        &self,
    ) -> Result<broadcast::Receiver<AuthRevocation>, HttpAuthError> {
        loop {
            let notified = self.installed.notified();
            if let Some(auth) = self.inner.get() {
                return auth.subscribe_revocations().await;
            }
            notified.await;
        }
    }
}

/// Clamp `local_open` anonymous scopes so `sensitive_read`, `system`, and `remote_admin`
/// are never granted to unauthenticated callers regardless of persisted settings.
#[must_use]
pub fn clamp_local_open_scopes(scopes: &[Scope]) -> ScopeSet {
    scopes
        .iter()
        .copied()
        .filter(|scope| {
            !matches!(
                scope,
                Scope::SensitiveRead | Scope::System | Scope::RemoteAdmin
            )
        })
        .collect()
}

#[derive(Clone, PartialEq, Eq)]
enum StreamKey {
    Token(TokenId),
    Session(SessionId),
}

/// Active HTTP streaming connections tracked by authenticated principal so token/session
/// revocations immediately close open SSE/WebSocket streams for that principal.
#[derive(Clone, Default)]
pub struct PrincipalStreams(Arc<Mutex<PrincipalStreamsInner>>);

#[derive(Default)]
struct PrincipalStreamsInner {
    next_id: u64,
    by_token: HashMap<TokenId, Vec<(u64, CancellationToken)>>,
    by_session: HashMap<SessionId, Vec<(u64, CancellationToken)>>,
}

/// Deregisters a finished stream entry when its response body drops.
pub struct StreamGuard {
    streams: PrincipalStreams,
    key: StreamKey,
    id: u64,
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        let mut inner = self.streams.0.lock();
        match &self.key {
            StreamKey::Token(token_id) => {
                if let Some(entries) = inner.by_token.get_mut(token_id) {
                    entries.retain(|(id, _)| *id != self.id);
                    if entries.is_empty() {
                        inner.by_token.remove(token_id);
                    }
                }
            }
            StreamKey::Session(session_id) => {
                if let Some(entries) = inner.by_session.get_mut(session_id) {
                    entries.retain(|(id, _)| *id != self.id);
                    if entries.is_empty() {
                        inner.by_session.remove(session_id);
                    }
                }
            }
        }
    }
}

impl PrincipalStreams {
    /// Register a request/stream cancellation token under `principal` as a child of `parent`.
    #[must_use]
    pub fn register(
        &self,
        principal: &Principal,
        parent: &CancellationToken,
    ) -> (CancellationToken, Option<StreamGuard>) {
        let token = parent.child_token();
        let key = match principal {
            Principal::Token { id, .. } => TokenId::parse(id.clone()).ok().map(StreamKey::Token),
            Principal::Session { id } => SessionId::parse(id.clone()).ok().map(StreamKey::Session),
            Principal::Anonymous | Principal::Desktop { .. } | Principal::LocalSocket { .. } => {
                None
            }
        };
        let Some(key) = key else {
            return (token, None);
        };
        let mut inner = self.0.lock();
        inner.next_id = inner.next_id.wrapping_add(1);
        let id = inner.next_id;
        match &key {
            StreamKey::Token(token_id) => {
                inner
                    .by_token
                    .entry(token_id.clone())
                    .or_default()
                    .push((id, token.clone()));
            }
            StreamKey::Session(session_id) => {
                inner
                    .by_session
                    .entry(session_id.clone())
                    .or_default()
                    .push((id, token.clone()));
            }
        }
        (
            token,
            Some(StreamGuard {
                streams: self.clone(),
                key,
                id,
            }),
        )
    }

    /// Apply an [`AuthRevocation`] notice, cancelling every stream owned by the revoked principal.
    pub fn handle_revocation(&self, revocation: &AuthRevocation) {
        let mut cancelled = Vec::new();
        {
            let mut inner = self.0.lock();
            match revocation {
                AuthRevocation::Token {
                    token_id,
                    session_ids,
                } => {
                    if let Some(entries) = inner.by_token.remove(token_id) {
                        cancelled.extend(entries.into_iter().map(|(_, token)| token));
                    }
                    for session_id in session_ids {
                        if let Some(entries) = inner.by_session.remove(session_id) {
                            cancelled.extend(entries.into_iter().map(|(_, token)| token));
                        }
                    }
                }
                AuthRevocation::Session { session_id } => {
                    if let Some(entries) = inner.by_session.remove(session_id) {
                        cancelled.extend(entries.into_iter().map(|(_, token)| token));
                    }
                }
            }
        }
        for token in cancelled {
            token.cancel();
        }
    }

    /// Cancel every active token/session stream (used when the revocation broadcast lags or closes).
    pub fn cancel_all(&self) {
        let mut cancelled = Vec::new();
        {
            let mut inner = self.0.lock();
            for (_, entries) in inner.by_token.drain() {
                cancelled.extend(entries.into_iter().map(|(_, token)| token));
            }
            for (_, entries) in inner.by_session.drain() {
                cancelled.extend(entries.into_iter().map(|(_, token)| token));
            }
        }
        for token in cancelled {
            token.cancel();
        }
    }

    /// Forward `authenticator` revocation events into this registry until `shutdown` fires.
    pub async fn watch(
        self,
        authenticator: Arc<dyn HttpAuthenticator>,
        shutdown: CancellationToken,
    ) {
        let receiver = tokio::select! {
            () = shutdown.cancelled() => return,
            result = authenticator.subscribe_revocations() => match result {
                Ok(receiver) => receiver,
                Err(_) => {
                    self.cancel_all();
                    return;
                }
            },
        };
        self.drive_receiver(receiver, shutdown).await;
    }

    /// Process events from an explicit `broadcast::Receiver<AuthRevocation>` until `shutdown`.
    pub async fn drive_receiver(
        self,
        mut receiver: broadcast::Receiver<AuthRevocation>,
        shutdown: CancellationToken,
    ) {
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                item = receiver.recv() => match item {
                    Ok(revocation) => self.handle_revocation(&revocation),
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(
                            target: "nyanpasu::remote_access",
                            skipped,
                            "auth revocation receiver lagged; closing all active principal streams"
                        );
                        self.cancel_all();
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        self.cancel_all();
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nyanpasu_contracts::auth::{AuthRevocation, Principal, Scope, SessionId, TokenId};
    use parking_lot::Mutex;
    use tokio::sync::broadcast;
    use tokio_util::sync::CancellationToken;
    use tracing::{Level, Subscriber};
    use tracing_subscriber::{
        Layer,
        layer::{Context, SubscriberExt as _},
    };

    use super::*;

    #[derive(Clone, Default)]
    struct RemoteAccessWarnCapture(Arc<Mutex<Vec<(Level, BTreeMap<String, String>)>>>);

    struct Visitor(BTreeMap<String, String>);

    impl tracing::field::Visit for Visitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.insert(field.name().to_owned(), format!("{value:?}"));
        }
    }

    impl<S: Subscriber> Layer<S> for RemoteAccessWarnCapture {
        fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
            if event.metadata().target() == "nyanpasu::remote_access" {
                let mut visitor = Visitor(BTreeMap::new());
                event.record(&mut visitor);
                self.0.lock().push((*event.metadata().level(), visitor.0));
            }
        }
    }

    #[test]
    fn clamp_local_open_scopes_strips_privileged_scopes() {
        let clamped = clamp_local_open_scopes(&[
            Scope::Read,
            Scope::SensitiveRead,
            Scope::Proxy,
            Scope::Profile,
            Scope::Settings,
            Scope::System,
            Scope::RemoteAdmin,
        ]);
        assert!(clamped.contains(Scope::Read));
        assert!(clamped.contains(Scope::Proxy));
        assert!(clamped.contains(Scope::Profile));
        assert!(clamped.contains(Scope::Settings));
        assert!(!clamped.contains(Scope::SensitiveRead));
        assert!(!clamped.contains(Scope::System));
        assert!(!clamped.contains(Scope::RemoteAdmin));
    }

    #[tokio::test]
    async fn principal_streams_cancels_matching_revocations_and_lag_cancels_all() {
        let streams = PrincipalStreams::default();
        let parent = CancellationToken::new();

        let token_principal = Principal::Token {
            id: "tok_01".to_owned(),
            name: "cli".to_owned(),
        };
        let derived_session = Principal::Session {
            id: "sess_derived".to_owned(),
        };
        let independent_session = Principal::Session {
            id: "sess_other".to_owned(),
        };

        let (tok_cancel, _tok_guard) = streams.register(&token_principal, &parent);
        let (derived_cancel, _derived_guard) = streams.register(&derived_session, &parent);
        let (other_cancel, other_guard) = streams.register(&independent_session, &parent);
        let (anon_cancel, anon_guard) = streams.register(&Principal::Anonymous, &parent);
        assert!(anon_guard.is_none());

        // Single-session revocation touches only that session.
        streams.handle_revocation(&AuthRevocation::Session {
            session_id: SessionId::parse("sess_other").unwrap(),
        });
        assert!(other_cancel.is_cancelled());
        assert!(!tok_cancel.is_cancelled());
        assert!(!derived_cancel.is_cancelled());
        assert!(!anon_cancel.is_cancelled());
        drop(other_guard);

        // Re-register an unrelated session; token revocation cancels the token and its derived
        // session without touching the unrelated session.
        let (unrelated_cancel, _unrelated_guard) = streams.register(&independent_session, &parent);
        streams.handle_revocation(&AuthRevocation::Token {
            token_id: TokenId::parse("tok_01").unwrap(),
            session_ids: vec![SessionId::parse("sess_derived").unwrap()],
        });
        assert!(tok_cancel.is_cancelled());
        assert!(derived_cancel.is_cancelled());
        assert!(!unrelated_cancel.is_cancelled());

        // Broadcast lag cancels all remaining tracked streams and emits a WARN log.
        let (tx, rx) = broadcast::channel(1);
        tx.send(AuthRevocation::Session {
            session_id: SessionId::parse("sess_x").unwrap(),
        })
        .unwrap();
        tx.send(AuthRevocation::Session {
            session_id: SessionId::parse("sess_y").unwrap(),
        })
        .unwrap();
        let stop = CancellationToken::new();
        let capture = RemoteAccessWarnCapture::default();
        let _sub =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(capture.clone()));
        let watcher = tokio::spawn({
            let streams = streams.clone();
            let stop = stop.clone();
            async move { streams.drive_receiver(rx, stop).await }
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            unrelated_cancel.cancelled(),
        )
        .await
        .expect("lagged revocation receiver must cancel remaining streams");
        stop.cancel();
        watcher.await.unwrap();

        let logs = capture.0.lock();
        assert!(logs.iter().any(|(level, _)| *level == Level::WARN));
    }
}
