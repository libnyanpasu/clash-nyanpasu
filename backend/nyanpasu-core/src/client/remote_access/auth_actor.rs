//! Serial actor owner and typed client facade for remote-access credentials,
//! browser/access sessions, principal-bound resource leases, and revocations.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use nyanpasu_contracts::auth::{
    AccessTokenStatus, AccessTokenSummary, AuthClock, AuthContext, AuthPeer, AuthRevocation,
    AuthenticatedCaller, BrowserSessionSummary, CreateAccessTokenRequest, EntropySource,
    EstablishedBrowserSession, EstablishedResourceSession, IssuedAccessToken, IssuedLaunchCode,
    Principal, ResolvedResourceSession, ResourceLease, ResourceOwnerId, ResourceSessionId,
    ScopeSet, SecretCipher, SecretHash, SecretStore, SecretString, SessionId, TlsMaterialBlob,
    TlsMaterialSummary, TokenId, is_valid_launch_code_syntax, parse_access_token,
    parse_resource_cookie, parse_session_cookie,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::{
    auth_store::{
        MAX_PERSISTED_RECORD_BYTES, PersistedAuthState, PersistedTokenRecord,
        constant_time_hash_eq, load_persisted_state, mint_launch_code_credential,
        mint_prefixed_cookie_credential, mint_token_credential, save_persisted_state,
        sha256_secret, validate_tls_blob,
    },
    error::RemoteAuthError,
    rate_limit::{PeerRateLimitConfig, PeerRateLimiter},
};

/// Default one-time browser launch code TTL in monotonic milliseconds (60 seconds).
pub const DEFAULT_LAUNCH_CODE_TTL_MS: u64 = 60_000;
/// Default browser session idle expiration in monotonic milliseconds (30 minutes).
pub const DEFAULT_BROWSER_SESSION_IDLE_MS: u64 = 30 * 60 * 1000;
/// Default resource session idle expiration in monotonic milliseconds (15 minutes).
pub const DEFAULT_RESOURCE_SESSION_IDLE_MS: u64 = 15 * 60 * 1000;
/// Default maximum interval between wall-clock expiry checks when UTC-expiring tokens/sessions exist.
pub const DEFAULT_WALL_EXPIRY_RECHECK_MS: u64 = 1_000;
const MAX_EXPIRY_SLEEP_MS: u64 = 86_400_000;

/// Bounded policy limits governing remote authentication, sessions, leases, and lockouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteAuthPolicy {
    /// Maximum persisted named access tokens.
    pub max_tokens: usize,
    /// Maximum pending one-time launch codes kept concurrently.
    pub max_launch_codes: usize,
    /// Single-use launch code time-to-live on the monotonic clock.
    pub launch_code_ttl_ms: u64,
    /// Maximum active browser/access sessions.
    pub max_browser_sessions: usize,
    /// Idle timeout for browser/access sessions on the monotonic clock.
    pub browser_session_idle_ms: u64,
    /// Global cap across all active resource sessions.
    pub max_resource_sessions_global: usize,
    /// Per-principal cap for active resource sessions (including shared tokens and Anonymous).
    pub max_resource_sessions_per_principal: usize,
    /// Idle timeout for bearer and anonymous resource sessions on the monotonic clock (`>= 1`).
    pub resource_session_idle_ms: u64,
    /// Capacity of the `tokio::sync::broadcast` channel for `AuthRevocation` notifications.
    pub revocation_channel_capacity: usize,
    /// Maximum persisted secret-store record or decrypted payload byte length (`>= 1`).
    pub max_persisted_record_bytes: usize,
    /// Maximum sleep interval (ms) between wall-clock expiry rechecks when UTC-expiring items exist (`>= 1`).
    pub wall_expiry_recheck_ms: u64,
    /// Bounded failed-authentication rate-limit configuration.
    pub rate_limit: PeerRateLimitConfig,
}

impl RemoteAuthPolicy {
    /// Validate that all configured capacities, TTLs, and rate-limit bounds are non-zero.
    pub fn validate(&self) -> Result<(), RemoteAuthError> {
        if self.max_tokens == 0
            || self.max_launch_codes == 0
            || self.launch_code_ttl_ms == 0
            || self.max_browser_sessions == 0
            || self.browser_session_idle_ms == 0
            || self.max_resource_sessions_global == 0
            || self.max_resource_sessions_per_principal == 0
            || self.resource_session_idle_ms == 0
            || self.max_persisted_record_bytes == 0
            || self.wall_expiry_recheck_ms == 0
        {
            return Err(RemoteAuthError::InvalidInput {
                reason: "remote auth policy capacities, byte limits, and TTLs must all be >= 1"
                    .to_owned(),
            });
        }
        self.rate_limit.validate()
    }
}

impl Default for RemoteAuthPolicy {
    fn default() -> Self {
        Self {
            max_tokens: 64,
            max_launch_codes: 32,
            launch_code_ttl_ms: DEFAULT_LAUNCH_CODE_TTL_MS,
            max_browser_sessions: 64,
            browser_session_idle_ms: DEFAULT_BROWSER_SESSION_IDLE_MS,
            max_resource_sessions_global: 256,
            max_resource_sessions_per_principal: 16,
            resource_session_idle_ms: DEFAULT_RESOURCE_SESSION_IDLE_MS,
            revocation_channel_capacity: 256,
            max_persisted_record_bytes: MAX_PERSISTED_RECORD_BYTES,
            wall_expiry_recheck_ms: DEFAULT_WALL_EXPIRY_RECHECK_MS,
            rate_limit: PeerRateLimitConfig::default(),
        }
    }
}

/// Explicit dependency injection bundle for the core remote-access authentication owner.
#[derive(Clone)]
pub struct RemoteAccessAuthPorts {
    /// Wall and monotonic clock dependency.
    pub clock: Arc<dyn AuthClock>,
    /// Cryptographic random byte generator.
    pub entropy: Arc<dyn EntropySource>,
    /// Durable secret store port enforcing write-time owner protection.
    pub store: Arc<dyn SecretStore>,
    /// Optional OS-credential-store envelope cipher used when backing storage is unprotected.
    pub cipher: Option<Arc<dyn SecretCipher>>,
    /// Policy bounds and TTL configuration.
    pub policy: RemoteAuthPolicy,
}

impl RemoteAccessAuthPorts {
    /// Bundle injected auth infrastructure with default `RemoteAuthPolicy`.
    #[must_use]
    pub fn new(
        clock: Arc<dyn AuthClock>,
        entropy: Arc<dyn EntropySource>,
        store: Arc<dyn SecretStore>,
        cipher: Option<Arc<dyn SecretCipher>>,
    ) -> Self {
        Self {
            clock,
            entropy,
            store,
            cipher,
            policy: RemoteAuthPolicy::default(),
        }
    }

    /// Override policy limits/TTLs on the injected ports bundle.
    #[must_use]
    pub fn with_policy(mut self, policy: RemoteAuthPolicy) -> Self {
        self.policy = policy;
        self
    }
}

