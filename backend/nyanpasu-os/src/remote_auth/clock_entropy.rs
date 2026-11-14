//! Per-instance OS wall/monotonic clock and CSPRNG entropy adapters.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use nyanpasu_contracts::auth::{AuthClock, EntropyError, EntropySource};

/// Monotonic-anchored wall and relative millisecond clock backed by the OS.
#[derive(Debug)]
pub struct OsAuthClock {
    start: Instant,
}

impl OsAuthClock {
    /// Construct a fresh clock instance anchored at creation time.
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
}

impl Default for OsAuthClock {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthClock for OsAuthClock {
    fn now_unix_ms(&self) -> i64 {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        i64::try_from(millis).unwrap_or(i64::MAX)
    }

    fn monotonic_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// Cryptographic randomness source backed by the OS CSPRNG (`getrandom`).
#[derive(Debug, Clone, Default)]
pub struct OsEntropySource;

impl EntropySource for OsEntropySource {
    fn fill_bytes(&self, dest: &mut [u8]) -> Result<(), EntropyError> {
        if dest.is_empty() {
            return Ok(());
        }
        getrandom::fill(dest).map_err(|_| EntropyError {
            reason: "OS CSPRNG fill failed".to_owned(),
        })
    }
}
