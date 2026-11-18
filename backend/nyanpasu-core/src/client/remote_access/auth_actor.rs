//! Serial owner of access tokens, single-use launch codes and browser sessions.
//!
//! One mailbox serializes every credential change, so verification, revocation and expiry can
//! never race. Expired items are pruned before each message and by a timer armed for the next
//! deadline, which is what announces idle-expired sessions and expired tokens to subscribers.
//! Tokens are persisted through the injected [`SecretStore`]; launch codes and sessions are
//! memory only and do not survive a restart.

use std::{collections::BTreeMap, net::IpAddr, sync::Arc, time::Duration};

use nyanpasu_config::application::Role;
use nyanpasu_contracts::auth::{
    AccessTokenStatus, AccessTokenSummary, AuthClock, AuthContext, AuthRevocation,
    AuthenticatedCaller, BrowserSessionSummary, CreateAccessTokenRequest, EntropySource,
    EstablishedBrowserSession, Grant, GrantError, IssuedAccessToken, IssuedLaunchCode, Principal,
    RemoteAuthPolicy, SecretHash, SecretStore, SecretString, SessionId, TlsCertificateGenerator,
    TlsCertificateRequest, TlsCertificateSummary, TlsIdentity, TokenId,
    is_valid_launch_code_syntax, parse_access_token, parse_session_cookie,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, MessagingErr, RpcReplyPort};
use tokio::{sync::broadcast, task::JoinHandle};

use super::{
    auth_error::RemoteAuthError,
    auth_store::{
        TokenRecord, constant_time_hash_eq, mint_launch_code, mint_session_cookie, mint_token,
        save_record, sha256_secret,
    },
};

/// Pending launch codes kept at once. They live 60 seconds, so this only bounds a flood.
const MAX_LAUNCH_CODES: usize = 32;
const MAX_WAKE_DELAY_MS: u64 = 24 * 60 * 60 * 1000;
/// How long a generated certificate stays valid: two years, which stays inside the 825-day
/// limit some platforms apply to server certificates.
const TLS_VALIDITY_MS: i64 = 730 * 24 * 60 * 60 * 1000;
/// A certificate is valid from this long before it was made. A device on the network whose clock
/// runs behind this computer's would otherwise reject a certificate that is not yet valid for it.
const TLS_CLOCK_SKEW_MS: i64 = 24 * 60 * 60 * 1000;

pub(super) type Reply<T> = RpcReplyPort<Result<T, RemoteAuthError>>;

pub(super) enum Message {
    CreateAccessToken {
        request: CreateAccessTokenRequest,
        reply: Reply<IssuedAccessToken>,
    },
    ListAccessTokens {
        reply: Reply<Vec<AccessTokenSummary>>,
    },
    RevokeAccessToken {
        id: TokenId,
        reply: Reply<()>,
    },
    AuthenticateToken {
        token: SecretString,
        reply: Reply<AuthenticatedCaller>,
    },
    IssueLaunchCode {
        reply: Reply<IssuedLaunchCode>,
    },
    ExchangeLaunchCode {
        code: SecretString,
        reply: Reply<EstablishedBrowserSession>,
    },
    LoginWithToken {
        token: SecretString,
        reply: Reply<EstablishedBrowserSession>,
    },
    ResolveBrowserSession {
        cookie: SecretString,
        reply: Reply<AuthenticatedCaller>,
    },
    ListBrowserSessions {
        reply: Reply<Vec<BrowserSessionSummary>>,
    },
    RevokeBrowserSession {
        id: SessionId,
        reply: Reply<()>,
    },
    Logout {
        cookie: SecretString,
        reply: Reply<()>,
    },
    /// Whether a token or session stream principal is still valid.
    PrincipalActive {
        principal: Principal,
        reply: Reply<bool>,
    },
    /// The certificate of the HTTPS listener, generated on first use and when it has expired.
    TlsIdentity {
        addresses: Vec<IpAddr>,
        reply: Reply<TlsIdentity>,
    },
    /// The public view of the certificate, or `None` while none was generated yet.
    TlsSummary {
        reply: Reply<Option<TlsCertificateSummary>>,
    },
    /// Replaces the certificate, which changes its fingerprint.
    ResetTls {
        addresses: Vec<IpAddr>,
        reply: Reply<TlsCertificateSummary>,
    },
    /// Sent by the expiry timer; pruning before every message does the work.
    ExpiryTick,
}

