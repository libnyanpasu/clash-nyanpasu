//! Serial owner of access tokens, single-use launch codes and browser sessions.
//!
//! One mailbox serializes every credential change, so verification, revocation and expiry can
//! never race. Expired items are pruned before each message and by a timer armed for the next
//! deadline, which is what announces idle-expired sessions and expired tokens to subscribers.
//! Tokens are persisted through the injected [`SecretStore`]; launch codes and sessions are
//! memory only and do not survive a restart.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use nyanpasu_contracts::auth::{
    AccessTokenStatus, AccessTokenSummary, AuthClock, AuthContext, AuthRevocation,
    AuthenticatedCaller, BrowserSessionSummary, CreateAccessTokenRequest, EntropySource,
    EstablishedBrowserSession, IssuedAccessToken, IssuedLaunchCode, Principal, RemoteAuthPolicy,
    ScopeSet, SecretHash, SecretStore, SecretString, SessionId, TokenId,
    is_valid_launch_code_syntax, parse_access_token, parse_session_cookie,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, MessagingErr, RpcReplyPort};
use tokio::{sync::broadcast, task::JoinHandle};

use super::{
    auth_error::RemoteAuthError,
    auth_store::{
        TokenRecord, constant_time_hash_eq, mint_launch_code, mint_session_cookie, mint_token,
        save_tokens, sha256_secret,
    },
};

/// Pending launch codes kept at once. They live 60 seconds, so this only bounds a flood.
const MAX_LAUNCH_CODES: usize = 32;
const MAX_WAKE_DELAY_MS: u64 = 24 * 60 * 60 * 1000;

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
        scopes: ScopeSet,
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
    /// Sent by the expiry timer; pruning before every message does the work.
    ExpiryTick,
}

pub(super) struct Args {
    pub(super) clock: Arc<dyn AuthClock>,
    pub(super) entropy: Arc<dyn EntropySource>,
    pub(super) store: Arc<dyn SecretStore>,
    pub(super) policy: RemoteAuthPolicy,
    pub(super) tokens: Vec<TokenRecord>,
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
            scopes: self.record.scopes.clone(),
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
    scopes: ScopeSet,
    expires_at_mono_ms: u64,
}

struct Session {
    id: SessionId,
    cookie_hash: SecretHash,
    backing_token_id: Option<TokenId>,
    scopes: ScopeSet,
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
            scopes: self.scopes.clone(),
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
                self.scopes.clone(),
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
    revocations: broadcast::Sender<AuthRevocation>,
    tokens: BTreeMap<TokenId, LiveToken>,
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
            revocations: args.revocations,
            tokens,
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
            Message::IssueLaunchCode { scopes, reply } => {
                let _ = reply.send(state.issue_launch_code(scopes));
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
                scopes: request.scopes,
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
        save_tokens(
            self.store.as_ref(),
            self.tokens.values().map(|token| &token.record),
        )
        .await
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
                live.record.scopes.clone(),
            ),
            expires_at_ms: live.record.expires_at_ms,
        })
    }

    fn issue_launch_code(&mut self, scopes: ScopeSet) -> Result<IssuedLaunchCode, RemoteAuthError> {
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
            scopes: scopes.clone(),
            expires_at_mono_ms: self.clock.monotonic_ms().saturating_add(ttl_ms),
        });
        Ok(IssuedLaunchCode {
            code,
            scopes,
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
        self.ensure_session_capacity()?;
        let code = self.launch_codes.swap_remove(index);
        self.open_session(code.scopes, None, None)
    }

    fn login_with_token(
        &mut self,
        raw: &SecretString,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        let live = self
            .verify_token(raw)
            .ok_or(RemoteAuthError::Unauthorized)?;
        let (scopes, id, expires) = (
            live.record.scopes.clone(),
            live.record.id.clone(),
            live.record.expires_at_ms,
        );
        self.ensure_session_capacity()?;
        self.open_session(scopes, Some(id), expires)
    }

    fn ensure_session_capacity(&self) -> Result<(), RemoteAuthError> {
        if self.sessions.len() >= self.policy.max_sessions {
            return Err(RemoteAuthError::CapacityExceeded {
                resource: "browser_sessions",
            });
        }
        Ok(())
    }

    fn open_session(
        &mut self,
        scopes: ScopeSet,
        backing_token_id: Option<TokenId>,
        token_expires_at_ms: Option<i64>,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        let now_unix = self.clock.now_unix_ms();
        let (raw_id, cookie, cookie_hash) = mint_session_cookie(self.entropy.as_ref())?;
        let id = SessionId::parse(raw_id).map_err(|error| RemoteAuthError::EntropyFailed {
            reason: error.to_string(),
        })?;
        let session = Session {
            id: id.clone(),
            cookie_hash,
            backing_token_id,
            scopes,
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
}
