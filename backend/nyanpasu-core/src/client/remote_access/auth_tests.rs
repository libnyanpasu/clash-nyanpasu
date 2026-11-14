//! Deterministic fake-port tests for the remote-access auth owner, lifecycle timers,
//! protected-secret persistence contracts, resource leases, and `NyanpasuClient` facade.

use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr},
    sync::{
        Arc,
        atomic::{AtomicI64, AtomicU8, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use nyanpasu_config::application::{ReleaseChannel, Scope};
use nyanpasu_contracts::auth::{
    AccessTokenStatus, AuthClock, AuthContext, AuthPeer, AuthRevocation, AuthenticatedCaller,
    CreateAccessTokenRequest, EntropyError, EntropySource, LoadedSecretRecord, Principal, ScopeSet,
    SecretBytes, SecretCipher, SecretCipherError, SecretStore, SecretStoreError, SecretString,
    StorageProtection, TlsMaterialBlob, TokenName, parse_access_token,
};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use tokio::sync::{broadcast::error::RecvError, oneshot};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    Error as RemoteAccessError, PeerRateLimitConfig as ExportedRateLimitConfig, RemoteAccessArgs,
    RemoteAccessAuthPorts, RemoteAccessClient, RemoteAuthError, RemoteAuthPolicy,
    rate_limit::{PeerRateLimitConfig, PeerRateLimiter},
};
use crate::client::{
    NyanpasuClient, SetupError,
    application::ApplicationClient,
    test_support::{TestControlEndpoint, test_client_args_with_endpoint},
};

/// Deterministic clock driven by Tokio paused time (`tokio::time::Instant`) plus an optional
/// cross-restart wall-clock shift. Calling `tokio::time::advance` advances monotonic time,
/// wall time, and Tokio wake timers simultaneously without real `sleep` calls.
struct TestClock {
    base_unix_ms: i64,
    start_instant: tokio::time::Instant,
    wall_offset_ms: AtomicI64,
}

impl TestClock {
    fn new(base_unix_ms: i64) -> Self {
        Self {
            base_unix_ms,
            start_instant: tokio::time::Instant::now(),
            wall_offset_ms: AtomicI64::new(0),
        }
    }

    fn shift_wall_ms(&self, delta_ms: i64) {
        self.wall_offset_ms.fetch_add(delta_ms, Ordering::SeqCst);
    }
}

impl AuthClock for TestClock {
    fn now_unix_ms(&self) -> i64 {
        let elapsed_ms = tokio::time::Instant::now()
            .saturating_duration_since(self.start_instant)
            .as_millis() as i64;
        self.base_unix_ms + elapsed_ms + self.wall_offset_ms.load(Ordering::SeqCst)
    }

    fn monotonic_ms(&self) -> u64 {
        tokio::time::Instant::now()
            .saturating_duration_since(self.start_instant)
            .as_millis() as u64
    }
}

/// Deterministic counter-based entropy fake (never global RNG state).
struct DeterministicEntropy {
    counter: AtomicU8,
}

impl DeterministicEntropy {
    fn new(seed: u8) -> Self {
        Self {
            counter: AtomicU8::new(seed),
        }
    }
}

impl EntropySource for DeterministicEntropy {
    fn fill_bytes(&self, dest: &mut [u8]) -> Result<(), EntropyError> {
        let base = self.counter.fetch_add(1, Ordering::SeqCst);
        for (i, byte) in dest.iter_mut().enumerate() {
            *byte = base.wrapping_mul(31).wrapping_add(i as u8);
        }
        Ok(())
    }
}

struct WriteGate {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

struct StoreWriteHandle {
    entered: oneshot::Receiver<()>,
    release: oneshot::Sender<()>,
}

struct StoreState {
    probe_protection: StorageProtection,
    write_protection: StorageProtection,
    load_protection: Option<StorageProtection>,
    stored: Option<Vec<u8>>,
    plaintext_writes: usize,
    encrypted_writes: usize,
    fail_writes_with: Option<String>,
    gates: VecDeque<WriteGate>,
}

/// In-memory fake `SecretStore` capable of simulating TOCTOU protection downgrades between
/// `protection()` probe and `save_protected_plaintext()`, I/O errors, corruption, and gated writes.
struct FakeSecretStore {
    state: Mutex<StoreState>,
}

impl FakeSecretStore {
    fn protected() -> Self {
        Self {
            state: Mutex::new(StoreState {
                probe_protection: StorageProtection::Protected,
                write_protection: StorageProtection::Protected,
                load_protection: None,
                stored: None,
                plaintext_writes: 0,
                encrypted_writes: 0,
                fail_writes_with: None,
                gates: VecDeque::new(),
            }),
        }
    }

    fn unprotected() -> Self {
        Self {
            state: Mutex::new(StoreState {
                probe_protection: StorageProtection::Unprotected,
                write_protection: StorageProtection::Unprotected,
                load_protection: None,
                stored: None,
                plaintext_writes: 0,
                encrypted_writes: 0,
                fail_writes_with: None,
                gates: VecDeque::new(),
            }),
        }
    }

    fn downgrade_at_write_only(&self) {
        let mut s = self.state.lock();
        s.probe_protection = StorageProtection::Protected;
        s.write_protection = StorageProtection::Unprotected;
    }

    fn set_fail_writes(&self, reason: Option<&str>) {
        self.state.lock().fail_writes_with = reason.map(str::to_owned);
    }

    fn inject_raw(&self, bytes: Vec<u8>, load_protection: StorageProtection) {
        let mut s = self.state.lock();
        s.stored = Some(bytes);
        s.load_protection = Some(load_protection);
    }

    fn raw_bytes(&self) -> Option<Vec<u8>> {
        self.state.lock().stored.clone()
    }

    fn write_counts(&self) -> (usize, usize) {
        let s = self.state.lock();
        (s.plaintext_writes, s.encrypted_writes)
    }

    fn gate_next_write(&self) -> StoreWriteHandle {
        let (entered_tx, entered) = oneshot::channel();
        let (release, release_rx) = oneshot::channel();
        self.state.lock().gates.push_back(WriteGate {
            entered: entered_tx,
            release: release_rx,
        });
        StoreWriteHandle { entered, release }
    }
}

#[async_trait]
impl SecretStore for FakeSecretStore {
    async fn protection(&self) -> Result<StorageProtection, SecretStoreError> {
        Ok(self.state.lock().probe_protection)
    }

    async fn load(&self) -> Result<Option<LoadedSecretRecord>, SecretStoreError> {
        let s = self.state.lock();
        let Some(bytes) = s.stored.clone() else {
            return Ok(None);
        };
        let protection = s.load_protection.unwrap_or(s.write_protection);
        Ok(Some(LoadedSecretRecord {
            bytes: SecretBytes::new(bytes),
            protection,
        }))
    }

    async fn save_protected_plaintext(&self, bytes: &[u8]) -> Result<(), SecretStoreError> {
        let gate = {
            let mut s = self.state.lock();
            s.gates.pop_front()
        };
        if let Some(gate) = gate {
            let _ = gate.entered.send(());
            let _ = gate.release.await;
        }

        let mut s = self.state.lock();
        if let Some(reason) = &s.fail_writes_with {
            return Err(SecretStoreError::Io {
                reason: reason.clone(),
            });
        }
        // Enforces write-time owner protection cleanly: refuse without modifying stored bytes.
        if s.write_protection != StorageProtection::Protected {
            return Err(SecretStoreError::UnprotectedMedium);
        }
        s.stored = Some(bytes.to_vec());
        s.load_protection = Some(StorageProtection::Protected);
        s.plaintext_writes += 1;
        Ok(())
    }

    async fn save_encrypted(&self, bytes: &[u8]) -> Result<(), SecretStoreError> {
        let gate = {
            let mut s = self.state.lock();
            s.gates.pop_front()
        };
        if let Some(gate) = gate {
            let _ = gate.entered.send(());
            let _ = gate.release.await;
        }

        let mut s = self.state.lock();
        if let Some(reason) = &s.fail_writes_with {
            return Err(SecretStoreError::Io {
                reason: reason.clone(),
            });
        }
        s.stored = Some(bytes.to_vec());
        s.load_protection = Some(s.write_protection);
        s.encrypted_writes += 1;
        Ok(())
    }
}

/// Test-only port boundary fake for `SecretCipher`. Explicitly NOT real cryptography; tests the
/// port invocation, AAD binding, and sealed-envelope behavior in unit tests before concrete AEAD
/// implementation in the OS adapter layer.
struct FakeTestPortCipher {
    seal_calls: AtomicU8,
    open_calls: AtomicU8,
}

impl FakeTestPortCipher {
    fn new() -> Self {
        Self {
            seal_calls: AtomicU8::new(0),
            open_calls: AtomicU8::new(0),
        }
    }

    fn seal_count(&self) -> u8 {
        self.seal_calls.load(Ordering::SeqCst)
    }

    fn open_count(&self) -> u8 {
        self.open_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SecretCipher for FakeTestPortCipher {
    async fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, SecretCipherError> {
        self.seal_calls.fetch_add(1, Ordering::SeqCst);
        let mut out = b"TEST_FAKE_PORT_SEAL:".to_vec();
        out.extend_from_slice(&(aad.len() as u32).to_le_bytes());
        out.extend_from_slice(aad);
        for b in plaintext {
            out.push(*b ^ 0xA5);
        }
        Ok(out)
    }

    async fn open(&self, ciphertext: &[u8], aad: &[u8]) -> Result<SecretBytes, SecretCipherError> {
        self.open_calls.fetch_add(1, Ordering::SeqCst);
        let Some(rest) = ciphertext.strip_prefix(b"TEST_FAKE_PORT_SEAL:") else {
            return Err(SecretCipherError::Crypto {
                reason: "invalid test cipher envelope".to_owned(),
            });
        };
        if rest.len() < 4 {
            return Err(SecretCipherError::Crypto {
                reason: "truncated test cipher header".to_owned(),
            });
        }
        let aad_len = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        let payload = &rest[4..];
        if payload.len() < aad_len || &payload[..aad_len] != aad {
            return Err(SecretCipherError::Crypto {
                reason: "AAD mismatch".to_owned(),
            });
        }
        let decrypted: Vec<u8> = payload[aad_len..].iter().map(|b| *b ^ 0xA5).collect();
        Ok(SecretBytes::new(decrypted))
    }
}

struct AuthFixture {
    client: RemoteAccessClient,
    clock: Arc<TestClock>,
    shutdown: CancellationToken,
    tasks: TaskTracker,
    _dir: tempfile::TempDir,
}

impl AuthFixture {
    async fn spawn(
        store: Arc<FakeSecretStore>,
        cipher: Option<Arc<FakeTestPortCipher>>,
        clock: Arc<TestClock>,
        policy: RemoteAuthPolicy,
    ) -> Result<Self, super::Error> {
        let dir = tempfile::tempdir().unwrap();
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let config_path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("application.yaml")).unwrap();
        let application = ApplicationClient::new(
            crate::state::mutation::MutationCoordinator::isolated(),
            ReleaseChannel::Stable,
            config_path,
            shutdown.child_token(),
            &tasks,
        )
        .await
        .unwrap();
        let entropy = Arc::new(DeterministicEntropy::new(7));
        let ports = RemoteAccessAuthPorts::new(
            clock.clone(),
            entropy,
            store.clone(),
            cipher.clone().map(|c| c as Arc<dyn SecretCipher>),
        )
        .with_policy(policy);
        let client = RemoteAccessClient::spawn(
            RemoteAccessArgs {
                application,
                http: None,
                shutdown: shutdown.child_token(),
            },
            Some(ports),
            &tasks,
        )
        .await?;
        Ok(Self {
            client,
            clock,
            shutdown,
            tasks,
            _dir: dir,
        })
    }

    async fn finish(self) {
        self.shutdown.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}

fn peer(last_octet: u8) -> AuthPeer {
    AuthPeer::from_ip(IpAddr::V4(Ipv4Addr::new(192, 0, 2, last_octet)))
}

#[tokio::test(start_paused = true)]
async fn token_issuance_hashes_only_redacts_secrets_and_verifies_independently() {
    let clock = Arc::new(TestClock::new(1_700_000_000_000));
    let store = Arc::new(FakeSecretStore::protected());
    let f = AuthFixture::spawn(store.clone(), None, clock, RemoteAuthPolicy::default())
        .await
        .unwrap();

    // Token with ONLY Scope::Proxy (does NOT grant Read or SensitiveRead).
    let proxy_issued = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("proxy-only").unwrap(),
            scopes: ScopeSet::from([Scope::Proxy]),
            expires_at_ms: None,
        })
        .await
        .unwrap();

    // Token with empty ScopeSet (still a valid identity for own-principal calls).
    let empty_issued = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("identity-only").unwrap(),
            scopes: ScopeSet::empty(),
            expires_at_ms: None,
        })
        .await
        .unwrap();

    assert!(proxy_issued.token.expose_secret().starts_with("npk_"));
    let dbg_issued = format!("{proxy_issued:?}");
    assert!(!dbg_issued.contains(proxy_issued.token.expose_secret()));
    assert!(dbg_issued.contains("proxy-only"));

    // Persisted JSON holds hash of the 256-bit secret only, never raw bearer secret or full token digest.
    let stored_text = String::from_utf8(store.raw_bytes().unwrap()).unwrap();
    assert!(!stored_text.contains(proxy_issued.token.expose_secret()));
    assert!(!stored_text.contains(empty_issued.token.expose_secret()));
    let parsed_proxy = parse_access_token(proxy_issued.token.expose_secret()).unwrap();
    let decoded_secret = hex::decode(parsed_proxy.secret_hex).unwrap();
    let expected_secret_hash_hex = hex::encode(Sha256::digest(&decoded_secret));
    let full_raw_token_hash_hex = hex::encode(Sha256::digest(
        proxy_issued.token.expose_secret().as_bytes(),
    ));
    assert!(stored_text.contains(&expected_secret_hash_hex));
    assert!(!stored_text.contains(&full_raw_token_hash_hex));

    // Public list summary never contains secret or hash.
    let listed = f.client.list_access_tokens().await.unwrap();
    assert_eq!(listed.len(), 2);
    let listed_json = serde_json::to_string(&listed).unwrap();
    assert!(!listed_json.contains(proxy_issued.token.expose_secret()));
    assert!(!listed_json.contains("token_hash"));

    // Verify independent ScopeSet behavior.
    let proxy_caller = f
        .client
        .authenticate_token(peer(1), proxy_issued.token.clone())
        .await
        .unwrap();
    assert!(proxy_caller.auth.has_scope(Scope::Proxy));
    assert!(!proxy_caller.auth.has_scope(Scope::Read));
    assert!(!proxy_caller.auth.has_sensitive_read());

    let empty_caller = f
        .client
        .authenticate_token(peer(1), empty_issued.token.clone())
        .await
        .unwrap();
    assert!(empty_caller.auth.scopes.is_empty());
    assert!(!empty_caller.auth.has_scope(Scope::Read));

    // Uniform refusal for invalid, malformed, tampered, or missing tokens (no anonymous fallback).
    let tampered = SecretString::new(format!(
        "{}0",
        &proxy_issued.token.expose_secret()[..proxy_issued.token.expose_secret().len() - 1]
    ));
    for bad in [
        tampered,
        SecretString::new(
            "npk_unknownid012345_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ),
        SecretString::new("not-even-a-token"),
        SecretString::new(
            "nps_sess_0123456789abcdef_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ),
    ] {
        assert_eq!(
            f.client.authenticate_token(peer(2), bad).await.unwrap_err(),
            RemoteAuthError::Unauthorized
        );
    }

    f.finish().await;
}

#[tokio::test(start_paused = true)]
async fn launch_code_is_atomic_60s_one_time_and_browser_session_stays_scoped() {
    let clock = Arc::new(TestClock::new(1_700_000_000_000));
    let store = Arc::new(FakeSecretStore::protected());
    let f = AuthFixture::spawn(store, None, clock, RemoteAuthPolicy::default())
        .await
        .unwrap();

    // Launch code minted by trusted composition for [Scope::Proxy] only (never Session full).
    let issued = f
        .client
        .issue_launch_code(ScopeSet::from([Scope::Proxy]))
        .await
        .unwrap();
    assert!(issued.code.expose_secret().starts_with("npl_"));
    assert_eq!(issued.expires_at_ms - issued.issued_at_ms, 60_000);

    // 8 concurrent callers try to exchange the exact same single-use code: at most 1 wins!
    let mut joins = Vec::new();
    for i in 0..8u8 {
        let client = f.client.clone();
        let code = issued.code.clone();
        joins.push(tokio::spawn(async move {
            client.exchange_launch_code(peer(10 + i), code).await
        }));
    }
    let mut successes = Vec::new();
    let mut failures = 0usize;
    for j in joins {
        match j.await.unwrap() {
            Ok(session) => successes.push(session),
            Err(RemoteAuthError::Unauthorized) => failures += 1,
            Err(other) => panic!("unexpected exchange error: {other:?}"),
        }
    }
    assert_eq!(successes.len(), 1);
    assert_eq!(failures, 7);

    let session = successes.pop().unwrap();
    assert_ne!(
        session.summary.id.as_str(),
        session.cookie.expose_secret(),
        "public SessionId must stay distinct from HttpOnly cookie secret"
    );
    assert!(session.auth.has_scope(Scope::Proxy));
    assert!(
        !session.auth.has_scope(Scope::Read),
        "must NOT use legacy AuthContext::session full scopes"
    );

    // Resolving session works while active and returns bound scopes.
    let resolved = f
        .client
        .resolve_browser_session(peer(3), session.cookie.clone())
        .await
        .unwrap();
    assert_eq!(resolved.auth, session.auth);

    // Revoking session invalidates old cookie permanently; old cookie cannot become another principal.
    f.client
        .revoke_browser_session(session.summary.id.clone())
        .await
        .unwrap();
    assert_eq!(
        f.client
            .resolve_browser_session(peer(3), session.cookie)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );

    // Expiry of 60s launch code when unexchanged.
    let expired_code = f
        .client
        .issue_launch_code(ScopeSet::from([Scope::Read]))
        .await
        .unwrap();
    tokio::time::advance(Duration::from_millis(60_000)).await;
    tokio::task::yield_now().await;
    assert_eq!(
        f.client
            .exchange_launch_code(peer(4), expired_code.code)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );

    f.finish().await;
}

#[tokio::test(start_paused = true)]
async fn expiry_without_new_requests_cancels_resource_leases_and_leaves_admitted_work_alive() {
    let clock = Arc::new(TestClock::new(1_700_000_000_000));
    let store = Arc::new(FakeSecretStore::protected());
    let f = AuthFixture::spawn(store, None, clock.clone(), RemoteAuthPolicy::default())
        .await
        .unwrap();

    let token = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("short-lived").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: Some(clock.now_unix_ms() + 5_000),
        })
        .await
        .unwrap();
    let caller = f
        .client
        .authenticate_token(peer(1), token.token.clone())
        .await
        .unwrap();
    let session = f
        .client
        .login_with_token(peer(1), token.token.clone())
        .await
        .unwrap();
    let res = f
        .client
        .create_resource_session(caller.auth.clone())
        .await
        .unwrap();
    let sess_res = f
        .client
        .create_resource_session(session.auth.clone())
        .await
        .unwrap();
    let mut rev_rx = f.client.subscribe_revocations().unwrap();

    // Simulate an already-admitted business operation holding its captured AuthContext and a
    // gate so it only completes AFTER credential expiry fires.
    let admitted = f.client.admit_caller(caller.clone()).await.unwrap();
    let (biz_gate_tx, biz_gate_rx) = oneshot::channel::<()>();
    let biz_task = tokio::spawn(async move {
        let captured = admitted.auth;
        let _ = biz_gate_rx.await;
        captured.has_scope(Scope::Read)
    });

    assert!(!res.lease.is_cancelled());
    assert!(!sess_res.lease.is_cancelled());

    // Advance Tokio time with ZERO further client requests: supervised owner wake must cancel leases!
    tokio::time::advance(Duration::from_millis(5_001)).await;
    tokio::time::timeout(
        Duration::from_secs(1),
        res.lease.cancellation_token().cancelled(),
    )
    .await
    .expect("token lease must cancel cleanly without new client requests");
    tokio::time::timeout(
        Duration::from_secs(1),
        sess_res.lease.cancellation_token().cancelled(),
    )
    .await
    .expect("token-bound session lease must cancel cleanly without new client requests");

    // Verify revocation broadcast received Token invalidation.
    let mut saw_token_revocation = false;
    while let Ok(ev) = rev_rx.try_recv() {
        if let AuthRevocation::Token { token_id, .. } = ev {
            assert_eq!(token_id, token.summary.id);
            saw_token_revocation = true;
        }
    }
    assert!(saw_token_revocation);

    // Old resolved context / stale credentials are refused on NEW admissions.
    assert_eq!(
        f.client.admit_caller(caller.clone()).await.unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert_eq!(
        f.client
            .admit_caller(nyanpasu_contracts::auth::AuthenticatedCaller {
                auth: session.auth.clone(),
                expires_at_ms: session.summary.token_expires_at_ms,
                auth_generation: session.auth_generation,
            })
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert_eq!(
        f.client
            .resolve_resource_session(caller.auth.clone(), res.cookie)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );

    // Already-admitted business work finishes normally without being cancelled.
    biz_gate_tx.send(()).unwrap();
    assert!(biz_task.await.unwrap());

    f.finish().await;
}

#[tokio::test(start_paused = true)]
async fn cross_restart_monotonic_reset_still_refuses_wall_expired_durable_token() {
    let store = Arc::new(FakeSecretStore::protected());

    // First process: wall = T0, create token expiring at T0 + 10_000ms.
    let clock1 = Arc::new(TestClock::new(1_700_000_000_000));
    let f1 = AuthFixture::spawn(
        store.clone(),
        None,
        clock1.clone(),
        RemoteAuthPolicy::default(),
    )
    .await
    .unwrap();
    let issued = f1
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("wall-expiry").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: Some(clock1.now_unix_ms() + 10_000),
        })
        .await
        .unwrap();
    f1.finish().await;

    // Second process starts with BRAND-NEW monotonic clock starting at 0, while UTC wall clock
    // jumped forward +20_000ms across restart.
    let clock2 = Arc::new(TestClock::new(1_700_000_020_000));
    assert_eq!(clock2.monotonic_ms(), 0);
    let f2 = AuthFixture::spawn(store, None, clock2, RemoteAuthPolicy::default())
        .await
        .unwrap();

    assert_eq!(
        f2.client
            .authenticate_token(peer(1), issued.token)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    let listed = f2.client.list_access_tokens().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].status, AccessTokenStatus::Expired);

    f2.finish().await;
}