pub(super) struct Args {
    pub(super) clock: Arc<dyn AuthClock>,
    pub(super) entropy: Arc<dyn EntropySource>,
    pub(super) store: Arc<dyn SecretStore>,
    pub(super) tls_generator: Arc<dyn TlsCertificateGenerator>,
    pub(super) policy: RemoteAuthPolicy,
    pub(super) tokens: Vec<TokenRecord>,
    pub(super) tls: Option<TlsIdentity>,
    pub(super) revocations: broadcast::Sender<AuthRevocation>,
}

struct LiveToken {
    record: TokenRecord,
    last_used_at_ms: Option<i64>,
    /// Whether the expiry was already announced and its sessions closed.
    expiry_announced: bool,
}

impl LiveToken {
    fn is_expired_at(&self, now_unix_ms: i64) -> bool {
        self.record
            .expires_at_ms
            .is_some_and(|expires| now_unix_ms >= expires)
    }

    fn summary(&self, now_unix_ms: i64) -> AccessTokenSummary {
        AccessTokenSummary {
            id: self.record.id.clone(),
            name: self.record.name.clone(),
            grant: self.record.grant,
            created_at_ms: self.record.created_at_ms,
            expires_at_ms: self.record.expires_at_ms,
            last_used_at_ms: self.last_used_at_ms,
            status: if self.is_expired_at(now_unix_ms) {
                AccessTokenStatus::Expired
            } else {
                AccessTokenStatus::Active
            },
        }
    }
}

struct LaunchCode {
    hash: SecretHash,
    expires_at_mono_ms: u64,
}

struct Session {
    id: SessionId,
    cookie_hash: SecretHash,
    backing_token_id: Option<TokenId>,
    grant: Grant,
    token_expires_at_ms: Option<i64>,
    created_at_ms: i64,
    last_active_at_ms: i64,
    idle_deadline_mono_ms: u64,
}

impl Session {
    fn summary(&self) -> BrowserSessionSummary {
        BrowserSessionSummary {
            id: self.id.clone(),
            backing_token_id: self.backing_token_id.clone(),
            grant: self.grant,
            created_at_ms: self.created_at_ms,
            last_active_at_ms: self.last_active_at_ms,
            token_expires_at_ms: self.token_expires_at_ms,
        }
    }

    fn caller(&self) -> AuthenticatedCaller {
        AuthenticatedCaller {
            auth: AuthContext::new(
                Principal::Session {
                    id: self.id.as_str().to_owned(),
                },
                self.grant,
            ),
            expires_at_ms: self.token_expires_at_ms,
        }
    }
}

pub(super) struct State {
    policy: RemoteAuthPolicy,
    clock: Arc<dyn AuthClock>,
    entropy: Arc<dyn EntropySource>,
    store: Arc<dyn SecretStore>,
    tls_generator: Arc<dyn TlsCertificateGenerator>,
    revocations: broadcast::Sender<AuthRevocation>,
    tokens: BTreeMap<TokenId, LiveToken>,
    tls: Option<TlsIdentity>,
    launch_codes: Vec<LaunchCode>,
    sessions: BTreeMap<SessionId, Session>,
    wake: Option<JoinHandle<Result<(), MessagingErr<Message>>>>,
}

pub(super) struct RemoteAccessAuthActor;