/// Typed client facade hiding `ActorRef` for all remote-access auth and lease operations.
#[derive(Clone)]
pub(super) struct RemoteAccessAuthClient {
    actor: ActorRef<AuthMessage>,
    revocations: broadcast::Sender<AuthRevocation>,
}

impl RemoteAccessAuthClient {
    pub(super) async fn spawn(
        ports: RemoteAccessAuthPorts,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Result<Self, RemoteAuthError> {
        ports.policy.validate()?;

        let persisted = load_persisted_state(
            ports.store.as_ref(),
            ports.cipher.as_deref(),
            ports.policy.max_tokens,
            ports.policy.max_persisted_record_bytes,
        )
        .await?;

        let channel_cap = ports.policy.revocation_channel_capacity.max(8);
        let (revocations, _) = broadcast::channel(channel_cap);

        let startup = AuthStartupArgs {
            ports,
            persisted,
            revocations: revocations.clone(),
        };
        let (actor, _handle) = Actor::spawn(None, RemoteAccessAuthActor, startup)
            .await
            .map_err(|_| RemoteAuthError::OwnerStopped)?;
        nyanpasu_owner_support::shutdown::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self { actor, revocations })
    }

    pub(super) fn subscribe_revocations(&self) -> broadcast::Receiver<AuthRevocation> {
        self.revocations.subscribe()
    }

    pub(super) async fn create_access_token(
        &self,
        request: CreateAccessTokenRequest,
    ) -> Result<IssuedAccessToken, RemoteAuthError> {
        self.call(|reply| AuthMessage::CreateAccessToken { request, reply })
            .await
    }

    pub(super) async fn list_access_tokens(
        &self,
    ) -> Result<Vec<AccessTokenSummary>, RemoteAuthError> {
        self.call(|reply| AuthMessage::ListAccessTokens { reply })
            .await
    }

    pub(super) async fn revoke_access_token(&self, id: TokenId) -> Result<(), RemoteAuthError> {
        self.call(|reply| AuthMessage::RevokeAccessToken { id, reply })
            .await
    }

    pub(super) async fn authenticate_token(
        &self,
        peer: AuthPeer,
        token: SecretString,
    ) -> Result<AuthenticatedCaller, RemoteAuthError> {
        self.call(|reply| AuthMessage::AuthenticateToken { peer, token, reply })
            .await
    }

    pub(super) async fn issue_launch_code(
        &self,
        scopes: ScopeSet,
    ) -> Result<IssuedLaunchCode, RemoteAuthError> {
        self.call(|reply| AuthMessage::IssueLaunchCode { scopes, reply })
            .await
    }

    pub(super) async fn exchange_launch_code(
        &self,
        peer: AuthPeer,
        code: SecretString,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        self.call(|reply| AuthMessage::ExchangeLaunchCode { peer, code, reply })
            .await
    }

    pub(super) async fn login_with_token(
        &self,
        peer: AuthPeer,
        token: SecretString,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        self.call(|reply| AuthMessage::LoginWithToken { peer, token, reply })
            .await
    }

    pub(super) async fn resolve_browser_session(
        &self,
        peer: AuthPeer,
        cookie: SecretString,
    ) -> Result<AuthenticatedCaller, RemoteAuthError> {
        self.call(|reply| AuthMessage::ResolveBrowserSession {
            peer,
            cookie,
            reply,
        })
        .await
    }

    pub(super) async fn list_browser_sessions(
        &self,
    ) -> Result<Vec<BrowserSessionSummary>, RemoteAuthError> {
        self.call(|reply| AuthMessage::ListBrowserSessions { reply })
            .await
    }

    pub(super) async fn revoke_browser_session(
        &self,
        id: SessionId,
    ) -> Result<(), RemoteAuthError> {
        self.call(|reply| AuthMessage::RevokeBrowserSession { id, reply })
            .await
    }

    pub(super) async fn admit_caller(
        &self,
        caller: AuthenticatedCaller,
    ) -> Result<AuthenticatedCaller, RemoteAuthError> {
        self.call(|reply| AuthMessage::AdmitCaller { caller, reply })
            .await
    }

    pub(super) async fn create_resource_session(
        &self,
        caller: AuthContext,
    ) -> Result<EstablishedResourceSession, RemoteAuthError> {
        self.call(|reply| AuthMessage::CreateResourceSession { caller, reply })
            .await
    }

    pub(super) async fn resolve_resource_session(
        &self,
        caller: AuthContext,
        cookie: SecretString,
    ) -> Result<ResolvedResourceSession, RemoteAuthError> {
        self.call(|reply| AuthMessage::ResolveResourceSession {
            caller,
            cookie,
            reply,
        })
        .await
    }

    pub(super) async fn release_resource_session(
        &self,
        id: ResourceSessionId,
    ) -> Result<(), RemoteAuthError> {
        self.call(|reply| AuthMessage::ReleaseResourceSession { id, reply })
            .await
    }

    pub(super) async fn set_tls_material(
        &self,
        blob: TlsMaterialBlob,
    ) -> Result<TlsMaterialSummary, RemoteAuthError> {
        self.call(|reply| AuthMessage::SetTlsMaterial { blob, reply })
            .await
    }

    pub(super) async fn tls_material_status(
        &self,
    ) -> Result<Option<TlsMaterialSummary>, RemoteAuthError> {
        self.call(|reply| AuthMessage::TlsMaterialStatus { reply })
            .await
    }

    pub(super) async fn load_tls_material(
        &self,
    ) -> Result<Option<TlsMaterialBlob>, RemoteAuthError> {
        self.call(|reply| AuthMessage::LoadTlsMaterial { reply })
            .await
    }

    async fn call<T: Send + 'static>(
        &self,
        make: impl FnOnce(RpcReplyPort<Result<T, RemoteAuthError>>) -> AuthMessage,
    ) -> Result<T, RemoteAuthError> {
        match self.actor.call(make, None).await {
            Ok(CallResult::Success(result)) => result,
            _ => Err(RemoteAuthError::OwnerStopped),
        }
    }
}

struct AuthStartupArgs {
    ports: RemoteAccessAuthPorts,
    persisted: PersistedAuthState,
    revocations: broadcast::Sender<AuthRevocation>,
}

