//! Behavior of the authentication owner through its typed client, over a paused tokio clock,
//! a deterministic entropy source and an in-memory secret store.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use nyanpasu_config::application::Scope;
use nyanpasu_contracts::auth::{
    AccessTokenStatus, AuthClock, AuthRevocation, CreateAccessTokenRequest, EntropyError,
    EntropySource, Principal, RemoteAuthPolicy, ScopeSet, SecretStore, SecretStoreError,
    SecretString, TokenName,
};
use parking_lot::Mutex;
use tokio::{sync::broadcast, time::Instant};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use zeroize::Zeroizing;

use super::{RemoteAccessAuthClient, RemoteAccessAuthPorts, RemoteAuthError};

const START_UNIX_MS: i64 = 1_700_000_000_000;
const MINUTE: Duration = Duration::from_secs(60);

/// Time that only moves when the paused tokio clock does.
struct PausedClock {
    start: Instant,
}

impl AuthClock for PausedClock {
    fn now_unix_ms(&self) -> i64 {
        START_UNIX_MS + i64::try_from(self.monotonic_ms()).unwrap()
    }

    fn monotonic_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap()
    }
}

/// Distinct, reproducible bytes on every fill.
struct CountingEntropy(AtomicU8);

impl EntropySource for CountingEntropy {
    fn fill_bytes(&self, dest: &mut [u8]) -> Result<(), EntropyError> {
        let seed = self.0.fetch_add(1, Ordering::SeqCst);
        for (offset, byte) in dest.iter_mut().enumerate() {
            *byte = seed
                .wrapping_mul(31)
                .wrapping_add(u8::try_from(offset % 251).unwrap());
        }
        Ok(())
    }
}

#[derive(Default)]
struct MemoryStore {
    record: Mutex<Option<Vec<u8>>>,
    fail_saves: AtomicBool,
}

#[async_trait]
impl SecretStore for MemoryStore {
    async fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        Ok(self.record.lock().clone().map(Zeroizing::new))
    }

    async fn save(&self, bytes: &[u8]) -> Result<(), SecretStoreError> {
        if self.fail_saves.load(Ordering::SeqCst) {
            return Err(SecretStoreError::Io {
                reason: "disk full".to_owned(),
            });
        }
        *self.record.lock() = Some(bytes.to_vec());
        Ok(())
    }
}

struct Harness {
    client: RemoteAccessAuthClient,
    store: Arc<MemoryStore>,
    revocations: broadcast::Receiver<AuthRevocation>,
}

impl Harness {
    async fn start() -> Self {
        Self::start_with(RemoteAuthPolicy::default(), Arc::default()).await
    }

    async fn start_with(policy: RemoteAuthPolicy, store: Arc<MemoryStore>) -> Self {
        let client = Self::spawn(policy, &store).await.unwrap();
        let revocations = client.subscribe_revocations();
        Self {
            client,
            store,
            revocations,
        }
    }

    async fn spawn(
        policy: RemoteAuthPolicy,
        store: &Arc<MemoryStore>,
    ) -> Result<RemoteAccessAuthClient, RemoteAuthError> {
        let ports = RemoteAccessAuthPorts {
            clock: Arc::new(PausedClock {
                start: Instant::now(),
            }),
            entropy: Arc::new(CountingEntropy(AtomicU8::new(1))),
            store: store.clone(),
            policy,
        };
        RemoteAccessAuthClient::spawn(ports, CancellationToken::new(), &TaskTracker::new()).await
    }

    async fn create_token(&self, expires_in: Option<Duration>) -> SecretString {
        let request = CreateAccessTokenRequest {
            name: TokenName::parse("laptop-cli").unwrap(),
            scopes: ScopeSet::from([Scope::Read, Scope::Proxy]),
            expires_at_ms: expires_in
                .map(|ttl| START_UNIX_MS + i64::try_from(ttl.as_millis()).unwrap()),
        };
        self.client
            .create_access_token(request)
            .await
            .unwrap()
            .token
    }
}

fn tampered(raw: &SecretString) -> SecretString {
    let mut text = raw.expose_secret().to_owned();
    let last = text.pop().unwrap();
    text.push(if last == '0' { '1' } else { '0' });
    SecretString::new(text)
}