impl Actor for RemoteAccessAuthActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let tokens = args
            .tokens
            .into_iter()
            .map(|record| {
                (
                    record.id.clone(),
                    LiveToken {
                        record,
                        last_used_at_ms: None,
                        expiry_announced: false,
                    },
                )
            })
            .collect();
        let mut state = State {
            policy: args.policy,
            clock: args.clock,
            entropy: args.entropy,
            store: args.store,
            tls_generator: args.tls_generator,
            revocations: args.revocations,
            tokens,
            tls: args.tls,
            launch_codes: Vec::new(),
            sessions: BTreeMap::new(),
            wake: None,
        };
        state.prune();
        state.arm_wake(&myself);
        Ok(state)
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(wake) = state.wake.take() {
            wake.abort();
        }
        state.launch_codes.clear();
        state.sessions.clear();
        Ok(())
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.prune();
        match message {
            Message::ExpiryTick => {}
            Message::TlsIdentity { addresses, reply } => {
                let _ = reply.send(state.tls_identity(addresses).await);
            }
            Message::TlsSummary { reply } => {
                let _ = reply.send(Ok(state.tls.as_ref().map(TlsIdentity::summary)));
            }
            Message::ResetTls { addresses, reply } => {
                let _ = reply.send(state.reset_tls(addresses).await);
            }
            Message::CreateAccessToken { request, reply } => {
                let _ = reply.send(state.create_token(request).await);
            }
            Message::ListAccessTokens { reply } => {
                let now = state.clock.now_unix_ms();
                let _ = reply.send(Ok(state.tokens.values().map(|t| t.summary(now)).collect()));
            }
            Message::RevokeAccessToken { id, reply } => {
                let _ = reply.send(state.revoke_token(&id).await);
            }
            Message::AuthenticateToken { token, reply } => {
                let _ = reply.send(state.authenticate_token(&token));
            }
            Message::IssueLaunchCode { reply } => {
                let _ = reply.send(state.issue_launch_code());
            }
            Message::ExchangeLaunchCode { code, reply } => {
                let _ = reply.send(state.exchange_launch_code(&code));
            }
            Message::LoginWithToken { token, reply } => {
                let _ = reply.send(state.login_with_token(&token));
            }
            Message::ResolveBrowserSession { cookie, reply } => {
                let _ = reply.send(state.resolve_session(&cookie));
            }
            Message::ListBrowserSessions { reply } => {
                let _ = reply.send(Ok(state.sessions.values().map(Session::summary).collect()));
            }
            Message::RevokeBrowserSession { id, reply } => {
                let _ = reply.send(state.close_session(&id));
            }
            Message::Logout { cookie, reply } => {
                let _ = reply.send(state.logout(&cookie));
            }
            Message::PrincipalActive { principal, reply } => {
                let _ = reply.send(Ok(state.is_principal_active(&principal)));
            }
        }
        state.arm_wake(&myself);
        Ok(())
    }
}

impl State {
    fn announce(&self, event: AuthRevocation) {
        // No subscriber is not an error.
        let _ = self.revocations.send(event);
    }

    /// Removes every session backed by `token_id` and returns their ids.
    fn drop_sessions_of(&mut self, token_id: &TokenId) -> Vec<SessionId> {
        let ids: Vec<SessionId> = self
            .sessions
            .values()
            .filter(|session| session.backing_token_id.as_ref() == Some(token_id))
            .map(|session| session.id.clone())
            .collect();
        for id in &ids {
            self.sessions.remove(id);
        }
        ids
    }

    fn close_session(&mut self, id: &SessionId) -> Result<(), RemoteAuthError> {
        self.sessions.remove(id).ok_or(RemoteAuthError::NotFound)?;
        self.announce(AuthRevocation::Session {
            session_id: id.clone(),
        });
        Ok(())
    }

    /// Drops what has expired and announces it.
    fn prune(&mut self) {
        let now_unix = self.clock.now_unix_ms();
        let now_mono = self.clock.monotonic_ms();

        self.launch_codes
            .retain(|code| now_mono < code.expires_at_mono_ms);

        let newly_expired: Vec<TokenId> = self
            .tokens
            .values_mut()
            .filter(|token| !token.expiry_announced && token.is_expired_at(now_unix))
            .map(|token| {
                token.expiry_announced = true;
                token.record.id.clone()
            })
            .collect();
        for token_id in newly_expired {
            let session_ids = self.drop_sessions_of(&token_id);
            self.announce(AuthRevocation::Token {
                token_id,
                session_ids,
            });
        }

        let idle: Vec<SessionId> = self
            .sessions
            .values()
            .filter(|session| now_mono >= session.idle_deadline_mono_ms)
            .map(|session| session.id.clone())
            .collect();
        for session_id in idle {
            self.sessions.remove(&session_id);
            self.announce(AuthRevocation::Session { session_id });
        }
    }