enum AuthMessage {
    CreateAccessToken {
        request: CreateAccessTokenRequest,
        reply: RpcReplyPort<Result<IssuedAccessToken, RemoteAuthError>>,
    },
    ListAccessTokens {
        reply: RpcReplyPort<Result<Vec<AccessTokenSummary>, RemoteAuthError>>,
    },
    RevokeAccessToken {
        id: TokenId,
        reply: RpcReplyPort<Result<(), RemoteAuthError>>,
    },
    AuthenticateToken {
        peer: AuthPeer,
        token: SecretString,
        reply: RpcReplyPort<Result<AuthenticatedCaller, RemoteAuthError>>,
    },
    IssueLaunchCode {
        scopes: ScopeSet,
        reply: RpcReplyPort<Result<IssuedLaunchCode, RemoteAuthError>>,
    },
    ExchangeLaunchCode {
        peer: AuthPeer,
        code: SecretString,
        reply: RpcReplyPort<Result<EstablishedBrowserSession, RemoteAuthError>>,
    },
    LoginWithToken {
        peer: AuthPeer,
        token: SecretString,
        reply: RpcReplyPort<Result<EstablishedBrowserSession, RemoteAuthError>>,
    },
    ResolveBrowserSession {
        peer: AuthPeer,
        cookie: SecretString,
        reply: RpcReplyPort<Result<AuthenticatedCaller, RemoteAuthError>>,
    },
    ListBrowserSessions {
        reply: RpcReplyPort<Result<Vec<BrowserSessionSummary>, RemoteAuthError>>,
    },
    RevokeBrowserSession {
        id: SessionId,
        reply: RpcReplyPort<Result<(), RemoteAuthError>>,
    },
    AdmitCaller {
        caller: AuthenticatedCaller,
        reply: RpcReplyPort<Result<AuthenticatedCaller, RemoteAuthError>>,
    },
    CreateResourceSession {
        caller: AuthContext,
        reply: RpcReplyPort<Result<EstablishedResourceSession, RemoteAuthError>>,
    },
    ResolveResourceSession {
        caller: AuthContext,
        cookie: SecretString,
        reply: RpcReplyPort<Result<ResolvedResourceSession, RemoteAuthError>>,
    },
    ReleaseResourceSession {
        id: ResourceSessionId,
        reply: RpcReplyPort<Result<(), RemoteAuthError>>,
    },
    SetTlsMaterial {
        blob: TlsMaterialBlob,
        reply: RpcReplyPort<Result<TlsMaterialSummary, RemoteAuthError>>,
    },
    TlsMaterialStatus {
        reply: RpcReplyPort<Result<Option<TlsMaterialSummary>, RemoteAuthError>>,
    },
    LoadTlsMaterial {
        reply: RpcReplyPort<Result<Option<TlsMaterialBlob>, RemoteAuthError>>,
    },
    ExpiryTick,
}

#[derive(Clone)]
struct LiveTokenRecord {
    record: PersistedTokenRecord,
    generation: u64,
    expired_notified: bool,
}

impl LiveTokenRecord {
    fn is_expired_at(&self, now_unix_ms: i64) -> bool {
        self.record
            .expires_at_ms
            .is_some_and(|exp| now_unix_ms >= exp)
    }