#[tokio::test(start_paused = true)]
async fn token_is_minted_verified_and_revoked() {
    let harness = Harness::start().await;
    let token = harness.create_token(None).await;

    let listed = harness.client.list_access_tokens().await.unwrap();
    let caller = harness
        .client
        .authenticate_token(token.clone())
        .await
        .unwrap();
    assert_eq!(
        caller.auth.principal,
        Principal::Token {
            id: listed[0].id.to_string(),
            name: "laptop-cli".to_owned()
        }
    );
    assert!(caller.auth.has_scope(Scope::Proxy));
    assert!(!caller.auth.has_scope(Scope::System));

    assert_eq!(
        harness
            .client
            .authenticate_token(tampered(&token))
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert_eq!(
        harness
            .client
            .authenticate_token(SecretString::new("npk_nothing"))
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );

    // The record holds a hash, never the secret.
    let stored = String::from_utf8(harness.store.record.lock().clone().unwrap()).unwrap();
    let secret = token.expose_secret().rsplit('_').next().unwrap();
    assert!(!stored.contains(secret));

    let listed = harness.client.list_access_tokens().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].status, AccessTokenStatus::Active);
    assert!(listed[0].last_used_at_ms.is_some());

    harness
        .client
        .revoke_access_token(listed[0].id.clone())
        .await
        .unwrap();
    assert_eq!(
        harness.client.authenticate_token(token).await.unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert_eq!(
        harness
            .client
            .revoke_access_token(listed[0].id.clone())
            .await
            .unwrap_err(),
        RemoteAuthError::NotFound
    );
}