    /// Time until the next expiry that has to be announced, if there is one.
    fn next_wake(&self) -> Option<Duration> {
        let now_unix = self.clock.now_unix_ms();
        let now_mono = self.clock.monotonic_ms();
        let tokens = self
            .tokens
            .values()
            .filter(|token| !token.expiry_announced)
            .filter_map(|token| token.record.expires_at_ms)
            .map(|expires| u64::try_from(expires.saturating_sub(now_unix)).unwrap_or(0));
        let sessions = self
            .sessions
            .values()
            .map(|session| session.idle_deadline_mono_ms.saturating_sub(now_mono));
        let delay_ms = tokens.chain(sessions).min()?;
        Some(Duration::from_millis(delay_ms.clamp(1, MAX_WAKE_DELAY_MS)))
    }

    fn arm_wake(&mut self, myself: &ActorRef<Message>) {
        if let Some(previous) = self.wake.take() {
            previous.abort();
        }
        if let Some(delay) = self.next_wake() {
            self.wake = Some(myself.send_after(delay, || Message::ExpiryTick));
        }
    }

    async fn create_token(
        &mut self,
        request: CreateAccessTokenRequest,
    ) -> Result<IssuedAccessToken, RemoteAuthError> {
        if self.tokens.len() >= self.policy.max_tokens {
            return Err(RemoteAuthError::CapacityExceeded { resource: "tokens" });
        }
        // The power to grant stays with the owner, and remote administration is never reachable
        // over HTTP, so a token holding it would only look more powerful than it is.
        request
            .grant
            .validate()
            .map_err(|error| RemoteAuthError::InvalidInput {
                reason: match error {
                    GrantError::UnassignableRole { .. } => {
                        "a token can only hold the viewer, operator or admin role"
                    }
                    GrantError::UngrantableResource { .. } => {
                        "a token cannot hold meta or remote permissions"
                    }
                },
            })?;
        let now = self.clock.now_unix_ms();
        if request.expires_at_ms.is_some_and(|expires| expires <= now) {
            return Err(RemoteAuthError::InvalidInput {
                reason: "token expiry must be in the future",
            });
        }
        let (id, raw, hash) = mint_token(self.entropy.as_ref())?;
        let live = LiveToken {
            record: TokenRecord {
                id: id.clone(),
                name: request.name,
                grant: request.grant,
                hash,
                created_at_ms: now,
                expires_at_ms: request.expires_at_ms,
            },
            last_used_at_ms: None,
            expiry_announced: false,
        };
        let summary = live.summary(now);
        self.tokens.insert(id.clone(), live);
        // The token is only handed out once it is durable.
        if let Err(error) = self.persist().await {
            self.tokens.remove(&id);
            return Err(error);
        }
        Ok(IssuedAccessToken {
            summary,
            token: raw,
        })
    }

    async fn revoke_token(&mut self, id: &TokenId) -> Result<(), RemoteAuthError> {
        self.tokens.remove(id).ok_or(RemoteAuthError::NotFound)?;
        let session_ids = self.drop_sessions_of(id);
        self.announce(AuthRevocation::Token {
            token_id: id.clone(),
            session_ids,
        });
        // Revocation already holds in memory; a failed write is reported, not undone.
        self.persist().await
    }

    async fn persist(&self) -> Result<(), RemoteAuthError> {
        save_record(
            self.store.as_ref(),
            self.tokens.values().map(|token| &token.record),
            self.tls.as_ref(),
        )
        .await
    }

    /// The certificate in use. A missing or expired one is generated and made durable first; one
    /// that is still valid is kept even when the listener's addresses changed, because a new
    /// fingerprint would break every pinned client. An explicit reset replaces it.
    async fn tls_identity(
        &mut self,
        addresses: Vec<IpAddr>,
    ) -> Result<TlsIdentity, RemoteAuthError> {
        let now = self.clock.now_unix_ms();
        if let Some(identity) = self.tls.as_ref().filter(|tls| now < tls.not_after_ms()) {
            return Ok(identity.clone());
        }
        self.replace_tls(addresses).await
    }

    async fn reset_tls(
        &mut self,
        addresses: Vec<IpAddr>,
    ) -> Result<TlsCertificateSummary, RemoteAuthError> {
        self.replace_tls(addresses)
            .await
            .map(|identity| identity.summary())
    }