    fn summary(&self, now_unix_ms: i64) -> AccessTokenSummary {
        let status = if self.is_expired_at(now_unix_ms) {
            AccessTokenStatus::Expired
        } else {
            AccessTokenStatus::Active
        };
        AccessTokenSummary {
            id: self.record.id.clone(),
            name: self.record.name.clone(),
            scopes: self.record.scopes.clone(),
            created_at_ms: self.record.created_at_ms,
            expires_at_ms: self.record.expires_at_ms,
            last_used_at_ms: self.record.last_used_at_ms,
            status,
        }
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
struct LaunchCodeRecord {
    code_hash: SecretHash,
    #[zeroize(skip)]
    scopes: ScopeSet,
    issued_at_ms: i64,
    expires_at_monotonic_ms: u64,
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
struct BrowserSessionRecord {
    #[zeroize(skip)]
    id: SessionId,
    cookie_hash: SecretHash,
    #[zeroize(skip)]
    backing_token_id: Option<TokenId>,
    #[zeroize(skip)]
    scopes: ScopeSet,
    created_at_ms: i64,
    last_active_at_ms: i64,
    idle_expires_at_monotonic_ms: u64,
    token_expires_at_ms: Option<i64>,
    generation: u64,
}

impl BrowserSessionRecord {
    fn is_expired_at(&self, now_mono_ms: u64, now_unix_ms: i64) -> bool {
        if now_mono_ms >= self.idle_expires_at_monotonic_ms {
            return true;
        }
        self.token_expires_at_ms
            .is_some_and(|exp| now_unix_ms >= exp)
    }

    fn auth_context(&self) -> AuthContext {
        // Explicit AuthContext::new preserves bound ScopeSet; NEVER use legacy AuthContext::session.
        AuthContext::new(
            Principal::Session {
                id: self.id.as_str().to_owned(),
            },
            self.scopes.clone(),
        )
    }

    fn summary(&self) -> BrowserSessionSummary {
        BrowserSessionSummary {
            id: self.id.clone(),
            backing_token_id: self.backing_token_id.clone(),
            scopes: self.scopes.clone(),
            created_at_ms: self.created_at_ms,
            last_active_at_ms: self.last_active_at_ms,
            idle_expires_at_monotonic_ms: self.idle_expires_at_monotonic_ms,
            token_expires_at_ms: self.token_expires_at_ms,
        }
    }
}

#[derive(Clone)]
struct ResourceSessionRecord {
    id: ResourceSessionId,
    owner_id: ResourceOwnerId,
    cookie_hash: SecretHash,
    principal: Principal,
    principal_bucket: String,
    backing_token_id: Option<TokenId>,
    backing_session_id: Option<SessionId>,
    idle_expires_at_monotonic_ms: u64,
    lease_token: CancellationToken,
}

impl ResourceSessionRecord {
    fn is_expired_at(&self, now_mono_ms: u64) -> bool {
        now_mono_ms >= self.idle_expires_at_monotonic_ms
    }

    fn refresh_idle(&mut self, now_mono_ms: u64, idle_ttl_ms: u64) {
        self.idle_expires_at_monotonic_ms = now_mono_ms.saturating_add(idle_ttl_ms);
    }
}

struct ExpiryWakeHandle {
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

impl ExpiryWakeHandle {
    fn stop(self) {
        self.cancel.cancel();
        self.task.abort();
    }
}

struct AuthActorState {
    policy: RemoteAuthPolicy,
    clock: Arc<dyn AuthClock>,
    entropy: Arc<dyn EntropySource>,
    store: Arc<dyn SecretStore>,
    cipher: Option<Arc<dyn SecretCipher>>,
    revocations: broadcast::Sender<AuthRevocation>,
    tokens: BTreeMap<TokenId, LiveTokenRecord>,
    pending_revocations: BTreeSet<TokenId>,
    launch_codes: Vec<LaunchCodeRecord>,
    browser_sessions: BTreeMap<SessionId, BrowserSessionRecord>,
    resource_sessions: BTreeMap<ResourceSessionId, ResourceSessionRecord>,
    rate_limiter: PeerRateLimiter,
    tls_material: Option<TlsMaterialBlob>,
    next_generation: u64,
    expiry_wake: Option<ExpiryWakeHandle>,
}

impl AuthActorState {
    fn bump_generation(&mut self) -> u64 {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.saturating_add(1);
        generation
    }

    fn persisted_tokens_snapshot(&self) -> Vec<PersistedTokenRecord> {
        self.tokens
            .values()
            .filter(|live| !self.pending_revocations.contains(&live.record.id))
            .map(|live| live.record.clone())
            .collect()
    }

    fn emit_revocation(&self, event: AuthRevocation) {
        let _ = self.revocations.send(event);
    }

    fn cancel_resource_sessions_matching(
        &mut self,
        predicate: impl Fn(&ResourceSessionRecord) -> bool,
    ) -> Vec<ResourceOwnerId> {
        let matched_ids: Vec<ResourceSessionId> = self
            .resource_sessions
            .iter()
            .filter(|(_, record)| predicate(record))
            .map(|(id, _)| id.clone())
            .collect();
        let mut owners = Vec::with_capacity(matched_ids.len());
        for id in matched_ids {
            if let Some(record) = self.resource_sessions.remove(&id) {
                record.lease_token.cancel();
                owners.push(record.owner_id);
            }
        }
        owners
    }

    fn invalidate_session(&mut self, id: &SessionId) {
        if self.browser_sessions.remove(id).is_some() {
            let owners = self.cancel_resource_sessions_matching(|rec| {
                rec.backing_session_id.as_ref() == Some(id)
            });
            self.emit_revocation(AuthRevocation::Session {
                session_id: id.clone(),
                resource_owners: owners,
            });
        }
    }

    fn invalidate_token_runtime(&mut self, token_id: &TokenId) {
        let session_ids: Vec<SessionId> = self
            .browser_sessions
            .iter()
            .filter(|(_, session)| session.backing_token_id.as_ref() == Some(token_id))
            .map(|(id, _)| id.clone())
            .collect();

        let mut all_owners = self.cancel_resource_sessions_matching(|rec| {
            rec.backing_token_id.as_ref() == Some(token_id)
                || rec
                    .backing_session_id
                    .as_ref()
                    .is_some_and(|sid| session_ids.contains(sid))
        });

        for sid in session_ids {
            self.browser_sessions.remove(&sid);
            self.emit_revocation(AuthRevocation::Session {
                session_id: sid,
                resource_owners: Vec::new(),
            });
        }

        all_owners.sort();
        all_owners.dedup();
        self.emit_revocation(AuthRevocation::Token {
            token_id: token_id.clone(),
            resource_owners: all_owners,
        });
    }

    fn prune_expired(&mut self) {
        let now_unix = self.clock.now_unix_ms();
        let now_mono = self.clock.monotonic_ms();

        self.launch_codes
            .retain(|lc| now_mono < lc.expires_at_monotonic_ms);

        let newly_expired_tokens: Vec<TokenId> = self
            .tokens
            .iter_mut()
            .filter_map(|(id, live)| {
                if !live.expired_notified && live.is_expired_at(now_unix) {
                    live.expired_notified = true;
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        for token_id in newly_expired_tokens {
            self.invalidate_token_runtime(&token_id);
        }

        let expired_sessions: Vec<SessionId> = self
            .browser_sessions
            .iter()
            .filter_map(|(id, session)| {
                let backing_invalid = session.backing_token_id.as_ref().is_some_and(|tid| {
                    self.pending_revocations.contains(tid)
                        || !self.tokens.contains_key(tid)
                        || self
                            .tokens
                            .get(tid)
                            .is_some_and(|live| live.is_expired_at(now_unix))
                });
                if backing_invalid || session.is_expired_at(now_mono, now_unix) {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        for sid in expired_sessions {
            self.invalidate_session(&sid);
        }

        let expired_resources: Vec<ResourceSessionId> = self
            .resource_sessions
            .iter()
            .filter(|(_, record)| record.is_expired_at(now_mono))
            .map(|(id, _)| id.clone())
            .collect();
        for rid in expired_resources {
            if let Some(record) = self.resource_sessions.remove(&rid) {
                record.lease_token.cancel();
                self.emit_revocation(AuthRevocation::ResourceSession {
                    resource_session_id: rid,
                    owner_id: record.owner_id,
                });
            }
        }
    }

    fn next_expiry_delay(&self) -> Option<Duration> {
        let now_unix = self.clock.now_unix_ms();
        let now_mono = self.clock.monotonic_ms();
        let mut earliest_ms: Option<u64> = None;

        let mut consider = |delta_ms: u64| {
            earliest_ms = Some(match earliest_ms {
                Some(cur) => cur.min(delta_ms),
                None => delta_ms,
            });
        };

        for live in self.tokens.values() {
            if live.expired_notified {
                continue;
            }
            if let Some(exp) = live.record.expires_at_ms {
                let delta = if exp <= now_unix {
                    0
                } else {
                    exp.abs_diff(now_unix)
                };
                // Cap UTC deadline waits so forward wall-clock jumps trigger prompt invalidation.
                consider(delta.min(self.policy.wall_expiry_recheck_ms));
            }
        }
        for lc in &self.launch_codes {
            consider(lc.expires_at_monotonic_ms.saturating_sub(now_mono));
        }
        for session in self.browser_sessions.values() {
            consider(
                session
                    .idle_expires_at_monotonic_ms
                    .saturating_sub(now_mono),
            );
            if let Some(exp) = session.token_expires_at_ms {
                let delta = if exp <= now_unix {
                    0
                } else {
                    exp.abs_diff(now_unix)
                };
                consider(delta.min(self.policy.wall_expiry_recheck_ms));
            }
        }
        for record in self.resource_sessions.values() {
            consider(record.idle_expires_at_monotonic_ms.saturating_sub(now_mono));
        }

        earliest_ms.map(|ms| Duration::from_millis(ms.clamp(1, MAX_EXPIRY_SLEEP_MS)))
    }

    fn reschedule_expiry_wake(&mut self, myself: &ActorRef<AuthMessage>) {
        if let Some(prev) = self.expiry_wake.take() {
            prev.stop();
        }
        let Some(delay) = self.next_expiry_delay() else {
            return;
        };
        let cancel = CancellationToken::new();
        let wake_cancel = cancel.clone();
        let actor_ref = myself.clone();
        let task = tokio::spawn(async move {
            tokio::select! {
                () = wake_cancel.cancelled() => {}
                () = tokio::time::sleep(delay) => {
                    let _ = actor_ref.cast(AuthMessage::ExpiryTick);
                }
            }
        });
        self.expiry_wake = Some(ExpiryWakeHandle { cancel, task });
    }

    fn verify_token_constant_work(
        &mut self,
        raw_token: &SecretString,
        now_unix_ms: i64,
    ) -> Option<LiveTokenRecord> {
        let dummy_hash = SecretHash::from_bytes([0u8; 32]);
        let mut secret_bytes = Zeroizing::new([0u8; 32]);

        let Some(parsed) = parse_access_token(raw_token.expose_secret()) else {
            let candidate_hash = sha256_secret(&*secret_bytes);
            let _ = constant_time_hash_eq(&candidate_hash, &dummy_hash);
            return None;
        };

        if hex::decode_to_slice(parsed.secret_hex, &mut *secret_bytes).is_err() {
            let candidate_hash = sha256_secret(&*secret_bytes);
            let _ = constant_time_hash_eq(&candidate_hash, &dummy_hash);
            return None;
        }
        let candidate_hash = sha256_secret(&*secret_bytes);

        let Some(live) = self.tokens.get_mut(&parsed.id) else {
            let _ = constant_time_hash_eq(&candidate_hash, &dummy_hash);
            return None;
        };

        let matches = constant_time_hash_eq(&live.record.token_hash, &candidate_hash);
        if !matches
            || self.pending_revocations.contains(&parsed.id)
            || live.is_expired_at(now_unix_ms)
        {
            return None;
        }
        live.record.last_used_at_ms = Some(now_unix_ms);
        Some(live.clone())
    }

    fn verify_browser_session_constant_work(
        &mut self,
        raw_cookie: &SecretString,
        now_mono_ms: u64,
        now_unix_ms: i64,
    ) -> Option<BrowserSessionRecord> {
        let candidate_hash = sha256_secret(raw_cookie.expose_secret().as_bytes());
        let dummy_hash = SecretHash::from_bytes([0u8; 32]);

        let Some(session_id) = parse_session_cookie(raw_cookie.expose_secret()) else {
            let _ = constant_time_hash_eq(&candidate_hash, &dummy_hash);
            return None;
        };

        let Some(session) = self.browser_sessions.get(&session_id) else {
            let _ = constant_time_hash_eq(&candidate_hash, &dummy_hash);
            return None;
        };

        let matches = constant_time_hash_eq(&session.cookie_hash, &candidate_hash);
        if !matches || session.is_expired_at(now_mono_ms, now_unix_ms) {
            return None;
        }
        if let Some(tid) = &session.backing_token_id {
            let token_valid = !self.pending_revocations.contains(tid)
                && self
                    .tokens
                    .get(tid)
                    .is_some_and(|tok| !tok.is_expired_at(now_unix_ms));
            if !token_valid {
                return None;
            }
        }

        let session = self.browser_sessions.get_mut(&session_id)?;
        session.last_active_at_ms = now_unix_ms;
        session.idle_expires_at_monotonic_ms =
            now_mono_ms.saturating_add(self.policy.browser_session_idle_ms);
        Some(session.clone())
    }

    fn revalidate_caller_principal(
        &mut self,
        caller: &AuthContext,
    ) -> Result<(Principal, Option<TokenId>, Option<SessionId>), RemoteAuthError> {
        let now_unix = self.clock.now_unix_ms();
        let now_mono = self.clock.monotonic_ms();

        match &caller.principal {
            Principal::Token { id, name } => {
                let tid = TokenId::parse(id.as_str()).map_err(|_| RemoteAuthError::Unauthorized)?;
                if self.pending_revocations.contains(&tid) {
                    return Err(RemoteAuthError::Unauthorized);
                }
                let live = self.tokens.get(&tid).ok_or(RemoteAuthError::Unauthorized)?;
                if live.is_expired_at(now_unix) || live.record.name.as_str() != name {
                    return Err(RemoteAuthError::Unauthorized);
                }
                Ok((
                    Principal::Token {
                        id: live.record.id.to_string(),
                        name: live.record.name.to_string(),
                    },
                    Some(tid),
                    None,
                ))
            }
            Principal::Session { id } => {
                let sid =
                    SessionId::parse(id.as_str()).map_err(|_| RemoteAuthError::Unauthorized)?;
                let (backing_token_id, expired) = {
                    let session = self
                        .browser_sessions
                        .get(&sid)
                        .ok_or(RemoteAuthError::Unauthorized)?;
                    (
                        session.backing_token_id.clone(),
                        session.is_expired_at(now_mono, now_unix),
                    )
                };
                if expired {
                    return Err(RemoteAuthError::Unauthorized);
                }
                if let Some(tid) = &backing_token_id {
                    let token_ok = !self.pending_revocations.contains(tid)
                        && self
                            .tokens
                            .get(tid)
                            .is_some_and(|tok| !tok.is_expired_at(now_unix));
                    if !token_ok {
                        return Err(RemoteAuthError::Unauthorized);
                    }
                }
                let session = self
                    .browser_sessions
                    .get_mut(&sid)
                    .ok_or(RemoteAuthError::Unauthorized)?;
                session.last_active_at_ms = now_unix;
                session.idle_expires_at_monotonic_ms =
                    now_mono.saturating_add(self.policy.browser_session_idle_ms);
                Ok((
                    Principal::Session {
                        id: session.id.to_string(),
                    },
                    backing_token_id,
                    Some(sid),
                ))
            }
            // Desktop, LocalSocket, and Anonymous callers come strictly from trusted Rust composition.
            Principal::Desktop { window } => {
                if window.is_empty() {
                    return Err(RemoteAuthError::Unauthorized);
                }
                Ok((caller.principal.clone(), None, None))
            }
            Principal::LocalSocket { user } => {
                if user.is_empty() {
                    return Err(RemoteAuthError::Unauthorized);
                }
                Ok((caller.principal.clone(), None, None))
            }
            Principal::Anonymous => Ok((Principal::Anonymous, None, None)),
        }
    }

    fn build_browser_session(
        &mut self,
        scopes: ScopeSet,
        backing_token_id: Option<TokenId>,
        token_expires_at_ms: Option<i64>,
    ) -> Result<EstablishedBrowserSession, RemoteAuthError> {
        if self.browser_sessions.len() >= self.policy.max_browser_sessions {
            return Err(RemoteAuthError::CapacityExceeded {
                resource: "browser_sessions",
            });
        }
        let now_unix = self.clock.now_unix_ms();
        let now_mono = self.clock.monotonic_ms();
        let (raw_id, cookie, cookie_hash) =
            mint_prefixed_cookie_credential(self.entropy.as_ref(), "nps", "sess_")?;
        let id = SessionId::parse(raw_id).map_err(|err| RemoteAuthError::EntropyFailed {
            reason: err.to_string(),
        })?;
        let generation = self.bump_generation();
        let record = BrowserSessionRecord {
            id: id.clone(),
            cookie_hash,
            backing_token_id,
            scopes,
            created_at_ms: now_unix,
            last_active_at_ms: now_unix,
            idle_expires_at_monotonic_ms: now_mono
                .saturating_add(self.policy.browser_session_idle_ms),
            token_expires_at_ms,
            generation,
        };
        let auth = record.auth_context();
        let summary = record.summary();
        self.browser_sessions.insert(id, record);
        Ok(EstablishedBrowserSession {
            summary,
            auth,
            auth_generation: generation,
            cookie,
        })
    }
}

struct RemoteAccessAuthActor;

impl Actor for RemoteAccessAuthActor {
    type Msg = AuthMessage;
    type State = AuthActorState;
    type Arguments = AuthStartupArgs;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let mut tokens = BTreeMap::new();
        let mut next_generation: u64 = 1;
        for record in args.persisted.tokens {
            let generation = next_generation;
            next_generation = next_generation.saturating_add(1);
            tokens.insert(
                record.id.clone(),
                LiveTokenRecord {
                    record,
                    generation,
                    expired_notified: false,
                },
            );
        }

        let mut state = AuthActorState {
            policy: args.ports.policy,
            clock: args.ports.clock,
            entropy: args.ports.entropy,
            store: args.ports.store,
            cipher: args.ports.cipher,
            revocations: args.revocations,
            tokens,
            pending_revocations: BTreeSet::new(),
            launch_codes: Vec::new(),
            browser_sessions: BTreeMap::new(),
            resource_sessions: BTreeMap::new(),
            rate_limiter: PeerRateLimiter::new(args.ports.policy.rate_limit),
            tls_material: args.persisted.tls_material,
            next_generation,
            expiry_wake: None,
        };
        state.prune_expired();
        state.reschedule_expiry_wake(&myself);
        Ok(state)
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(wake) = state.expiry_wake.take() {
            wake.stop();
        }
        let all_owners: Vec<ResourceSessionId> = state.resource_sessions.keys().cloned().collect();
        for rid in all_owners {
            if let Some(rec) = state.resource_sessions.remove(&rid) {
                rec.lease_token.cancel();
                state.emit_revocation(AuthRevocation::ResourceSession {
                    resource_session_id: rid,
                    owner_id: rec.owner_id,
                });
            }
        }
        state.browser_sessions.clear();
        state.launch_codes.clear();
        Ok(())
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.prune_expired();

        match message {
            AuthMessage::ExpiryTick => {
                // `prune_expired` above already invalidated newly expired tokens/sessions/leases.
            }
            AuthMessage::CreateAccessToken { request, reply } => {
                let res = handle_create_token(state, request).await;
                let _ = reply.send(res);
            }
            AuthMessage::ListAccessTokens { reply } => {
                let now_unix = state.clock.now_unix_ms();
                let list = state
                    .tokens
                    .values()
                    .filter(|live| !state.pending_revocations.contains(&live.record.id))
                    .map(|live| live.summary(now_unix))
                    .collect();
                let _ = reply.send(Ok(list));
            }
            AuthMessage::RevokeAccessToken { id, reply } => {
                let res = handle_revoke_token(state, id).await;
                let _ = reply.send(res);
            }
            AuthMessage::AuthenticateToken { peer, token, reply } => {
                let res = handle_authenticate_token(state, peer, &token);
                let _ = reply.send(res);
            }
            AuthMessage::IssueLaunchCode { scopes, reply } => {
                let res = handle_issue_launch_code(state, scopes);
                let _ = reply.send(res);
            }
            AuthMessage::ExchangeLaunchCode { peer, code, reply } => {
                let res = handle_exchange_launch_code(state, peer, &code);
                let _ = reply.send(res);
            }
            AuthMessage::LoginWithToken { peer, token, reply } => {
                let res = handle_login_with_token(state, peer, &token);
                let _ = reply.send(res);
            }
            AuthMessage::ResolveBrowserSession {
                peer,
                cookie,
                reply,
            } => {
                let res = handle_resolve_browser_session(state, peer, &cookie);
                let _ = reply.send(res);
            }
            AuthMessage::ListBrowserSessions { reply } => {
                let list = state
                    .browser_sessions
                    .values()
                    .map(BrowserSessionRecord::summary)
                    .collect();
                let _ = reply.send(Ok(list));
            }
            AuthMessage::RevokeBrowserSession { id, reply } => {
                let res = if state.browser_sessions.contains_key(&id) {
                    state.invalidate_session(&id);
                    Ok(())
                } else {
                    Err(RemoteAuthError::NotFound)
                };
                let _ = reply.send(res);
            }
            AuthMessage::AdmitCaller { caller, reply } => {
                let res = handle_admit_caller(state, &caller);
                let _ = reply.send(res);
            }
            AuthMessage::CreateResourceSession { caller, reply } => {
                let res = handle_create_resource_session(state, &caller);
                let _ = reply.send(res);
            }
            AuthMessage::ResolveResourceSession {
                caller,
                cookie,
                reply,
            } => {
                let res = handle_resolve_resource_session(state, &caller, &cookie);
                let _ = reply.send(res);
            }
            AuthMessage::ReleaseResourceSession { id, reply } => {
                let res = if let Some(record) = state.resource_sessions.remove(&id) {
                    record.lease_token.cancel();
                    state.emit_revocation(AuthRevocation::ResourceSession {
                        resource_session_id: id,
                        owner_id: record.owner_id,
                    });
                    Ok(())
                } else {
                    Err(RemoteAuthError::NotFound)
                };
                let _ = reply.send(res);
            }
            AuthMessage::SetTlsMaterial { blob, reply } => {
                let res = handle_set_tls_material(state, blob).await;
                let _ = reply.send(res);
            }
            AuthMessage::TlsMaterialStatus { reply } => {
                let status = state.tls_material.as_ref().map(|blob| TlsMaterialSummary {
                    fingerprint_sha256: blob.fingerprint_sha256.clone(),
                    expires_at_ms: blob.expires_at_ms,
                });
                let _ = reply.send(Ok(status));
            }
            AuthMessage::LoadTlsMaterial { reply } => {
                let _ = reply.send(Ok(state.tls_material.clone()));
            }
        }

        state.reschedule_expiry_wake(&myself);
        Ok(())
    }
}

async fn handle_create_token(
    state: &mut AuthActorState,
    request: CreateAccessTokenRequest,
) -> Result<IssuedAccessToken, RemoteAuthError> {
    if state.tokens.len() >= state.policy.max_tokens {
        return Err(RemoteAuthError::CapacityExceeded { resource: "tokens" });
    }
    let now_unix = state.clock.now_unix_ms();
    if let Some(exp) = request.expires_at_ms
        && exp <= now_unix
    {
        return Err(RemoteAuthError::InvalidInput {
            reason: "token expires_at_ms must be in the future".to_owned(),
        });
    }

    let (id, raw_token, token_hash) = mint_token_credential(state.entropy.as_ref())?;
    let generation = state.bump_generation();
    let record = PersistedTokenRecord {
        id: id.clone(),
        name: request.name,
        scopes: request.scopes,
        token_hash,
        created_at_ms: now_unix,
        expires_at_ms: request.expires_at_ms,
        last_used_at_ms: None,
    };

    // Persist BEFORE committing new token to memory; also carries forward any pending revocations.
    let mut candidate_snapshot = state.persisted_tokens_snapshot();
    candidate_snapshot.push(record.clone());
    save_persisted_state(
        state.store.as_ref(),
        state.cipher.as_deref(),
        &candidate_snapshot,
        state.tls_material.as_ref(),
        state.policy.max_persisted_record_bytes,
    )
    .await?;

    state.pending_revocations.clear();
    let live = LiveTokenRecord {
        record,
        generation,
        expired_notified: false,
    };
    let summary = live.summary(now_unix);
    state.tokens.insert(id, live);
    Ok(IssuedAccessToken {
        summary,
        token: raw_token,
    })
}

async fn handle_revoke_token(
    state: &mut AuthActorState,
    id: TokenId,
) -> Result<(), RemoteAuthError> {
    let was_present = state.tokens.contains_key(&id);
    let was_pending = state.pending_revocations.contains(&id);
    if !was_present && !was_pending {
        return Err(RemoteAuthError::NotFound);
    }

    if was_present {
        state.tokens.remove(&id);
        state.pending_revocations.insert(id.clone());
        state.invalidate_token_runtime(&id);
    }

    let candidate_snapshot = state.persisted_tokens_snapshot();
    match save_persisted_state(
        state.store.as_ref(),
        state.cipher.as_deref(),
        &candidate_snapshot,
        state.tls_material.as_ref(),
        state.policy.max_persisted_record_bytes,
    )
    .await
    {
        Ok(()) => {
            state.pending_revocations.clear();
            Ok(())
        }
        Err(err) => Err(RemoteAuthError::RevocationPersistenceFailed {
            reason: err.to_string(),
        }),
    }
}

fn handle_authenticate_token(
    state: &mut AuthActorState,
    peer: AuthPeer,
    raw_token: &SecretString,
) -> Result<AuthenticatedCaller, RemoteAuthError> {
    let now_mono = state.clock.monotonic_ms();
    let now_unix = state.clock.now_unix_ms();

    if let Err(retry_after_ms) = state.rate_limiter.check_allowed(peer, now_mono) {
        return Err(RemoteAuthError::RateLimited { retry_after_ms });
    }

    let Some(live) = state.verify_token_constant_work(raw_token, now_unix) else {
        state.rate_limiter.record_failure(peer, now_mono);
        return Err(RemoteAuthError::Unauthorized);
    };

    state.rate_limiter.record_success(peer);
    Ok(AuthenticatedCaller {
        auth: AuthContext::new(
            Principal::Token {
                id: live.record.id.to_string(),
                name: live.record.name.to_string(),
            },
            live.record.scopes.clone(),
        ),
        expires_at_ms: live.record.expires_at_ms,
        auth_generation: live.generation,
    })
}

fn handle_issue_launch_code(
    state: &mut AuthActorState,
    scopes: ScopeSet,
) -> Result<IssuedLaunchCode, RemoteAuthError> {
    if state.launch_codes.len() >= state.policy.max_launch_codes {
        return Err(RemoteAuthError::CapacityExceeded {
            resource: "launch_codes",
        });
    }
    let now_unix = state.clock.now_unix_ms();
    let now_mono = state.clock.monotonic_ms();
    let ttl_ms = state.policy.launch_code_ttl_ms;
    let (code, code_hash) = mint_launch_code_credential(state.entropy.as_ref())?;
    let expires_at_ms = now_unix.saturating_add(i64::try_from(ttl_ms).unwrap_or(i64::MAX));

    state.launch_codes.push(LaunchCodeRecord {
        code_hash,
        scopes: scopes.clone(),
        issued_at_ms: now_unix,
        expires_at_monotonic_ms: now_mono.saturating_add(ttl_ms),
    });

    Ok(IssuedLaunchCode {
        code,
        scopes,
        issued_at_ms: now_unix,
        expires_at_ms,
    })
}

fn handle_exchange_launch_code(
    state: &mut AuthActorState,
    peer: AuthPeer,
    raw_code: &SecretString,
) -> Result<EstablishedBrowserSession, RemoteAuthError> {
    let now_mono = state.clock.monotonic_ms();
    if let Err(retry_after_ms) = state.rate_limiter.check_allowed(peer, now_mono) {
        return Err(RemoteAuthError::RateLimited { retry_after_ms });
    }

    let candidate_hash = sha256_secret(raw_code.expose_secret().as_bytes());
    let dummy_hash = SecretHash::from_bytes([0u8; 32]);
    let syntax_ok = is_valid_launch_code_syntax(raw_code.expose_secret());

    let mut matched_idx: Option<usize> = None;
    for (idx, record) in state.launch_codes.iter().enumerate() {
        let is_match = constant_time_hash_eq(&record.code_hash, &candidate_hash);
        if is_match && syntax_ok && now_mono < record.expires_at_monotonic_ms {
            matched_idx = Some(idx);
        }
    }
    if matched_idx.is_none() {
        let _ = constant_time_hash_eq(&candidate_hash, &dummy_hash);
        state.rate_limiter.record_failure(peer, now_mono);
        return Err(RemoteAuthError::Unauthorized);
    }

    if state.browser_sessions.len() >= state.policy.max_browser_sessions {
        return Err(RemoteAuthError::CapacityExceeded {
            resource: "browser_sessions",
        });
    }

    let idx = matched_idx.expect("checked Some above");
    let consumed = state.launch_codes.swap_remove(idx);
    state.rate_limiter.record_success(peer);
    state.build_browser_session(consumed.scopes.clone(), None, None)
}

fn handle_login_with_token(
    state: &mut AuthActorState,
    peer: AuthPeer,
    raw_token: &SecretString,
) -> Result<EstablishedBrowserSession, RemoteAuthError> {
    let now_mono = state.clock.monotonic_ms();
    let now_unix = state.clock.now_unix_ms();

    if let Err(retry_after_ms) = state.rate_limiter.check_allowed(peer, now_mono) {
        return Err(RemoteAuthError::RateLimited { retry_after_ms });
    }

    let Some(live) = state.verify_token_constant_work(raw_token, now_unix) else {
        state.rate_limiter.record_failure(peer, now_mono);
        return Err(RemoteAuthError::Unauthorized);
    };

    if state.browser_sessions.len() >= state.policy.max_browser_sessions {
        return Err(RemoteAuthError::CapacityExceeded {
            resource: "browser_sessions",
        });
    }

    state.rate_limiter.record_success(peer);
    state.build_browser_session(
        live.record.scopes.clone(),
        Some(live.record.id.clone()),
        live.record.expires_at_ms,
    )
}

fn handle_resolve_browser_session(
    state: &mut AuthActorState,
    peer: AuthPeer,
    cookie: &SecretString,
) -> Result<AuthenticatedCaller, RemoteAuthError> {
    let now_mono = state.clock.monotonic_ms();
    let now_unix = state.clock.now_unix_ms();

    if let Err(retry_after_ms) = state.rate_limiter.check_allowed(peer, now_mono) {
        return Err(RemoteAuthError::RateLimited { retry_after_ms });
    }

    let Some(session) = state.verify_browser_session_constant_work(cookie, now_mono, now_unix)
    else {
        state.rate_limiter.record_failure(peer, now_mono);
        return Err(RemoteAuthError::Unauthorized);
    };

    state.rate_limiter.record_success(peer);
    Ok(AuthenticatedCaller {
        auth: session.auth_context(),
        expires_at_ms: session.token_expires_at_ms,
        auth_generation: session.generation,
    })
}

fn handle_admit_caller(
    state: &mut AuthActorState,
    caller: &AuthenticatedCaller,
) -> Result<AuthenticatedCaller, RemoteAuthError> {
    let now_unix = state.clock.now_unix_ms();
    let now_mono = state.clock.monotonic_ms();

    match &caller.auth.principal {
        Principal::Token { id, name } => {
            let tid = TokenId::parse(id.as_str()).map_err(|_| RemoteAuthError::Unauthorized)?;
            if state.pending_revocations.contains(&tid) {
                return Err(RemoteAuthError::Unauthorized);
            }
            let live = state
                .tokens
                .get(&tid)
                .ok_or(RemoteAuthError::Unauthorized)?;
            if live.is_expired_at(now_unix)
                || live.record.name.as_str() != name
                || live.generation != caller.auth_generation
            {
                return Err(RemoteAuthError::Unauthorized);
            }
            Ok(AuthenticatedCaller {
                auth: AuthContext::new(
                    Principal::Token {
                        id: live.record.id.to_string(),
                        name: live.record.name.to_string(),
                    },
                    live.record.scopes.clone(),
                ),
                expires_at_ms: live.record.expires_at_ms,
                auth_generation: live.generation,
            })
        }
        Principal::Session { id } => {
            let sid = SessionId::parse(id.as_str()).map_err(|_| RemoteAuthError::Unauthorized)?;
            let (backing_tid, expired, generation) = {
                let session = state
                    .browser_sessions
                    .get(&sid)
                    .ok_or(RemoteAuthError::Unauthorized)?;
                (
                    session.backing_token_id.clone(),
                    session.is_expired_at(now_mono, now_unix),
                    session.generation,
                )
            };
            if expired || generation != caller.auth_generation {
                return Err(RemoteAuthError::Unauthorized);
            }
            if let Some(tid) = &backing_tid {
                let token_ok = !state.pending_revocations.contains(tid)
                    && state
                        .tokens
                        .get(tid)
                        .is_some_and(|tok| !tok.is_expired_at(now_unix));
                if !token_ok {
                    return Err(RemoteAuthError::Unauthorized);
                }
            }
            let session = state
                .browser_sessions
                .get_mut(&sid)
                .ok_or(RemoteAuthError::Unauthorized)?;
            session.last_active_at_ms = now_unix;
            session.idle_expires_at_monotonic_ms =
                now_mono.saturating_add(state.policy.browser_session_idle_ms);
            Ok(AuthenticatedCaller {
                auth: session.auth_context(),
                expires_at_ms: session.token_expires_at_ms,
                auth_generation: session.generation,
            })
        }
        Principal::Desktop { window } => {
            if window.is_empty() {
                return Err(RemoteAuthError::Unauthorized);
            }
            Ok(caller.clone())
        }
        Principal::LocalSocket { user } => {
            if user.is_empty() {
                return Err(RemoteAuthError::Unauthorized);
            }
            Ok(caller.clone())
        }
        Principal::Anonymous => Ok(caller.clone()),
    }
}

fn handle_create_resource_session(
    state: &mut AuthActorState,
    caller: &AuthContext,
) -> Result<EstablishedResourceSession, RemoteAuthError> {
    let (principal, backing_token_id, backing_session_id) =
        state.revalidate_caller_principal(caller)?;

    if state.resource_sessions.len() >= state.policy.max_resource_sessions_global {
        return Err(RemoteAuthError::CapacityExceeded {
            resource: "resource_sessions_global",
        });
    }

    let bucket = principal.audit_label();
    let per_principal_count = state
        .resource_sessions
        .values()
        .filter(|rec| rec.principal_bucket == bucket)
        .count();
    if per_principal_count >= state.policy.max_resource_sessions_per_principal {
        return Err(RemoteAuthError::CapacityExceeded {
            resource: "resource_sessions_per_principal",
        });
    }

    let (raw_id, cookie, cookie_hash) =
        mint_prefixed_cookie_credential(state.entropy.as_ref(), "npr", "res_")?;
    let id = ResourceSessionId::parse(raw_id).map_err(|err| RemoteAuthError::EntropyFailed {
        reason: err.to_string(),
    })?;
    let owner_id = ResourceOwnerId::parse(format!("http_res:{id}")).map_err(|err| {
        RemoteAuthError::EntropyFailed {
            reason: err.to_string(),
        }
    })?;

    let now_mono = state.clock.monotonic_ms();
    let lease_token = CancellationToken::new();
    let lease = ResourceLease::new(owner_id.clone(), lease_token.clone());
    let record = ResourceSessionRecord {
        id: id.clone(),
        owner_id: owner_id.clone(),
        cookie_hash,
        principal: principal.clone(),
        principal_bucket: bucket,
        backing_token_id,
        backing_session_id,
        idle_expires_at_monotonic_ms: now_mono
            .saturating_add(state.policy.resource_session_idle_ms),
        lease_token,
    };
    state.resource_sessions.insert(id.clone(), record);

    Ok(EstablishedResourceSession {
        id,
        owner_id,
        principal,
        lease,
        cookie,
    })
}

fn handle_resolve_resource_session(
    state: &mut AuthActorState,
    caller: &AuthContext,
    cookie: &SecretString,
) -> Result<ResolvedResourceSession, RemoteAuthError> {
    let (verified_principal, _, _) = state.revalidate_caller_principal(caller)?;
    let now_mono = state.clock.monotonic_ms();
    let idle_ttl_ms = state.policy.resource_session_idle_ms;
    let candidate_hash = sha256_secret(cookie.expose_secret().as_bytes());
    let dummy_hash = SecretHash::from_bytes([0u8; 32]);

    let Some(res_id) = parse_resource_cookie(cookie.expose_secret()) else {
        let _ = constant_time_hash_eq(&candidate_hash, &dummy_hash);
        return Err(RemoteAuthError::Unauthorized);
    };

    let Some(record) = state.resource_sessions.get_mut(&res_id) else {
        let _ = constant_time_hash_eq(&candidate_hash, &dummy_hash);
        return Err(RemoteAuthError::Unauthorized);
    };

    let hash_matches = constant_time_hash_eq(&record.cookie_hash, &candidate_hash);
    if !hash_matches || record.principal != verified_principal || record.is_expired_at(now_mono) {
        return Err(RemoteAuthError::Unauthorized);
    }

    // Refresh idle deadline ONLY after verifying both secret cookie hash and bound principal.
    record.refresh_idle(now_mono, idle_ttl_ms);

    Ok(ResolvedResourceSession {
        id: record.id.clone(),
        owner_id: record.owner_id.clone(),
        principal: record.principal.clone(),
        lease: ResourceLease::new(record.owner_id.clone(), record.lease_token.clone()),
    })
}

async fn handle_set_tls_material(
    state: &mut AuthActorState,
    blob: TlsMaterialBlob,
) -> Result<TlsMaterialSummary, RemoteAuthError> {
    validate_tls_blob(&blob)?;
    let snapshot_tokens = state.persisted_tokens_snapshot();
    save_persisted_state(
        state.store.as_ref(),
        state.cipher.as_deref(),
        &snapshot_tokens,
        Some(&blob),
        state.policy.max_persisted_record_bytes,
    )
    .await?;

    state.pending_revocations.clear();
    let summary = TlsMaterialSummary {
        fingerprint_sha256: blob.fingerprint_sha256.clone(),
        expires_at_ms: blob.expires_at_ms,
    };
    state.tls_material = Some(blob);
    Ok(summary)
}