#[tokio::test(start_paused = true)]
async fn browser_idle_timeout_refreshes_on_use_and_expires_when_idle() {
    let clock = Arc::new(TestClock::new(1_700_000_000_000));
    let store = Arc::new(FakeSecretStore::protected());
    let policy = RemoteAuthPolicy {
        browser_session_idle_ms: 10_000,
        ..Default::default()
    };
    let f = AuthFixture::spawn(store, None, clock, policy)
        .await
        .unwrap();

    let code = f
        .client
        .issue_launch_code(ScopeSet::from([Scope::Read]))
        .await
        .unwrap();
    let sess = f
        .client
        .exchange_launch_code(peer(1), code.code)
        .await
        .unwrap();
    let lease = f
        .client
        .create_resource_session(sess.auth.clone())
        .await
        .unwrap();

    // Advance 6_000ms (< 10_000ms idle) and resolve -> refreshes idle deadline to +16_000ms!
    tokio::time::advance(Duration::from_millis(6_000)).await;
    assert!(
        f.client
            .resolve_browser_session(peer(1), sess.cookie.clone())
            .await
            .is_ok()
    );

    // Advance another 6_000ms (total 12_000ms > initial 10_000ms, but only 6_000ms idle since last use).
    tokio::time::advance(Duration::from_millis(6_000)).await;
    assert!(!lease.lease.is_cancelled());

    // Advance another 4_001ms (idle >= 10_001ms) with zero requests -> wake cancels lease & expires session.
    tokio::time::advance(Duration::from_millis(4_001)).await;
    tokio::time::timeout(
        Duration::from_secs(1),
        lease.lease.cancellation_token().cancelled(),
    )
    .await
    .expect("idle session lease should cancel automatically");

    assert_eq!(
        f.client
            .resolve_browser_session(peer(1), sess.cookie)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    f.finish().await;
}

#[tokio::test(start_paused = true)]
async fn resource_sessions_distinct_non_authenticating_and_bounded_per_principal_and_global() {
    let clock = Arc::new(TestClock::new(1_700_000_000_000));
    let store = Arc::new(FakeSecretStore::protected());
    let policy = RemoteAuthPolicy {
        max_resource_sessions_per_principal: 2,
        max_resource_sessions_global: 3,
        ..Default::default()
    };
    let f = AuthFixture::spawn(store, None, clock, policy)
        .await
        .unwrap();

    let tok_a = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("tok-a").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    let tok_b = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("tok-b").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        })
        .await
        .unwrap();

    let caller_a = f
        .client
        .authenticate_token(peer(1), tok_a.token.clone())
        .await
        .unwrap();
    let caller_b = f
        .client
        .authenticate_token(peer(1), tok_b.token.clone())
        .await
        .unwrap();

    // Two connections sharing the same Token principal get distinct resource IDs, owners, cookies, and leases.
    let r1 = f
        .client
        .create_resource_session(caller_a.auth.clone())
        .await
        .unwrap();
    let r2 = f
        .client
        .create_resource_session(caller_a.auth.clone())
        .await
        .unwrap();
    assert_ne!(r1.id, r2.id);
    assert_ne!(r1.owner_id, r2.owner_id);
    assert_ne!(r1.cookie.expose_secret(), r2.cookie.expose_secret());

    // Per-principal cap blocks 3rd allocation for Token A.
    assert_eq!(
        f.client
            .create_resource_session(caller_a.auth.clone())
            .await
            .unwrap_err(),
        RemoteAuthError::CapacityExceeded {
            resource: "resource_sessions_per_principal"
        }
    );

    // Resource cookie can NEVER authenticate as a token or browser session.
    assert_eq!(
        f.client
            .authenticate_token(peer(1), r1.cookie.clone())
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert_eq!(
        f.client
            .resolve_browser_session(peer(1), r1.cookie.clone())
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );

    // Principal mismatch refuses cross-principal resource cookie resolution.
    assert_eq!(
        f.client
            .resolve_resource_session(caller_b.auth.clone(), r1.cookie.clone())
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    let resolved = f
        .client
        .resolve_resource_session(caller_a.auth.clone(), r1.cookie.clone())
        .await
        .unwrap();
    assert_eq!(resolved.owner_id, r1.owner_id);

    // Same UID local socket connections allocate separate resource owners cleanly until global cap (3).
    let local = AuthContext::local_socket("1000");
    let r3 = f
        .client
        .create_resource_session(local.clone())
        .await
        .unwrap();
    assert_ne!(r3.owner_id, r1.owner_id);

    // Global cap blocks 4th allocation across distinct principals.
    assert_eq!(
        f.client
            .create_resource_session(AuthContext::new(Principal::Anonymous, ScopeSet::empty()))
            .await
            .unwrap_err(),
        RemoteAuthError::CapacityExceeded {
            resource: "resource_sessions_global"
        }
    );

    // Releasing r1 frees a slot and cancels ONLY r1's lease.
    f.client.release_resource_session(r1.id).await.unwrap();
    assert!(r1.lease.is_cancelled());
    assert!(!r2.lease.is_cancelled());
    assert!(
        f.client
            .create_resource_session(caller_a.auth)
            .await
            .is_ok()
    );

    f.finish().await;
}

#[tokio::test(start_paused = true)]
async fn broadcast_lag_does_not_bypass_resource_lease_cancellation_or_admission_recheck() {
    let clock = Arc::new(TestClock::new(1_700_000_000_000));
    let store = Arc::new(FakeSecretStore::protected());
    let policy = RemoteAuthPolicy {
        revocation_channel_capacity: 8,
        ..Default::default()
    };
    let f = AuthFixture::spawn(store, None, clock, policy)
        .await
        .unwrap();

    let token = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("lag-test").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    let caller = f
        .client
        .authenticate_token(peer(1), token.token.clone())
        .await
        .unwrap();
    let lease_res = f
        .client
        .create_resource_session(caller.auth.clone())
        .await
        .unwrap();

    // Subscribe, then overflow the tiny broadcast channel (capacity 8) with unrelated session revocations
    // BEFORE revoking the token so the subscriber experiences `RecvError::Lagged`.
    let mut lagged_rx = f.client.subscribe_revocations().unwrap();
    f.client
        .revoke_access_token(token.summary.id)
        .await
        .unwrap();
    for _ in 0..10 {
        let anon_res = f
            .client
            .create_resource_session(AuthContext::new(Principal::Anonymous, ScopeSet::empty()))
            .await
            .unwrap();
        f.client
            .release_resource_session(anon_res.id)
            .await
            .unwrap();
    }

    assert!(matches!(lagged_rx.recv().await, Err(RecvError::Lagged(_))));
    assert!(
        lease_res.lease.is_cancelled(),
        "ResourceLease remains cancelled even when broadcast receiver lagged"
    );
    assert_eq!(
        f.client.admit_caller(caller).await.unwrap_err(),
        RemoteAuthError::Unauthorized
    );

    f.finish().await;
}

#[tokio::test(start_paused = true)]
async fn bounded_peer_rate_limiter_locks_out_failing_peer_and_bounds_peer_table() {
    let clock = Arc::new(TestClock::new(1_700_000_000_000));
    let store = Arc::new(FakeSecretStore::protected());
    let policy = RemoteAuthPolicy {
        rate_limit: ExportedRateLimitConfig {
            max_peers: 3,
            max_failures: 3,
            window_ms: 10_000,
            lockout_ms: 15_000,
        },
        ..Default::default()
    };
    let f = AuthFixture::spawn(store, None, clock, policy)
        .await
        .unwrap();

    let valid = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("rate-test").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        })
        .await
        .unwrap();

    let bad_cred = SecretString::new("npk_bad_secret");
    for _ in 0..3 {
        assert_eq!(
            f.client
                .authenticate_token(peer(10), bad_cred.clone())
                .await
                .unwrap_err(),
            RemoteAuthError::Unauthorized
        );
    }

    // Even presenting the VALID token from peer(10) is rate-limited before credential acceptance.
    assert!(matches!(
        f.client.authenticate_token(peer(10), valid.token.clone()).await,
        Err(RemoteAuthError::RateLimited { retry_after_ms }) if retry_after_ms > 0
    ));
    // Distinct peer(11) succeeds normally.
    assert!(
        f.client
            .authenticate_token(peer(11), valid.token.clone())
            .await
            .is_ok()
    );

    // Wall-clock skew MUST NOT prematurely unlock monotonic lockout.
    f.clock.shift_wall_ms(120_000);
    assert!(matches!(
        f.client
            .authenticate_token(peer(10), valid.token.clone())
            .await,
        Err(RemoteAuthError::RateLimited { .. })
    ));

    // Monotonic clock advance clears lockout.
    tokio::time::advance(Duration::from_millis(15_001)).await;
    assert!(
        f.client
            .authenticate_token(peer(10), valid.token)
            .await
            .is_ok()
    );
    f.finish().await;

    // Direct unit check proving table stays bounded <= max_peers across IP churn.
    let mut limiter = PeerRateLimiter::new(PeerRateLimitConfig {
        max_peers: 4,
        max_failures: 3,
        window_ms: 60_000,
        lockout_ms: 60_000,
    });
    for i in 1..=25u8 {
        limiter.record_failure(peer(i), 100 + u64::from(i));
        assert!(limiter.tracked_peer_count() <= 4);
    }
}