    /// Generates a certificate and keeps it only once it is durable, so a restart never serves a
    /// certificate that was not saved.
    async fn replace_tls(
        &mut self,
        addresses: Vec<IpAddr>,
    ) -> Result<TlsIdentity, RemoteAuthError> {
        let now = self.clock.now_unix_ms();
        let identity = self
            .tls_generator
            .generate(TlsCertificateRequest {
                addresses,
                not_before_ms: now.saturating_sub(TLS_CLOCK_SKEW_MS),
                not_after_ms: now.saturating_add(TLS_VALIDITY_MS),
            })
            .await
            .map_err(|error| RemoteAuthError::PersistenceFailed {
                reason: error.to_string(),
            })?;
        let previous = self.tls.replace(identity.clone());
        if let Err(error) = self.persist().await {
            self.tls = previous;
            return Err(error);
        }
        Ok(identity)
    }

    /// Looks up the token a raw credential names, doing the same hashing work for every
    /// outcome so a miss cannot be told from a wrong secret by timing.
    fn verify_token(&mut self, raw: &SecretString) -> Option<&LiveToken> {
        let now = self.clock.now_unix_ms();
        let candidate = sha256_secret(raw.expose_secret().as_bytes());
        let parsed = parse_access_token(raw.expose_secret());
        let known = parsed
            .as_ref()
            .and_then(|parsed| self.tokens.get(&parsed.id));
        let stored = known.map_or(&candidate, |live| &live.record.hash);
        let matches = constant_time_hash_eq(stored, &candidate);
        let id = parsed.map(|parsed| parsed.id)?;
        let live = self.tokens.get_mut(&id)?;
        if !matches || live.is_expired_at(now) {
            return None;
        }
        live.last_used_at_ms = Some(now);
        Some(&*live)
    }

    fn authenticate_token(
        &mut self,
        raw: &SecretString,
    ) -> Result<AuthenticatedCaller, RemoteAuthError> {
        let live = self
            .verify_token(raw)
            .ok_or(RemoteAuthError::Unauthorized)?;
        Ok(AuthenticatedCaller {
            auth: AuthContext::new(
                Principal::Token {
                    id: live.record.id.to_string(),
                    name: live.record.name.to_string(),
                },
                live.record.grant,
            ),
            expires_at_ms: live.record.expires_at_ms,
        })
    }

    fn issue_launch_code(&mut self) -> Result<IssuedLaunchCode, RemoteAuthError> {
        if self.launch_codes.len() >= MAX_LAUNCH_CODES {
            return Err(RemoteAuthError::CapacityExceeded {
                resource: "launch_codes",
            });
        }
        let now_unix = self.clock.now_unix_ms();
        let ttl_ms = self.policy.launch_code_ttl_ms;
        let (code, hash) = mint_launch_code(self.entropy.as_ref())?;
        self.launch_codes.push(LaunchCode {
            hash,
            expires_at_mono_ms: self.clock.monotonic_ms().saturating_add(ttl_ms),
        });
        Ok(IssuedLaunchCode {
            code,
            issued_at_ms: now_unix,
            expires_at_ms: now_unix.saturating_add(i64::try_from(ttl_ms).unwrap_or(i64::MAX)),
        })
    }

    fn exchange_launch_code(
        &mut self,
        raw: &SecretString,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        let candidate = sha256_secret(raw.expose_secret().as_bytes());
        let syntax_ok = is_valid_launch_code_syntax(raw.expose_secret());
        // Compare against every pending code so the work does not depend on which one matches.
        let mut matched = None;
        for (index, code) in self.launch_codes.iter().enumerate() {
            if constant_time_hash_eq(&code.hash, &candidate) && syntax_ok {
                matched = Some(index);
            }
        }
        let index = matched.ok_or(RemoteAuthError::Unauthorized)?;
        self.launch_codes.swap_remove(index);
        // A launch code comes from the owner's desktop: everything but remote administration.
        self.open_session(Grant::Role(Role::Admin), None, None)
    }

    fn login_with_token(
        &mut self,
        raw: &SecretString,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        let live = self
            .verify_token(raw)
            .ok_or(RemoteAuthError::Unauthorized)?;
        let (grant, id, expires) = (
            live.record.grant,
            live.record.id.clone(),
            live.record.expires_at_ms,
        );
        self.open_session(grant, Some(id), expires)
    }

