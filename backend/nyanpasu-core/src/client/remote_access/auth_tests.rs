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
use nyanpasu_config::application::{Access, Permission, Permissions, Resource, Role};
use nyanpasu_contracts::auth::{
    AccessTokenStatus, AuthClock, AuthRevocation, CreateAccessTokenRequest, EntropyError,
    EntropySource, Grant, Principal, RemoteAuthPolicy, SecretStore, SecretStoreError, SecretString,
    TokenName, test_support::FakeTlsGenerator,
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
    generator: Arc<FakeTlsGenerator>,
    revocations: broadcast::Receiver<AuthRevocation>,
}

impl Harness {
    async fn start() -> Self {
        Self::start_with(RemoteAuthPolicy::default(), Arc::default()).await
    }

    async fn start_with(policy: RemoteAuthPolicy, store: Arc<MemoryStore>) -> Self {
        let generator = Arc::<FakeTlsGenerator>::default();
        let client = Self::spawn(policy, &store, &generator).await.unwrap();
        let revocations = client.subscribe_revocations();
        Self {
            client,
            store,
            generator,
            revocations,
        }
    }

    async fn spawn(
        policy: RemoteAuthPolicy,
        store: &Arc<MemoryStore>,
        generator: &Arc<FakeTlsGenerator>,
    ) -> Result<RemoteAccessAuthClient, RemoteAuthError> {
        let ports = RemoteAccessAuthPorts {
            clock: Arc::new(PausedClock {
                start: Instant::now(),
            }),
            entropy: Arc::new(CountingEntropy(AtomicU8::new(1))),
            store: store.clone(),
            tls_generator: generator.clone(),
            policy,
        };
        RemoteAccessAuthClient::spawn(ports, CancellationToken::new(), &TaskTracker::new()).await
    }