#[tokio::test(start_paused = true)]
async fn write_time_medium_downgrade_never_writes_plaintext_and_falls_back_or_refuses() {
    // Case A: probe reports Protected, save downgrades to Unprotected, NO cipher -> refuse cleanly!
    let store_no_cipher = Arc::new(FakeSecretStore::protected());
    store_no_cipher.downgrade_at_write_only();
    let f_refuse = AuthFixture::spawn(
        store_no_cipher.clone(),
        None,
        Arc::new(TestClock::new(1_700_000_000_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    .unwrap();

    let err = f_refuse
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("no-cipher-refuse").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        })
        .await
        .unwrap_err();
    assert_eq!(err, RemoteAuthError::UnprotectedStorageWithoutCipher);
    assert_eq!(store_no_cipher.write_counts(), (0, 0));
    assert!(store_no_cipher.raw_bytes().is_none());
    assert!(
        f_refuse
            .client
            .list_access_tokens()
            .await
            .unwrap()
            .is_empty()
    );
    f_refuse.finish().await;

    // Case B: probe reports Protected, save downgrades to Unprotected, cipher IS wired -> seals & encrypts!
    let store_downgrade = Arc::new(FakeSecretStore::protected());
    store_downgrade.downgrade_at_write_only();
    let cipher = Arc::new(FakeTestPortCipher::new());
    let f_enc = AuthFixture::spawn(
        store_downgrade.clone(),
        Some(cipher.clone()),
        Arc::new(TestClock::new(1_700_000_000_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    .unwrap();

    let issued = f_enc
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("sealed-token").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    assert_eq!(store_downgrade.write_counts(), (0, 1));
    assert_eq!(cipher.seal_count(), 1);
    let persisted = String::from_utf8(store_downgrade.raw_bytes().unwrap()).unwrap();
    assert!(persisted.contains("\"encrypted_v1\""));
    assert!(!persisted.contains("sealed-token"));
    f_enc.finish().await;

    // Reload from encrypted blob works when cipher is provided.
    let f_reload = AuthFixture::spawn(
        store_downgrade,
        Some(cipher),
        Arc::new(TestClock::new(1_700_000_001_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    .unwrap();
    assert!(
        f_reload
            .client
            .authenticate_token(peer(1), issued.token)
            .await
            .is_ok()
    );
    f_reload.finish().await;

    // Case C: initially unprotected store with cipher skips plaintext path cleanly.
    let store_unprot = Arc::new(FakeSecretStore::unprotected());
    let cipher2 = Arc::new(FakeTestPortCipher::new());
    let f_unprot = AuthFixture::spawn(
        store_unprot.clone(),
        Some(cipher2.clone()),
        Arc::new(TestClock::new(1_700_000_000_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    .unwrap();
    f_unprot
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("unprot-enc").unwrap(),
            scopes: ScopeSet::empty(),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    assert_eq!(store_unprot.write_counts(), (0, 1));
    assert_eq!(cipher2.seal_count(), 1);
    f_unprot.finish().await;
}

#[tokio::test(start_paused = true)]
async fn startup_fails_closed_on_unprotected_plaintext_or_corrupt_store_without_clearing() {
    // Inject PlainV1 bytes with StorageProtection::Unprotected.
    let store_unprot_plain = Arc::new(FakeSecretStore::protected());
    let plain_payload =
        br#"{"format":"plain_v1","payload":{"version":1,"tokens":[],"tls":null}}"#.to_vec();
    store_unprot_plain.inject_raw(plain_payload.clone(), StorageProtection::Unprotected);

    let err = match AuthFixture::spawn(
        store_unprot_plain.clone(),
        Some(Arc::new(FakeTestPortCipher::new())),
        Arc::new(TestClock::new(1_700_000_000_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    {
        Ok(_) => panic!("unprotected plaintext record must fail closed at startup"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        super::Error::SpawnAuthOwner {
            source: RemoteAuthError::PersistenceFailed { .. }
        }
    ));
    assert_eq!(
        store_unprot_plain.raw_bytes().unwrap(),
        plain_payload,
        "failed startup must NOT clear or overwrite unprotected record"
    );

    // Inject corrupt JSON with StorageProtection::Protected.
    let store_corrupt = Arc::new(FakeSecretStore::protected());
    let corrupt_bytes = b"{not valid json".to_vec();
    store_corrupt.inject_raw(corrupt_bytes.clone(), StorageProtection::Protected);
    let err2 = match AuthFixture::spawn(
        store_corrupt.clone(),
        None,
        Arc::new(TestClock::new(1_700_000_000_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    {
        Ok(_) => panic!("corrupt store record must fail closed at startup"),
        Err(err) => err,
    };
    assert!(matches!(
        err2,
        super::Error::SpawnAuthOwner {
            source: RemoteAuthError::PersistenceFailed { .. }
        }
    ));
    assert_eq!(store_corrupt.raw_bytes().unwrap(), corrupt_bytes);
}

#[tokio::test(start_paused = true)]
async fn revoke_durable_failure_reports_in_memory_only_and_later_persist_flushes_removal() {
    let store = Arc::new(FakeSecretStore::protected());
    let f = AuthFixture::spawn(
        store.clone(),
        None,
        Arc::new(TestClock::new(1_700_000_000_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    .unwrap();

    let issued = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("revoke-fail").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    let caller = f
        .client
        .authenticate_token(peer(1), issued.token.clone())
        .await
        .unwrap();
    let lease = f
        .client
        .create_resource_session(caller.auth.clone())
        .await
        .unwrap();

    // Inject I/O error during revocation persist.
    store.set_fail_writes(Some("disk read-only"));
    let err = f
        .client
        .revoke_access_token(issued.summary.id.clone())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        RemoteAuthError::RevocationPersistenceFailed { .. }
    ));
    assert!(err.to_string().contains("in memory only"));
    assert!(lease.lease.is_cancelled());
    assert_eq!(
        f.client
            .authenticate_token(peer(1), issued.token.clone())
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert!(f.client.list_access_tokens().await.unwrap().is_empty());

    // Recover store and create another token -> subsequent durable snapshot MUST omit the
    // pending-revoked token so restart never resurrects it!
    store.set_fail_writes(None);
    let second = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("flush-pending").unwrap(),
            scopes: ScopeSet::from([Scope::Proxy]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    f.finish().await;

    let restarted = AuthFixture::spawn(
        store,
        None,
        Arc::new(TestClock::new(1_700_000_005_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        restarted
            .client
            .authenticate_token(peer(1), issued.token)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert!(
        restarted
            .client
            .authenticate_token(peer(1), second.token)
            .await
            .is_ok()
    );
    restarted.finish().await;
}

#[tokio::test(start_paused = true)]
async fn caller_drop_does_not_cancel_persist_and_tls_blob_roundtrips_redacted() {
    let store = Arc::new(FakeSecretStore::protected());
    let f = AuthFixture::spawn(
        store.clone(),
        None,
        Arc::new(TestClock::new(1_700_000_000_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    .unwrap();

    // Gate store write, abort caller mid-write, then release gate: owner completes full persist!
    let gate = store.gate_next_write();
    let client = f.client.clone();
    let caller = tokio::spawn(async move {
        client
            .create_access_token(CreateAccessTokenRequest {
                name: TokenName::parse("dropped-caller").unwrap(),
                scopes: ScopeSet::from([Scope::Read]),
                expires_at_ms: None,
            })
            .await
    });
    gate.entered.await.unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    gate.release.send(()).unwrap();

    // Next serialized actor call observes committed token on disk and in memory.
    let listed = f.client.list_access_tokens().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name.as_str(), "dropped-caller");
    assert_eq!(store.write_counts(), (1, 0));

    let valid_fingerprint = "deadbeef01234567deadbeef01234567deadbeef01234567deadbeef01234567";
    // TLS material contract persistence & redaction (no cert generation in this layer).
    let blob = TlsMaterialBlob {
        cert_pem: "-----BEGIN CERTIFICATE-----\nMIIB...\n-----END CERTIFICATE-----\n".to_owned(),
        private_key_pem: SecretString::new(
            "-----BEGIN PRIVATE KEY-----\nTOPSECRETKEYDATA\n-----END PRIVATE KEY-----\n",
        ),
        fingerprint_sha256: valid_fingerprint.to_owned(),
        expires_at_ms: Some(1_800_000_000_000),
    };
    assert!(!format!("{blob:?}").contains("TOPSECRETKEYDATA"));

    let status = f.client.set_tls_material(blob).await.unwrap();
    assert_eq!(status.fingerprint_sha256, valid_fingerprint);
    assert_eq!(f.client.tls_material_status().await.unwrap(), Some(status));

    // Concurrent token creations serialize cleanly inside the actor mailbox with zero lost updates.
    let (c1, c2) = tokio::join!(
        f.client.create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("concurrent-a").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        }),
        f.client.create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("concurrent-b").unwrap(),
            scopes: ScopeSet::from([Scope::Proxy]),
            expires_at_ms: None,
        })
    );
    assert!(c1.is_ok());
    assert!(c2.is_ok());
    assert_eq!(f.client.list_access_tokens().await.unwrap().len(), 3);
    f.finish().await;

    // Restart loads TLS material status cleanly without leaking private key in public summary.
    let restarted = AuthFixture::spawn(
        store,
        None,
        Arc::new(TestClock::new(1_700_000_001_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    .unwrap();
    let reloaded_status = restarted
        .client
        .tls_material_status()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reloaded_status.fingerprint_sha256, valid_fingerprint);
    let reloaded_blob = restarted.client.load_tls_material().await.unwrap().unwrap();
    assert!(
        reloaded_blob
            .private_key_pem
            .expose_secret()
            .contains("TOPSECRETKEYDATA")
    );
    restarted.finish().await;
}

#[tokio::test(start_paused = true)]
async fn admit_caller_and_resource_apis_recompute_live_authority_and_reject_stale_scopes() {
    let clock = Arc::new(TestClock::new(1_700_000_000_000));
    let store = Arc::new(FakeSecretStore::protected());
    let f = AuthFixture::spawn(store, None, clock, RemoteAuthPolicy::default())
        .await
        .unwrap();

    let issued = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("proxy-only").unwrap(),
            scopes: ScopeSet::from([Scope::Proxy]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    let caller = f
        .client
        .authenticate_token(peer(1), issued.token.clone())
        .await
        .unwrap();

    // Even if an adapter passes an inflated ScopeSet inside AuthenticatedCaller, owner recomputes
    // authoritative live grants cleanly and refuses ungranted escalation.
    let tampered_caller = AuthenticatedCaller {
        auth: AuthContext::new(caller.auth.principal.clone(), ScopeSet::full()),
        expires_at_ms: caller.expires_at_ms,
        auth_generation: caller.auth_generation,
    };
    let recomputed = f.client.admit_caller(tampered_caller).await.unwrap();
    assert!(recomputed.auth.has_scope(Scope::Proxy));
    assert!(!recomputed.auth.has_scope(Scope::Read));

    // After revocation, old AuthContext / AuthenticatedCaller can never revive access or create resources.
    f.client
        .revoke_access_token(issued.summary.id)
        .await
        .unwrap();
    assert_eq!(
        f.client.admit_caller(caller.clone()).await.unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert_eq!(
        f.client
            .create_resource_session(caller.auth)
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );

    f.finish().await;
}

#[tokio::test(start_paused = true)]
async fn nyanpasu_client_facade_wires_auth_owner_and_reports_not_configured_when_none() {
    let dir = tempfile::tempdir().unwrap();

    // When `remote_auth` is `None`, facade operations report explicit `NotConfigured`.
    let mut args_none = test_client_args_with_endpoint(
        &dir,
        TestControlEndpoint::succeeding(),
        crate::client::test_support::profile_test_adapters(&dir),
    )
    .await;
    args_none.remote_auth = None;
    let client_none = NyanpasuClient::try_new(args_none).await.unwrap();
    assert_eq!(
        client_none.list_remote_access_tokens().await.unwrap_err(),
        RemoteAuthError::NotConfigured
    );
    assert!(matches!(
        client_none.subscribe_remote_auth_revocations(),
        Err(RemoteAuthError::NotConfigured)
    ));
    client_none.request_shutdown();
    client_none.wait_shutdown().await;

    // When `remote_auth` is `Some(...)`, full domain lifecycle works via NyanpasuClient facade.
    let dir2 = tempfile::tempdir().unwrap();
    let mut args_some = test_client_args_with_endpoint(
        &dir2,
        TestControlEndpoint::succeeding(),
        crate::client::test_support::profile_test_adapters(&dir2),
    )
    .await;
    let clock = Arc::new(TestClock::new(1_700_000_000_000));
    let store = Arc::new(FakeSecretStore::protected());
    args_some.remote_auth = Some(RemoteAccessAuthPorts::new(
        clock,
        Arc::new(DeterministicEntropy::new(13)),
        store,
        None,
    ));

    let client = NyanpasuClient::try_new(args_some).await.unwrap();
    let issued = client
        .create_remote_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("facade-client").unwrap(),
            scopes: ScopeSet::from([Scope::Read, Scope::Proxy]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    let session = client
        .login_remote_with_token(peer(5), issued.token)
        .await
        .unwrap();
    let resource = client
        .create_remote_resource_session(session.auth.clone())
        .await
        .unwrap();

    assert_eq!(
        client.list_remote_browser_sessions().await.unwrap().len(),
        1
    );
    client
        .revoke_remote_access_token(issued.summary.id)
        .await
        .unwrap();
    assert!(resource.lease.is_cancelled());
    assert!(
        client
            .list_remote_browser_sessions()
            .await
            .unwrap()
            .is_empty()
    );

    // Verify shutdown cancels remaining resource leases cleanly and exits with zero orphan tasks.
    let anon_res = client
        .create_remote_resource_session(AuthContext::new(Principal::Anonymous, ScopeSet::empty()))
        .await
        .unwrap();
    client.request_shutdown();
    client.wait_shutdown().await;
    assert!(anon_res.lease.is_cancelled());
}

#[tokio::test(start_paused = true)]
async fn nyanpasu_client_setup_fails_closed_when_secret_store_is_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let host_adapters = crate::client::test_support::profile_test_adapters(&dir);
    let mut args =
        test_client_args_with_endpoint(&dir, TestControlEndpoint::succeeding(), host_adapters)
            .await;
    let store = Arc::new(FakeSecretStore::protected());
    store.inject_raw(b"corrupt".to_vec(), StorageProtection::Protected);
    args.remote_auth = Some(RemoteAccessAuthPorts::new(
        Arc::new(TestClock::new(1_700_000_000_000)),
        Arc::new(DeterministicEntropy::new(3)),
        store,
        None,
    ));

    let err = match NyanpasuClient::try_new(args).await {
        Ok(_) => panic!("corrupt secret store must fail client startup"),
        Err(err) => err,
    };
    assert!(matches!(err, SetupError::StartRemoteAccess { .. }));
}

#[tokio::test(start_paused = true)]
async fn malformed_store_errors_redact_secret_substrings_and_reject_invalid_tls_fingerprints() {
    // Valid JSON hitting an unknown `StoredEnvelope` variant would echo the sentinel under raw
    // serde Display (`unknown variant \`SUPERSECRET_SENTINEL\``); verify both Display and Debug
    // stay bounded and omit the sentinel.
    let store = Arc::new(FakeSecretStore::protected());
    store.inject_raw(
        br#"{"format":"SUPERSECRET_SENTINEL"}"#.to_vec(),
        StorageProtection::Protected,
    );
    let err = match AuthFixture::spawn(
        store,
        None,
        Arc::new(TestClock::new(1_700_000_000_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    {
        Ok(_) => panic!("corrupt outer envelope must fail startup"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        RemoteAccessError::SpawnAuthOwner {
            source: RemoteAuthError::PersistenceFailed { .. }
        }
    ));
    let err_msg = format!("{err}");
    assert!(err_msg.contains("corrupt secret store envelope"));
    assert!(!err_msg.contains("SUPERSECRET_SENTINEL"));
    assert!(!format!("{err:?}").contains("SUPERSECRET_SENTINEL"));

    // Valid outer `encrypted_v1` envelope MUST pass outer parsing, call `cipher.open`, and hit
    // inner `PersistedAuthPayloadV1` deserialization with a type error that would echo the
    // sentinel under raw serde Display (`invalid type: string "SUPERSECRET_SENTINEL", expected u32`).
    let cipher = Arc::new(FakeTestPortCipher::new());
    let sealed_bad = cipher
        .seal(
            br#"{"version":"SUPERSECRET_SENTINEL","tokens":[],"tls":null}"#,
            b"nyanpasu.remote-access.secrets.v1",
        )
        .await
        .unwrap();
    let envelope_json = serde_json::to_vec(&serde_json::json!({
        "format": "encrypted_v1",
        "version": 1,
        "ciphertext_hex": hex::encode(sealed_bad),
    }))
    .unwrap();
    let enc_store = Arc::new(FakeSecretStore::unprotected());
    enc_store.inject_raw(envelope_json, StorageProtection::Unprotected);
    let err = match AuthFixture::spawn(
        enc_store,
        Some(cipher.clone()),
        Arc::new(TestClock::new(1_700_000_000_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    {
        Ok(_) => panic!("corrupt decrypted payload must fail startup"),
        Err(err) => err,
    };
    assert_eq!(cipher.open_count(), 1);
    assert!(matches!(
        err,
        RemoteAccessError::SpawnAuthOwner {
            source: RemoteAuthError::PersistenceFailed { .. }
        }
    ));
    let inner_msg = format!("{err}");
    assert!(inner_msg.contains("corrupt decrypted secret payload"));
    assert!(!inner_msg.contains("SUPERSECRET_SENTINEL"));
    assert!(!format!("{err:?}").contains("SUPERSECRET_SENTINEL"));

    // Wrong-length (63 chars) and uppercase hex TLS fingerprints must fail validation.
    let f = AuthFixture::spawn(
        Arc::new(FakeSecretStore::protected()),
        None,
        Arc::new(TestClock::new(1_700_000_000_000)),
        RemoteAuthPolicy::default(),
    )
    .await
    .unwrap();
    for bad_fp in ["a".repeat(63), "A".repeat(64)] {
        let res = f
            .client
            .set_tls_material(TlsMaterialBlob {
                cert_pem: "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----\n"
                    .to_owned(),
                private_key_pem: SecretString::new(
                    "-----BEGIN PRIVATE KEY-----\nKEY\n-----END PRIVATE KEY-----\n",
                ),
                fingerprint_sha256: bad_fp,
                expires_at_ms: None,
            })
            .await;
        assert!(matches!(res, Err(RemoteAuthError::InvalidInput { .. })));
    }
    f.finish().await;
}

#[tokio::test(start_paused = true)]
async fn max_persisted_record_bytes_enforced_before_plain_and_encrypted_writes() {
    // Case A: Protected plaintext store with tight limit preserves first token and refuses oversized write.
    let plain_store = Arc::new(FakeSecretStore::protected());
    let policy_plain = RemoteAuthPolicy {
        max_persisted_record_bytes: 512,
        ..Default::default()
    };
    let f = AuthFixture::spawn(
        plain_store.clone(),
        None,
        Arc::new(TestClock::new(1_700_000_000_000)),
        policy_plain,
    )
    .await
    .unwrap();

    f.client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("small-one").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    assert_eq!(plain_store.write_counts(), (1, 0));

    let err = f
        .client
        .set_tls_material(TlsMaterialBlob {
            cert_pem: "C".repeat(600),
            private_key_pem: SecretString::new("K".repeat(128)),
            fingerprint_sha256: "b".repeat(64),
            expires_at_ms: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, RemoteAuthError::PersistenceFailed { .. }));
    assert_eq!(plain_store.write_counts(), (1, 0));
    assert_eq!(f.client.list_access_tokens().await.unwrap().len(), 1);
    f.finish().await;

    // Restart still reads preserved small token cleanly.
    let reloaded = AuthFixture::spawn(
        plain_store,
        None,
        Arc::new(TestClock::new(1_700_000_001_000)),
        policy_plain,
    )
    .await
    .unwrap();
    assert_eq!(reloaded.client.list_access_tokens().await.unwrap().len(), 1);
    reloaded.finish().await;

    // Case B: Unprotected encrypted store checks ciphertext_hex expanded size BEFORE save_encrypted.
    let enc_store = Arc::new(FakeSecretStore::unprotected());
    let cipher = Arc::new(FakeTestPortCipher::new());
    let policy_enc = RemoteAuthPolicy {
        max_persisted_record_bytes: 420,
        ..Default::default()
    };
    let f_enc = AuthFixture::spawn(
        enc_store.clone(),
        Some(cipher),
        Arc::new(TestClock::new(1_700_000_000_000)),
        policy_enc,
    )
    .await
    .unwrap();
    let err = f_enc
        .client
        .set_tls_material(TlsMaterialBlob {
            // Fits in 420 bytes plaintext? ~320 bytes plaintext, > 640 hex ciphertext bytes!
            cert_pem: "C".repeat(220),
            private_key_pem: SecretString::new("K".repeat(60)),
            fingerprint_sha256: "c".repeat(64),
            expires_at_ms: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err, RemoteAuthError::PersistenceFailed { .. }));
    assert_eq!(enc_store.write_counts(), (0, 0));
    f_enc.finish().await;
}

#[tokio::test(start_paused = true)]
async fn rate_limiter_preserves_active_lockouts_fails_closed_at_saturation_and_rejects_zero_policy()
{
    // Reject invalid zero policy bounds cleanly at spawn time.
    let bad_policy = RemoteAuthPolicy {
        rate_limit: ExportedRateLimitConfig {
            max_peers: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    let res = AuthFixture::spawn(
        Arc::new(FakeSecretStore::protected()),
        None,
        Arc::new(TestClock::new(1_700_000_000_000)),
        bad_policy,
    )
    .await;
    assert!(matches!(
        res,
        Err(RemoteAccessError::SpawnAuthOwner {
            source: RemoteAuthError::InvalidInput { .. }
        })
    ));

    // Churning > max_peers failing peers must NEVER evict an active unexpired lockout on peer(1).
    let mut limiter = PeerRateLimiter::new(ExportedRateLimitConfig {
        max_peers: 4,
        max_failures: 2,
        window_ms: 60_000,
        lockout_ms: 60_000,
    });
    limiter.record_failure(peer(1), 100);
    limiter.record_failure(peer(1), 101);
    assert!(limiter.check_allowed(peer(1), 102).is_err());

    for i in 2..=25u8 {
        limiter.record_failure(peer(i), 200 + u64::from(i));
        assert!(limiter.tracked_peer_count() <= 4);
    }
    assert!(limiter.check_allowed(peer(1), 500).is_err());

    // Cycling max_peers + 1 distinct peers at fixed monotonic time must NOT erase unexpired
    // failure counters or let unknown peers bypass limits; once windows fill, unknown peers
    // fail closed until an entry expires.
    let mut churn_limiter = PeerRateLimiter::new(ExportedRateLimitConfig {
        max_peers: 3,
        max_failures: 2,
        window_ms: 10_000,
        lockout_ms: 30_000,
    });
    for p in 1..=3u8 {
        assert!(churn_limiter.check_allowed(peer(p), 1_000).is_ok());
        churn_limiter.record_failure(peer(p), 1_000);
    }
    // Rotating an extra peer(4) for multiple cycles keeps all 3 slots intact and fails closed.
    for _round in 0..5 {
        assert!(churn_limiter.check_allowed(peer(4), 1_050).is_err());
        churn_limiter.record_failure(peer(4), 1_050);
        assert_eq!(churn_limiter.tracked_peer_count(), 3);
    }
    // Original tracked peers still keep their first failure count and lock out on 2nd failure.
    assert!(churn_limiter.check_allowed(peer(1), 1_100).is_ok());
    churn_limiter.record_failure(peer(1), 1_100);
    assert!(churn_limiter.check_allowed(peer(1), 1_101).is_err());
    // Unexpired failure window on peer(2) & peer(3) expires at 11_000; peer(1) stays locked until 31_100.
    assert!(churn_limiter.check_allowed(peer(4), 11_000).is_ok());
    assert!(churn_limiter.check_allowed(peer(1), 11_000).is_err());
    assert!(churn_limiter.check_allowed(peer(1), 31_100).is_ok());
}

#[tokio::test(start_paused = true)]
async fn long_ttl_token_wall_clock_jump_invalidates_active_lease_without_new_requests() {
    let clock = Arc::new(TestClock::new(1_700_000_000_000));
    let store = Arc::new(FakeSecretStore::protected());
    let f = AuthFixture::spawn(store, None, clock.clone(), RemoteAuthPolicy::default())
        .await
        .unwrap();

    // 24-hour UTC token + session + active resource lease.
    let issued = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("long-ttl").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: Some(1_700_000_000_000 + 86_400_000),
        })
        .await
        .unwrap();
    let caller = f
        .client
        .authenticate_token(peer(1), issued.token)
        .await
        .unwrap();
    let res = f.client.create_resource_session(caller.auth).await.unwrap();
    assert!(!res.lease.is_cancelled());

    // Simulate an external already-admitted business task holding a resource lease.
    let already_admitted_business_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = already_admitted_business_done.clone();
    let business_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    // Jump wall clock past 24h expiry with ZERO further requests and WITHOUT sleeping 24h.
    clock.shift_wall_ms(86_400_001);
    tokio::time::advance(Duration::from_millis(1_050)).await;
    tokio::task::yield_now().await;
    business_task.await.unwrap();

    // Both the owner lease cancelled promptly and already-admitted business work completed cleanly.
    res.lease.cancellation_token().cancelled().await;
    assert!(res.lease.is_cancelled());
    assert!(already_admitted_business_done.load(std::sync::atomic::Ordering::SeqCst));
    f.finish().await;
}

#[tokio::test(start_paused = true)]
async fn resource_session_idle_expiry_cleans_up_anonymous_and_non_expiring_bearer() {
    let clock = Arc::new(TestClock::new(1_700_000_000_000));
    let store = Arc::new(FakeSecretStore::protected());
    let policy = RemoteAuthPolicy {
        resource_session_idle_ms: 5_000,
        max_resource_sessions_per_principal: 1,
        ..Default::default()
    };
    let f = AuthFixture::spawn(store, None, clock.clone(), policy)
        .await
        .unwrap();
    let mut rx = f.client.subscribe_revocations().unwrap();

    // 1. Anonymous resource session expires on monotonic idle with NO further requests & frees cap slot.
    let anon_auth = AuthContext::new(Principal::Anonymous, ScopeSet::empty());
    let anon_res = f
        .client
        .create_resource_session(anon_auth.clone())
        .await
        .unwrap();
    assert!(matches!(
        f.client.create_resource_session(anon_auth.clone()).await,
        Err(RemoteAuthError::CapacityExceeded { .. })
    ));

    // Jump wall clock forward — monotonic idle must NOT be confused by wall time jumps.
    clock.shift_wall_ms(60_000);
    tokio::time::advance(Duration::from_millis(4_900)).await;
    tokio::task::yield_now().await;
    assert!(!anon_res.lease.is_cancelled());

    // Advance past 5_000ms monotonic idle -> automatic cancellation + revocation + freed cap!
    tokio::time::advance(Duration::from_millis(150)).await;
    anon_res.lease.cancellation_token().cancelled().await;
    let rev = rx.recv().await.unwrap();
    assert_eq!(
        rev,
        AuthRevocation::ResourceSession {
            resource_session_id: anon_res.id,
            owner_id: anon_res.owner_id,
        }
    );
    let _anon_replacement = f
        .client
        .create_resource_session(anon_auth.clone())
        .await
        .unwrap();

    // 2. Non-expiring bearer resource session refreshes ONLY on valid authorized use,
    // NEVER on wrong cookie or wrong principal.
    let bearer_issued = f
        .client
        .create_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("non-expiring").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    let bearer_caller = f
        .client
        .authenticate_token(peer(3), bearer_issued.token)
        .await
        .unwrap();
    let bearer_res = f
        .client
        .create_resource_session(bearer_caller.auth.clone())
        .await
        .unwrap();

    // Authorized use at +3_000ms pushes idle expiry out to +8_000ms.
    tokio::time::advance(Duration::from_millis(3_000)).await;
    assert!(
        f.client
            .resolve_resource_session(bearer_caller.auth.clone(), bearer_res.cookie.clone())
            .await
            .is_ok()
    );

    // At +6_000ms (past original +5_000ms), session MUST still be alive!
    tokio::time::advance(Duration::from_millis(3_000)).await;
    tokio::task::yield_now().await;
    assert!(!bearer_res.lease.is_cancelled());

    // Invalid cookie and mismatched principal at +7_500ms MUST fail and MUST NOT refresh idle deadline.
    tokio::time::advance(Duration::from_millis(1_500)).await;
    assert_eq!(
        f.client
            .resolve_resource_session(
                bearer_caller.auth.clone(),
                SecretString::new(
                    "npr_res_0000000000000000_0000000000000000000000000000000000000000000000000000000000000000"
                ),
            )
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    assert_eq!(
        f.client
            .resolve_resource_session(anon_auth, bearer_res.cookie.clone())
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );

    // At +8_050ms (5_050ms after the last authorized refresh at +3_000ms), lease MUST cancel!
    tokio::time::advance(Duration::from_millis(550)).await;
    bearer_res.lease.cancellation_token().cancelled().await;
    assert!(bearer_res.lease.is_cancelled());

    f.finish().await;
}

async fn build_os_bundle_args(
    temp: &tempfile::TempDir,
    cred: Arc<dyn nyanpasu_os::remote_auth::CredentialSecretBackend>,
    policy: RemoteAuthPolicy,
) -> crate::client::ClientSetupArgs {
    let mut args = test_client_args_with_endpoint(
        temp,
        TestControlEndpoint::succeeding(),
        crate::client::test_support::profile_test_adapters(temp),
    )
    .await;
    let bundle = nyanpasu_os::remote_auth::build_remote_auth_bundle_with_backend(
        args.paths.remote_access_secrets_path().as_std_path(),
        policy.max_persisted_record_bytes,
        cred,
    );
    args.remote_auth = Some(
        RemoteAccessAuthPorts::new(
            bundle.clock,
            bundle.entropy,
            bundle.store,
            Some(bundle.cipher),
        )
        .with_policy(policy),
    );
    args
}

#[cfg(unix)]
fn set_unix_dir_mode(path: &std::path::Path, mode: u32) {
    use std::{fs, os::unix::fs::PermissionsExt};

    fs::create_dir_all(path).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(mode);
    fs::set_permissions(path, perms).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn core_composition_with_real_os_store_persists_creates_revocations_and_keeps_keyring_lazy() {
    use std::{fs, os::unix::fs::PermissionsExt};

    let dir = tempfile::tempdir().unwrap();
    let backend =
        Arc::new(nyanpasu_os::remote_auth::test_support::InMemoryCredentialBackend::new());
    let policy = RemoteAuthPolicy::default();

    // Startup on empty data dir succeeds lazily without touching the keyring backend or pre-creating
    // `auth-secrets.json`.
    let args1 = build_os_bundle_args(&dir, backend.clone(), policy.clone()).await;
    // Force predictable 0o700 mode on `app_data_dir` inside tempdir so Unix store sees `Protected`.
    set_unix_dir_mode(args1.paths.app_data_dir().as_std_path(), 0o700);
    let secrets_path = args1.paths.remote_access_secrets_path().into_std_path_buf();
    let client1 = NyanpasuClient::try_new(args1).await.unwrap();
    assert!(!secrets_path.exists());
    assert_eq!(backend.call_counts(), (0, 0));

    let issued = client1
        .create_remote_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("os-persist").unwrap(),
            scopes: ScopeSet::from([Scope::Read, Scope::Proxy]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    assert!(secrets_path.is_file());
    let file_mode = fs::metadata(&secrets_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(file_mode, 0o600);
    // Protected plaintext persistence must not touch the OS keyring at all.
    assert_eq!(backend.call_counts(), (0, 0));
    let summary_id = issued.summary.id;
    let token_raw = issued.token.expose_secret().to_owned();
    client1.request_shutdown();
    client1.wait_shutdown().await;
    drop(client1);

    // Restart client against the same PathResolver root: token survives and authenticates cleanly.
    let client2 =
        NyanpasuClient::try_new(build_os_bundle_args(&dir, backend.clone(), policy.clone()).await)
            .await
            .unwrap();
    let listed = client2.list_remote_access_tokens().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, summary_id);
    let auth = client2
        .login_remote_with_token(peer(7), SecretString::new(token_raw.clone()))
        .await
        .unwrap();
    assert_eq!(
        auth.auth.scopes,
        ScopeSet::from([Scope::Read, Scope::Proxy])
    );

    // Revoke token, shut down, and restart: revocation persists and login stays denied.
    client2
        .revoke_remote_access_token(summary_id)
        .await
        .unwrap();
    client2.request_shutdown();
    client2.wait_shutdown().await;
    drop(client2);

    let client3 =
        NyanpasuClient::try_new(build_os_bundle_args(&dir, backend.clone(), policy.clone()).await)
            .await
            .unwrap();
    assert_eq!(
        client3
            .login_remote_with_token(peer(7), SecretString::new(token_raw))
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    client3.request_shutdown();
    client3.wait_shutdown().await;
    drop(client3);
}

#[cfg(unix)]
#[tokio::test]
async fn core_composition_with_unprotected_os_store_uses_lazy_aead_and_fails_closed_on_tamper_or_missing_key()
 {
    use std::fs;

    let dir = tempfile::tempdir().unwrap();
    let backend =
        Arc::new(nyanpasu_os::remote_auth::test_support::InMemoryCredentialBackend::new());
    let policy = RemoteAuthPolicy::default();

    // Empty relaxed startup still stays lazy (never touches keyring until a secret is persisted).
    let args1 = build_os_bundle_args(&dir, backend.clone(), policy.clone()).await;
    // Force `remote-access` to mode 0o755 so `FsSecretStore` classifies `Unprotected` and
    // requires `KeyringSecretCipher` AEAD encryption instead of plaintext.
    set_unix_dir_mode(args1.paths.remote_access_dir().as_std_path(), 0o755);
    let secrets_path = args1.paths.remote_access_secrets_path().into_std_path_buf();
    let client1 = NyanpasuClient::try_new(args1).await.unwrap();
    assert_eq!(backend.call_counts(), (0, 0));

    let issued = client1
        .create_remote_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("aead-secret-token").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    assert_eq!(backend.call_counts().1, 1);
    let raw_bytes = fs::read(&secrets_path).unwrap();
    let raw_lossy = String::from_utf8_lossy(&raw_bytes);
    assert!(
        !raw_lossy.contains("aead-secret-token") && !raw_lossy.contains("plain_v1"),
        "unprotected medium must persist ciphertext, never plaintext token metadata"
    );
    client1.request_shutdown();
    client1.wait_shutdown().await;
    drop(client1);

    // Restart with the same backend + path decrypts and authenticates cleanly.
    let client2 =
        NyanpasuClient::try_new(build_os_bundle_args(&dir, backend.clone(), policy.clone()).await)
            .await
            .unwrap();
    let auth = client2
        .login_remote_with_token(
            peer(9),
            SecretString::new(issued.token.expose_secret().to_owned()),
        )
        .await
        .unwrap();
    assert_eq!(auth.auth.scopes, ScopeSet::from([Scope::Read]));
    client2.request_shutdown();
    client2.wait_shutdown().await;
    drop(client2);

    // Restart with a fresh empty backend (missing key) MUST fail startup closed and NOT create a new key.
    let wrong_backend =
        Arc::new(nyanpasu_os::remote_auth::test_support::InMemoryCredentialBackend::new());
    let missing_key_err = match NyanpasuClient::try_new(
        build_os_bundle_args(&dir, wrong_backend.clone(), policy.clone()).await,
    )
    .await
    {
        Ok(_) => panic!("missing AEAD key for existing encrypted store must fail startup"),
        Err(err) => err,
    };
    assert!(matches!(
        missing_key_err,
        SetupError::StartRemoteAccess { .. }
    ));
    assert_eq!(wrong_backend.call_counts().1, 0);

    // Tampering a ciphertext byte on disk MUST fail startup closed with `StartRemoteAccess`.
    let mut tampered = raw_bytes.clone();
    let last_idx = tampered.len() - 1;
    tampered[last_idx] ^= 0x55;
    fs::write(&secrets_path, &tampered).unwrap();
    let tamper_err = match NyanpasuClient::try_new(
        build_os_bundle_args(&dir, backend.clone(), policy.clone()).await,
    )
    .await
    {
        Ok(_) => panic!("tampered encrypted store must fail startup closed"),
        Err(err) => err,
    };
    assert!(matches!(tamper_err, SetupError::StartRemoteAccess { .. }));

    // Restore valid ciphertext and verify tiny `max_persisted_record_bytes` policy limit refuses
    // oversized loads/writes while keeping behavior fail-closed.
    fs::write(&secrets_path, &raw_bytes).unwrap();
    let tiny_policy = RemoteAuthPolicy {
        max_persisted_record_bytes: 32,
        ..policy
    };
    let tiny_limit_err =
        match NyanpasuClient::try_new(build_os_bundle_args(&dir, backend, tiny_policy).await).await
        {
            Ok(_) => panic!("record exceeding max_persisted_record_bytes must fail startup closed"),
            Err(err) => err,
        };
    assert!(matches!(
        tiny_limit_err,
        SetupError::StartRemoteAccess { .. }
    ));
}

#[tokio::test]
async fn core_composition_with_os_bundle_survives_restart_and_fails_closed_on_corrupt_record() {
    let dir = tempfile::tempdir().unwrap();
    let backend =
        Arc::new(nyanpasu_os::remote_auth::test_support::InMemoryCredentialBackend::new());
    let policy = RemoteAuthPolicy::default();

    let args1 = build_os_bundle_args(&dir, backend.clone(), policy.clone()).await;
    let secrets_path = args1.paths.remote_access_secrets_path().into_std_path_buf();
    let client1 = NyanpasuClient::try_new(args1).await.unwrap();
    assert!(!secrets_path.exists());
    assert_eq!(backend.call_counts(), (0, 0));

    let issued = client1
        .create_remote_access_token(CreateAccessTokenRequest {
            name: TokenName::parse("cross-platform-os-token").unwrap(),
            scopes: ScopeSet::from([Scope::Read]),
            expires_at_ms: None,
        })
        .await
        .unwrap();
    assert!(secrets_path.is_file());
    let summary_id = issued.summary.id;
    let token_raw = issued.token.expose_secret().to_owned();
    client1.request_shutdown();
    client1.wait_shutdown().await;
    drop(client1);

    let client2 =
        NyanpasuClient::try_new(build_os_bundle_args(&dir, backend.clone(), policy.clone()).await)
            .await
            .unwrap();
    let auth = client2
        .login_remote_with_token(peer(11), SecretString::new(token_raw.clone()))
        .await
        .unwrap();
    assert_eq!(auth.auth.scopes, ScopeSet::from([Scope::Read]));
    client2
        .revoke_remote_access_token(summary_id)
        .await
        .unwrap();
    client2.request_shutdown();
    client2.wait_shutdown().await;
    drop(client2);

    let client3 =
        NyanpasuClient::try_new(build_os_bundle_args(&dir, backend.clone(), policy.clone()).await)
            .await
            .unwrap();
    assert_eq!(
        client3
            .login_remote_with_token(peer(11), SecretString::new(token_raw))
            .await
            .unwrap_err(),
        RemoteAuthError::Unauthorized
    );
    client3.request_shutdown();
    client3.wait_shutdown().await;
    drop(client3);

    // Corrupting the persisted file must fail `NyanpasuClient::try_new` closed rather than
    // recovering with an empty token set.
    std::fs::write(&secrets_path, b"not-valid-auth-secrets").unwrap();
    let corrupt_err =
        match NyanpasuClient::try_new(build_os_bundle_args(&dir, backend, policy).await).await {
            Ok(_) => panic!("corrupt persisted secrets file must fail startup closed"),
            Err(err) => err,
        };
    assert!(matches!(corrupt_err, SetupError::StartRemoteAccess { .. }));
}