    /// A full session table is a bounded cache, not a login denial. Prefer dropping one of the
    /// caller's own older sessions; if it has none, drop the least recently active session.
    fn make_session_room(
        &mut self,
        backing_token_id: Option<&TokenId>,
    ) -> Result<(), RemoteAuthError> {
        while self.sessions.len() >= self.policy.max_sessions {
            let victim = backing_token_id
                .and_then(|token_id| self.oldest_session(Some(token_id)))
                .or_else(|| self.oldest_session(None))
                .ok_or(RemoteAuthError::CapacityExceeded {
                    resource: "browser_sessions",
                })?;
            self.sessions.remove(&victim);
            self.announce(AuthRevocation::Session { session_id: victim });
        }
        Ok(())
    }

    fn oldest_session(&self, backing_token_id: Option<&TokenId>) -> Option<SessionId> {
        self.sessions
            .values()
            .filter(|session| {
                backing_token_id
                    .is_none_or(|token_id| session.backing_token_id.as_ref() == Some(token_id))
            })
            .min_by(|left, right| {
                left.idle_deadline_mono_ms
                    .cmp(&right.idle_deadline_mono_ms)
                    .then_with(|| left.id.cmp(&right.id))
            })
            .map(|session| session.id.clone())
    }

    fn open_session(
        &mut self,
        grant: Grant,
        backing_token_id: Option<TokenId>,
        token_expires_at_ms: Option<i64>,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        let now_unix = self.clock.now_unix_ms();
        let (raw_id, cookie, cookie_hash) = mint_session_cookie(self.entropy.as_ref())?;
        let id = SessionId::parse(raw_id).map_err(|error| RemoteAuthError::EntropyFailed {
            reason: error.to_string(),
        })?;
        self.make_session_room(backing_token_id.as_ref())?;
        let session = Session {
            id: id.clone(),
            cookie_hash,
            backing_token_id,
            grant,
            token_expires_at_ms,
            created_at_ms: now_unix,
            last_active_at_ms: now_unix,
            idle_deadline_mono_ms: self
                .clock
                .monotonic_ms()
                .saturating_add(self.policy.session_idle_ms),
        };
        let established = EstablishedBrowserSession {
            summary: session.summary(),
            auth: session.caller().auth,
            cookie,
        };
        self.sessions.insert(id, session);
        Ok(established)
    }

    /// The session a cookie names, with the same hashing work for every outcome.
    fn verify_session(&mut self, raw: &SecretString) -> Option<&mut Session> {
        let candidate = sha256_secret(raw.expose_secret().as_bytes());
        let id = parse_session_cookie(raw.expose_secret());
        let known = id.as_ref().and_then(|id| self.sessions.get(id));
        let stored = known.map_or(&candidate, |session| &session.cookie_hash);
        let matches = constant_time_hash_eq(stored, &candidate);
        let session = self.sessions.get_mut(&id?)?;
        matches.then_some(session)
    }

    fn resolve_session(
        &mut self,
        raw: &SecretString,
    ) -> Result<AuthenticatedCaller, RemoteAuthError> {
        let now_unix = self.clock.now_unix_ms();
        let now_mono = self.clock.monotonic_ms();
        let idle_ms = self.policy.session_idle_ms;
        let session = self
            .verify_session(raw)
            .ok_or(RemoteAuthError::Unauthorized)?;
        session.last_active_at_ms = now_unix;
        session.idle_deadline_mono_ms = now_mono.saturating_add(idle_ms);
        Ok(session.caller())
    }

    fn logout(&mut self, raw: &SecretString) -> Result<(), RemoteAuthError> {
        let id = self
            .verify_session(raw)
            .map(|session| session.id.clone())
            .ok_or(RemoteAuthError::Unauthorized)?;
        self.close_session(&id)
    }

    /// Reports whether a token or session principal can still authenticate. Other principal
    /// kinds are not owned by this actor and are not tracked by HTTP stream revocation.
    fn is_principal_active(&self, principal: &Principal) -> bool {
        match principal {
            Principal::Token { id, .. } => TokenId::parse(id.clone())
                .ok()
                .and_then(|id| self.tokens.get(&id))
                .is_some_and(|token| !token.is_expired_at(self.clock.now_unix_ms())),
            Principal::Session { id } => SessionId::parse(id.clone())
                .ok()
                .is_some_and(|id| self.sessions.contains_key(&id)),
            Principal::Anonymous | Principal::Desktop { .. } | Principal::LocalSocket { .. } => {
                true
            }
        }
    }
}