    async fn create_token(&self, expires_in: Option<Duration>) -> SecretString {
        let request = CreateAccessTokenRequest {
            name: TokenName::parse("laptop-cli").unwrap(),
            grant: Grant::Custom(
                Role::Viewer
                    .permissions()
                    .with(Resource::Proxy, Access::Write),
            ),
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
    assert_eq!(caller.auth.role, None);
    assert!(
        caller
            .auth
            .permissions
            .permits(Permission::new(Resource::Proxy, Access::Write))
    );
    assert!(
        !caller
            .auth
            .permissions
            .permits(Permission::new(Resource::System, Access::Write))
    );

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
        grant: Grant::Role(Role::Viewer),
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
async fn a_token_cannot_hold_remote_or_an_unassignable_role() {
    let harness = Harness::start().await;
    for grant in [
        Grant::Custom(Permissions::NONE.with(Resource::Remote, Access::Read)),
        Grant::Role(Role::Owner),
        Grant::Role(Role::Anonymous),
    ] {
        let request = CreateAccessTokenRequest {
            name: TokenName::parse("t").unwrap(),
            grant,
            expires_at_ms: None,
        };
        assert!(
            matches!(
                harness.client.create_access_token(request).await,
                Err(RemoteAuthError::InvalidInput { .. })
            ),
            "{grant:?}"
        );
    }
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
async fn a_token_that_cannot_be_persisted_is_not_created() {
    let harness = Harness::start().await;
    harness.store.fail_saves.store(true, Ordering::SeqCst);

    let request = CreateAccessTokenRequest {
        name: TokenName::parse("t").unwrap(),
        grant: Grant::Role(Role::Viewer),
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

    let result = Harness::spawn(RemoteAuthPolicy::default(), &store, &Arc::default()).await;

    assert!(matches!(
        result,
        Err(RemoteAuthError::PersistenceFailed { .. })
    ));
}

#[tokio::test(start_paused = true)]
async fn launch_code_is_single_use_and_expires_after_60_seconds() {
    let harness = Harness::start().await;
    let issued = harness.client.issue_launch_code().await.unwrap();
    assert_eq!(issued.expires_at_ms - issued.issued_at_ms, 60_000);
    let session = harness
        .client
        .exchange_launch_code(issued.code.clone())
        .await
        .unwrap();
    assert_eq!(session.auth.role, Some(Role::Admin));
    assert_eq!(session.auth.permissions, Role::Admin.permissions());
    assert_eq!(session.summary.grant, Grant::Role(Role::Admin));
    assert!(matches!(session.auth.principal, Principal::Session { .. }));
    assert_eq!(
        harness
            .client
            .exchange_launch_code(issued.code)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );

    let stale = harness.client.issue_launch_code().await.unwrap();
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
    let launch = harness.client.issue_launch_code().await.unwrap();
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
    // A token login holds the token's grant.
    let listed_grant = harness.client.list_access_tokens().await.unwrap()[0].grant;
    assert_eq!(session.summary.grant, listed_grant);
    assert_eq!(session.auth.permissions, listed_grant.permissions());
    let unrelated = harness
        .client
        .exchange_launch_code(harness.client.issue_launch_code().await.unwrap().code)
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
async fn principal_activity_tracks_logout_token_revocation_and_expiry() {
    let harness = Harness::start().await;
    let token = harness.create_token(Some(10 * MINUTE)).await;
    let token_id = harness.client.list_access_tokens().await.unwrap()[0]
        .id
        .clone();
    let token_principal = Principal::Token {
        id: token_id.as_str().to_owned(),
        name: "unused".to_owned(),
    };

    let session = harness
        .client
        .login_with_token(token.clone())
        .await
        .unwrap();
    let session_principal = Principal::Session {
        id: session.summary.id.as_str().to_owned(),
    };
    assert!(
        harness
            .client
            .is_principal_active(&token_principal)
            .await
            .unwrap()
    );
    assert!(
        harness
            .client
            .is_principal_active(&session_principal)
            .await
            .unwrap()
    );

    harness.client.logout(session.cookie).await.unwrap();
    assert!(
        !harness
            .client
            .is_principal_active(&session_principal)
            .await
            .unwrap()
    );
    assert!(
        harness
            .client
            .is_principal_active(&token_principal)
            .await
            .unwrap()
    );

    let session = harness.client.login_with_token(token).await.unwrap();
    let session_principal = Principal::Session {
        id: session.summary.id.as_str().to_owned(),
    };
    tokio::time::advance(10 * MINUTE).await;
    assert!(
        !harness
            .client
            .is_principal_active(&token_principal)
            .await
            .unwrap()
    );
    assert!(
        !harness
            .client
            .is_principal_active(&session_principal)
            .await
            .unwrap()
    );

    let revoked_token = harness.create_token(None).await;
    let revoked_token_id = harness
        .client
        .list_access_tokens()
        .await
        .unwrap()
        .into_iter()
        .find(|summary| summary.id != token_id)
        .expect("the second token is listed")
        .id;
    let revoked_principal = Principal::Token {
        id: revoked_token_id.as_str().to_owned(),
        name: "unused".to_owned(),
    };
    let revoked_session = harness
        .client
        .login_with_token(revoked_token)
        .await
        .unwrap();
    let revoked_session_principal = Principal::Session {
        id: revoked_session.summary.id.as_str().to_owned(),
    };
    harness
        .client
        .revoke_access_token(revoked_token_id)
        .await
        .unwrap();
    assert!(
        !harness
            .client
            .is_principal_active(&revoked_principal)
            .await
            .unwrap()
    );
    assert!(
        !harness
            .client
            .is_principal_active(&revoked_session_principal)
            .await
            .unwrap()
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
async fn logout_and_revoke_close_one_session_and_a_full_table_evicts_the_oldest() {
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
            let code = client.issue_launch_code().await.unwrap().code;
            client.exchange_launch_code(code).await
        }
    };
    let first = open(&harness).await.unwrap();
    tokio::time::advance(MINUTE).await;
    let second = open(&harness).await.unwrap();
    tokio::time::advance(MINUTE).await;

    let spare = harness.client.issue_launch_code().await.unwrap();
    let third = harness
        .client
        .exchange_launch_code(spare.code)
        .await
        .expect("a full session table must not refuse a new login");
    assert_eq!(
        harness.revocations.recv().await.unwrap(),
        AuthRevocation::Session {
            session_id: first.summary.id.clone()
        }
    );

    assert_eq!(
        harness
            .client
            .logout(first.cookie)
            .await
            .expect_err("the evicted session is already gone"),
        RemoteAuthError::Unauthorized
    );

    harness.client.logout(second.cookie).await.unwrap();
    assert_eq!(
        harness.revocations.recv().await.unwrap(),
        AuthRevocation::Session {
            session_id: second.summary.id.clone()
        }
    );
    assert_eq!(
        harness
            .client
            .revoke_browser_session(second.summary.id.clone())
            .await
            .unwrap_err(),
        RemoteAuthError::NotFound
    );
    harness
        .client
        .revoke_browser_session(third.summary.id.clone())
        .await
        .unwrap();
    assert_eq!(
        harness
            .client
            .revoke_browser_session(third.summary.id)
            .await
            .unwrap_err(),
        RemoteAuthError::NotFound
    );
    assert_eq!(
        harness
            .client
            .resolve_browser_session(third.cookie)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
}

#[tokio::test(start_paused = true)]
async fn a_read_token_cannot_fill_the_session_table_and_lock_out_login() {
    let mut harness = Harness::start_with(
        RemoteAuthPolicy {
            max_sessions: 2,
            ..RemoteAuthPolicy::default()
        },
        Arc::default(),
    )
    .await;
    let read_token = harness.create_token(None).await;
    let other_token = harness.create_token(None).await;
    let first = harness
        .client
        .login_with_token(read_token.clone())
        .await
        .unwrap();
    tokio::time::advance(MINUTE).await;
    let second = harness
        .client
        .login_with_token(read_token.clone())
        .await
        .unwrap();
    tokio::time::advance(MINUTE).await;

    // The other credential still logs in. The table evicts one of the read token's own
    // sessions instead of returning a capacity error that would deny every login.
    let third = harness
        .client
        .login_with_token(other_token)
        .await
        .expect("filling the table with one token must not deny another login");
    let AuthRevocation::Session { session_id } = harness.revocations.recv().await.unwrap() else {
        panic!("expected a session eviction");
    };
    assert_eq!(
        session_id, first.summary.id,
        "the least recently active read-token session is evicted first"
    );

    // A further login from that token drops its remaining own session before another
    // principal's session.
    let fourth = harness.client.login_with_token(read_token).await.unwrap();
    let AuthRevocation::Session { session_id } = harness.revocations.recv().await.unwrap() else {
        panic!("expected a session eviction");
    };
    assert_eq!(
        session_id, second.summary.id,
        "the second read-token session is evicted before another principal"
    );

    assert_eq!(
        harness.client.list_browser_sessions().await.unwrap().len(),
        2
    );
    assert!(
        harness
            .client
            .resolve_browser_session(third.cookie)
            .await
            .is_ok()
    );
    assert!(
        harness
            .client
            .resolve_browser_session(fourth.cookie)
            .await
            .is_ok()
    );
}

const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const DAY_MS: i64 = 86_400_000;

impl Harness {
    async fn authority(&self) -> Option<Vec<u8>> {
        self.client.tls_certificates().await.unwrap().authority
    }
}

#[tokio::test(start_paused = true)]
async fn the_root_is_made_once_and_survives_a_restart() {
    let harness = Harness::start().await;
    assert_eq!(harness.authority().await, None);

    harness.client.ensure_tls_authority().await.unwrap();
    harness.client.ensure_tls_authority().await.unwrap();
    let requests = harness.generator.authority_requests();
    assert_eq!(requests.len(), 1, "a valid root is not made again");
    // A device whose clock runs a day behind still accepts it, and it outlives many renewals.
    assert_eq!(requests[0].not_before_ms, START_UNIX_MS - DAY_MS);
    assert_eq!(requests[0].not_after_ms, START_UNIX_MS + 3650 * DAY_MS);
    let authority = harness.authority().await.unwrap();
    // The listener's certificate is not made until something needs it.
    assert!(harness.generator.issue_requests().is_empty());

    let restarted = Harness::start_with(RemoteAuthPolicy::default(), harness.store.clone()).await;
    assert_eq!(restarted.authority().await, Some(authority.clone()));
    let identity = restarted.client.tls_identity().await.unwrap();
    assert_eq!(identity.authority_der(), authority.as_slice());
    assert!(restarted.generator.authority_requests().is_empty());
}

#[tokio::test(start_paused = true)]
async fn the_certificate_is_short_lived_and_reused_while_it_fits() {
    let harness = Harness::start().await;
    let first = harness.client.tls_identity().await.unwrap();
    let again = harness.client.tls_identity().await.unwrap();

    assert_eq!(first.fingerprint(), again.fingerprint());
    let requests = harness.generator.issue_requests();
    assert_eq!(
        requests.len(),
        1,
        "a fitting certificate is not issued again"
    );
    assert_eq!(requests[0].not_before_ms, START_UNIX_MS - DAY_MS);
    assert_eq!(requests[0].not_after_ms, START_UNIX_MS + 7 * DAY_MS);
    assert_eq!(
        harness.client.tls_certificates().await.unwrap().leaf,
        Some(first.certificate_der().to_vec())
    );
}

#[tokio::test(start_paused = true)]
async fn the_certificate_is_renewed_for_new_addresses_and_before_it_expires() {
    let harness = Harness::start().await;
    let first = harness.client.tls_identity().await.unwrap();

    // A new network, such as a VPN coming up, needs a certificate that names it.
    harness
        .generator
        .set_addresses(vec!["100.101.102.103".parse().unwrap()]);
    let second = harness.client.tls_identity().await.unwrap();
    assert_ne!(second.fingerprint(), first.fingerprint());
    assert!(second.covers_address("100.101.102.103".parse().unwrap()));

    // Five days in, it still has two left and is kept; a moment later it is renewed.
    tokio::time::advance(5 * DAY - Duration::from_secs(1)).await;
    let kept = harness.client.tls_identity().await.unwrap();
    assert_eq!(kept.fingerprint(), second.fingerprint());
    tokio::time::advance(Duration::from_secs(2)).await;
    let renewed = harness.client.tls_identity().await.unwrap();
    assert_ne!(renewed.fingerprint(), second.fingerprint());

    // None of this changes what clients trust.
    for identity in [&second, &renewed] {
        assert_eq!(
            identity.authority_fingerprint(),
            first.authority_fingerprint()
        );
    }
    assert_eq!(harness.generator.authority_requests().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_reset_replaces_the_root_and_keeps_the_tokens() {
    let harness = Harness::start().await;
    let token = harness.create_token(None).await;
    let first = harness.client.tls_identity().await.unwrap();

    let reset = harness.client.reset_tls().await.unwrap();

    assert_ne!(reset.fingerprint, first.authority_fingerprint());
    let current = harness.client.tls_identity().await.unwrap();
    assert_eq!(current.authority_fingerprint(), reset.fingerprint);
    // Saving the root rewrote the whole record; the token is still in it.
    let restarted = Harness::start_with(RemoteAuthPolicy::default(), harness.store.clone()).await;
    assert!(restarted.client.authenticate_token(token).await.is_ok());
    assert_eq!(
        restarted.authority().await,
        Some(current.authority_der().to_vec())
    );
}

#[tokio::test(start_paused = true)]
async fn creating_a_token_keeps_the_root_in_the_record() {
    let harness = Harness::start().await;
    harness.client.ensure_tls_authority().await.unwrap();
    let authority = harness.authority().await;
    harness.create_token(None).await;

    let restarted = Harness::start_with(RemoteAuthPolicy::default(), harness.store.clone()).await;
    assert_eq!(restarted.authority().await, authority);
}

#[tokio::test(start_paused = true)]
async fn a_root_that_cannot_be_saved_is_not_kept() {
    let harness = Harness::start().await;
    harness.store.fail_saves.store(true, Ordering::SeqCst);

    let failed = harness.client.tls_identity().await;
    assert!(matches!(
        failed,
        Err(RemoteAuthError::PersistenceFailed { .. })
    ));
    assert_eq!(harness.authority().await, None);
    assert!(harness.generator.issue_requests().is_empty());

    harness.store.fail_saves.store(false, Ordering::SeqCst);
    harness.client.ensure_tls_authority().await.unwrap();
    let authority = harness.authority().await;

    // A failed reset leaves the root clients trust in place.
    harness.store.fail_saves.store(true, Ordering::SeqCst);
    assert!(matches!(
        harness.client.reset_tls().await,
        Err(RemoteAuthError::PersistenceFailed { .. })
    ));
    assert_eq!(harness.authority().await, authority);
}

#[tokio::test(start_paused = true)]
async fn a_record_without_a_root_still_loads_and_a_malformed_one_fails_closed() {
    let store = Arc::new(MemoryStore::default());
    *store.record.lock() = Some(br#"{"version":1,"tokens":[]}"#.to_vec());
    let harness = Harness::start_with(RemoteAuthPolicy::default(), store.clone()).await;
    assert_eq!(harness.authority().await, None);

    *store.record.lock() = Some(
        br#"{"version":1,"tokens":[],"authority":{"certificate_hex":"zz","private_key_hex":"00","not_after_ms":1}}"#
            .to_vec(),
    );
    let result = Harness::spawn(RemoteAuthPolicy::default(), &store, &Arc::default()).await;
    assert!(matches!(
        result,
        Err(RemoteAuthError::PersistenceFailed { .. })
    ));
}

/// Issuance validates a grant; a record edited on disk afterwards is refused on load rather than
/// handing a token the owner role or `remote`.
#[tokio::test(start_paused = true)]
async fn a_stored_grant_that_cannot_be_issued_fails_closed() {
    let store = Arc::new(MemoryStore::default());
    let harness = Harness::start_with(RemoteAuthPolicy::default(), store.clone()).await;
    harness.create_token(None).await;
    let saved: serde_json::Value =
        serde_json::from_slice(store.record.lock().as_ref().unwrap()).unwrap();

    for grant in [
        serde_json::json!({"role": "owner"}),
        serde_json::json!({"role": "anonymous"}),
        serde_json::json!({"permissions": {"remote": "read"}}),
        serde_json::json!({"permissions": {"proxy": "write", "remote": "write"}}),
        serde_json::json!({"permissions": {"meta": "read"}}),
        serde_json::json!({"permissions": {"proxy": "read", "meta": "write"}}),
    ] {
        let mut edited = saved.clone();
        edited["tokens"][0]["grant"] = grant.clone();
        *store.record.lock() = Some(serde_json::to_vec(&edited).unwrap());
        let result = Harness::spawn(RemoteAuthPolicy::default(), &store, &Arc::default()).await;
        assert!(
            matches!(result, Err(RemoteAuthError::PersistenceFailed { .. })),
            "{grant}"
        );
    }
}