#[tokio::test(start_paused = true)]
async fn tokens_survive_a_restart_and_expire() {
    let harness = Harness::start().await;
    let durable = harness.create_token(None).await;
    let short_lived = harness.create_token(Some(10 * MINUTE)).await;

    let restarted = Harness::start_with(RemoteAuthPolicy::default(), harness.store.clone())
        .await
        .client;
    assert!(restarted.authenticate_token(durable.clone()).await.is_ok());
    assert!(
        restarted
            .authenticate_token(short_lived.clone())
            .await
            .is_ok()
    );

    tokio::time::advance(11 * MINUTE).await;
    assert_eq!(
        restarted.authenticate_token(short_lived).await.unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert!(restarted.authenticate_token(durable).await.is_ok());
    let statuses: Vec<_> = restarted
        .list_access_tokens()
        .await
        .unwrap()
        .into_iter()
        .map(|token| token.status)
        .collect();
    assert!(statuses.contains(&AccessTokenStatus::Expired));
}

#[tokio::test(start_paused = true)]
async fn token_creation_rejects_a_past_expiry_and_the_capacity_limit() {
    let policy = RemoteAuthPolicy {
        max_tokens: 1,
        ..RemoteAuthPolicy::default()
    };
    let harness = Harness::start_with(policy, Arc::default()).await;
    let request = |expires_at_ms| CreateAccessTokenRequest {
        name: TokenName::parse("t").unwrap(),
        scopes: ScopeSet::empty(),
        expires_at_ms,
    };

    assert!(matches!(
        harness
            .client
            .create_access_token(request(Some(START_UNIX_MS)))
            .await,
        Err(RemoteAuthError::InvalidInput { .. })
    ));
    harness
        .client
        .create_access_token(request(None))
        .await
        .unwrap();
    assert!(matches!(
        harness.client.create_access_token(request(None)).await,
        Err(RemoteAuthError::CapacityExceeded { resource: "tokens" })
    ));
}

#[tokio::test(start_paused = true)]
async fn a_token_that_cannot_be_persisted_is_not_created() {
    let harness = Harness::start().await;
    harness.store.fail_saves.store(true, Ordering::SeqCst);

    let request = CreateAccessTokenRequest {
        name: TokenName::parse("t").unwrap(),
        scopes: ScopeSet::empty(),
        expires_at_ms: None,
    };
    assert!(matches!(
        harness.client.create_access_token(request).await,
        Err(RemoteAuthError::PersistenceFailed { .. })
    ));
    assert!(
        harness
            .client
            .list_access_tokens()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_revoke_write_is_reported_but_the_token_is_revoked() {
    let harness = Harness::start().await;
    let token = harness.create_token(None).await;
    let id = harness.client.list_access_tokens().await.unwrap()[0]
        .id
        .clone();
    harness.store.fail_saves.store(true, Ordering::SeqCst);

    assert!(matches!(
        harness.client.revoke_access_token(id).await,
        Err(RemoteAuthError::PersistenceFailed { .. })
    ));
    assert_eq!(
        harness.client.authenticate_token(token).await.unwrap_err(),
        RemoteAuthError::Unauthorized
    );
}

#[tokio::test(start_paused = true)]
async fn a_corrupt_record_fails_startup_closed() {
    let store = Arc::new(MemoryStore::default());
    *store.record.lock() = Some(b"{ not json".to_vec());

    let result = Harness::spawn(RemoteAuthPolicy::default(), &store).await;

    assert!(matches!(
        result,
        Err(RemoteAuthError::PersistenceFailed { .. })
    ));
}

#[tokio::test(start_paused = true)]
async fn launch_code_is_single_use_and_expires_after_60_seconds() {
    let harness = Harness::start().await;
    let scopes = ScopeSet::from([Scope::Read]);

    let issued = harness
        .client
        .issue_launch_code(scopes.clone())
        .await
        .unwrap();
    assert_eq!(issued.expires_at_ms - issued.issued_at_ms, 60_000);
    let session = harness
        .client
        .exchange_launch_code(issued.code.clone())
        .await
        .unwrap();
    assert_eq!(session.auth.scopes, scopes);
    assert!(matches!(session.auth.principal, Principal::Session { .. }));
    assert_eq!(
        harness
            .client
            .exchange_launch_code(issued.code)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );

    let stale = harness.client.issue_launch_code(scopes).await.unwrap();
    tokio::time::advance(Duration::from_secs(60)).await;
    assert_eq!(
        harness
            .client
            .exchange_launch_code(stale.code)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
}

#[tokio::test(start_paused = true)]
async fn session_expires_after_30_idle_minutes_and_activity_extends_it() {
    let mut harness = Harness::start().await;
    let launch = harness
        .client
        .issue_launch_code(ScopeSet::full())
        .await
        .unwrap();
    let session = harness
        .client
        .exchange_launch_code(launch.code)
        .await
        .unwrap();

    for _ in 0..3 {
        tokio::time::advance(20 * MINUTE).await;
        assert!(
            harness
                .client
                .resolve_browser_session(session.cookie.clone())
                .await
                .is_ok()
        );
    }
    assert_eq!(
        harness.client.list_browser_sessions().await.unwrap().len(),
        1
    );

    // No request arrives: the owner's own timer announces the expiry.
    assert_eq!(
        harness.revocations.recv().await.unwrap(),
        AuthRevocation::Session {
            session_id: session.summary.id.clone()
        }
    );
    assert_eq!(
        harness
            .client
            .resolve_browser_session(session.cookie)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert!(
        harness
            .client
            .list_browser_sessions()
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test(start_paused = true)]
async fn revoking_a_token_closes_its_sessions_and_announces_them() {
    let mut harness = Harness::start().await;
    let token = harness.create_token(None).await;
    let token_id = harness.client.list_access_tokens().await.unwrap()[0]
        .id
        .clone();
    let session = harness.client.login_with_token(token).await.unwrap();
    let unrelated = harness
        .client
        .exchange_launch_code(
            harness
                .client
                .issue_launch_code(ScopeSet::empty())
                .await
                .unwrap()
                .code,
        )
        .await
        .unwrap();

    harness
        .client
        .revoke_access_token(token_id.clone())
        .await
        .unwrap();

    assert_eq!(
        harness.revocations.recv().await.unwrap(),
        AuthRevocation::Token {
            token_id,
            session_ids: vec![session.summary.id.clone()]
        }
    );
    assert_eq!(
        harness
            .client
            .resolve_browser_session(session.cookie)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert!(
        harness
            .client
            .resolve_browser_session(unrelated.cookie)
            .await
            .is_ok()
    );
}

#[tokio::test(start_paused = true)]
async fn an_expiring_token_closes_its_sessions_without_a_request() {
    let mut harness = Harness::start().await;
    let token = harness.create_token(Some(10 * MINUTE)).await;
    let session = harness.client.login_with_token(token).await.unwrap();

    let AuthRevocation::Token { session_ids, .. } = harness.revocations.recv().await.unwrap()
    else {
        panic!("expected the token's expiry");
    };

    assert_eq!(session_ids, vec![session.summary.id]);
    assert_eq!(
        harness
            .client
            .resolve_browser_session(session.cookie)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
}

#[tokio::test(start_paused = true)]
async fn logout_and_revoke_close_one_session_and_the_session_limit_holds() {
    let mut harness = Harness::start_with(
        RemoteAuthPolicy {
            max_sessions: 2,
            ..RemoteAuthPolicy::default()
        },
        Arc::default(),
    )
    .await;
    let open = |harness: &Harness| {
        let client = harness.client.clone();
        async move {
            let code = client
                .issue_launch_code(ScopeSet::empty())
                .await
                .unwrap()
                .code;
            client.exchange_launch_code(code).await
        }
    };
    let first = open(&harness).await.unwrap();
    let second = open(&harness).await.unwrap();

    let spare = harness
        .client
        .issue_launch_code(ScopeSet::empty())
        .await
        .unwrap();
    assert!(matches!(
        harness
            .client
            .exchange_launch_code(spare.code.clone())
            .await,
        Err(RemoteAuthError::CapacityExceeded { .. })
    ));

    harness.client.logout(first.cookie.clone()).await.unwrap();
    assert_eq!(
        harness.revocations.recv().await.unwrap(),
        AuthRevocation::Session {
            session_id: first.summary.id.clone()
        }
    );
    assert_eq!(
        harness.client.logout(first.cookie).await.unwrap_err(),
        RemoteAuthError::Unauthorized
    );

    // The refused exchange did not spend the code.
    let third = harness
        .client
        .exchange_launch_code(spare.code)
        .await
        .unwrap();
    harness
        .client
        .revoke_browser_session(second.summary.id.clone())
        .await
        .unwrap();
    assert_eq!(
        harness
            .client
            .revoke_browser_session(second.summary.id)
            .await
            .unwrap_err(),
        RemoteAuthError::NotFound
    );
    assert!(
        harness
            .client
            .resolve_browser_session(third.cookie)
            .await
            .is_ok()
    );
}
