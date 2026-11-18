//! Deterministic fakes of the authentication ports for tests of the owner and its transports.

use std::{
    sync::{
        Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use zeroize::Zeroizing;

use super::{
    AuthClock, EntropyError, EntropySource, SecretStore, SecretStoreError, TlsCertificateGenerator,
    TlsCertificateRequest, TlsGenerateError, TlsIdentity,
};

const START_UNIX_MS: u64 = 1_700_000_000_000;

/// A clock that only moves when a test advances it.
#[derive(Debug, Default)]
pub struct ManualAuthClock {
    elapsed_ms: AtomicU64,
}

impl ManualAuthClock {
    /// Moves both the wall and the monotonic time forward.
    pub fn advance(&self, by: Duration) {
        let millis = u64::try_from(by.as_millis()).expect("test durations fit in u64 milliseconds");
        self.elapsed_ms.fetch_add(millis, Ordering::SeqCst);
    }
}

impl AuthClock for ManualAuthClock {
    fn now_unix_ms(&self) -> i64 {
        i64::try_from(START_UNIX_MS + self.monotonic_ms()).expect("test time fits in i64")
    }

    fn monotonic_ms(&self) -> u64 {
        self.elapsed_ms.load(Ordering::SeqCst)
    }
}

/// Distinct, reproducible bytes on every fill.
#[derive(Debug, Default)]
pub struct CountingEntropy(AtomicU8);

impl EntropySource for CountingEntropy {
    fn fill_bytes(&self, dest: &mut [u8]) -> Result<(), EntropyError> {
        let seed = self.0.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        for (offset, byte) in dest.iter_mut().enumerate() {
            *byte = seed
                .wrapping_mul(31)
                .wrapping_add(u8::try_from(offset % 251).expect("offset is below 251"));
        }
        Ok(())
    }
}

/// An in-memory secret record.
#[derive(Debug, Default)]
pub struct MemorySecretStore {
    record: Mutex<Option<Vec<u8>>>,
}

#[async_trait]
impl SecretStore for MemorySecretStore {
    async fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        let record = self.record.lock().expect("no holder of the record panics");
        Ok(record.clone().map(Zeroizing::new))
    }

    async fn save(&self, bytes: &[u8]) -> Result<(), SecretStoreError> {
        *self.record.lock().expect("no holder of the record panics") = Some(bytes.to_vec());
        Ok(())
    }
}

/// A generator of distinguishable placeholder identities that remembers what it was asked for.
///
/// The bytes are not a real certificate, so only code that never parses them may use it.
#[derive(Debug, Default)]
pub struct FakeTlsGenerator {
    requests: Mutex<Vec<TlsCertificateRequest>>,
}

impl FakeTlsGenerator {
    /// Every request so far, oldest first.
    pub fn requests(&self) -> Vec<TlsCertificateRequest> {
        self.requests
            .lock()
            .expect("no holder of the requests panics")
            .clone()
    }
}

#[async_trait]
impl TlsCertificateGenerator for FakeTlsGenerator {
    async fn generate(
        &self,
        request: TlsCertificateRequest,
    ) -> Result<TlsIdentity, TlsGenerateError> {
        let mut requests = self
            .requests
            .lock()
            .expect("no holder of the requests panics");
        let serial = requests.len() + 1;
        let not_after_ms = request.not_after_ms;
        let addresses = request.addresses.clone();
        requests.push(request);
        Ok(TlsIdentity::new(
            format!("certificate-{serial}").into_bytes(),
            format!("private-key-{serial}").into_bytes(),
            vec!["localhost".to_owned()],
            addresses,
            not_after_ms,
        ))
    }
}
