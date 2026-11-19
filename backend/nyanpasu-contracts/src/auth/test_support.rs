//! Deterministic fakes of the authentication ports for tests of the owner and its transports.

use std::{
    net::IpAddr,
    sync::{
        Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use zeroize::Zeroizing;

use super::{
    AuthClock, EntropyError, EntropySource, SecretStore, SecretStoreError, TlsAuthority,
    TlsCertificateGenerator, TlsCertificateRequest, TlsGenerateError, TlsIdentity,
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

/// A generator of distinguishable placeholder certificates that remembers what it was asked for.
///
/// The bytes are not real certificates, so only code that never parses them may use it.
#[derive(Debug, Default)]
pub struct FakeTlsGenerator {
    authorities: Mutex<Vec<TlsCertificateRequest>>,
    issued: Mutex<Vec<TlsCertificateRequest>>,
    addresses: Mutex<Vec<IpAddr>>,
}

impl FakeTlsGenerator {
    /// Every root request so far, oldest first.
    pub fn authority_requests(&self) -> Vec<TlsCertificateRequest> {
        self.authorities
            .lock()
            .expect("no holder of the requests panics")
            .clone()
    }

    /// Every server certificate request so far, oldest first.
    pub fn issue_requests(&self) -> Vec<TlsCertificateRequest> {
        self.issued
            .lock()
            .expect("no holder of the requests panics")
            .clone()
    }

    /// Changes the addresses the host reports, as a new network would.
    pub fn set_addresses(&self, addresses: Vec<IpAddr>) {
        *self
            .addresses
            .lock()
            .expect("no holder of the addresses panics") = addresses;
    }
}

#[async_trait]
impl TlsCertificateGenerator for FakeTlsGenerator {
    async fn generate_authority(
        &self,
        request: TlsCertificateRequest,
    ) -> Result<TlsAuthority, TlsGenerateError> {
        let mut requests = self
            .authorities
            .lock()
            .expect("no holder of the requests panics");
        requests.push(request);
        let serial = requests.len();
        Ok(TlsAuthority::new(
            format!("authority-{serial}").into_bytes(),
            format!("authority-key-{serial}").into_bytes(),
            request.not_after_ms,
        ))
    }

    async fn issue(
        &self,
        authority: &TlsAuthority,
        request: TlsCertificateRequest,
    ) -> Result<TlsIdentity, TlsGenerateError> {
        let mut requests = self
            .issued
            .lock()
            .expect("no holder of the requests panics");
        requests.push(request);
        let serial = requests.len();
        Ok(TlsIdentity::new(
            format!("certificate-{serial}").into_bytes(),
            format!("private-key-{serial}").into_bytes(),
            authority.certificate_der().to_vec(),
            vec!["localhost".to_owned()],
            self.addresses(),
            request.not_after_ms,
        ))
    }

    fn addresses(&self) -> Vec<IpAddr> {
        self.addresses
            .lock()
            .expect("no holder of the addresses panics")
            .clone()
    }
}
