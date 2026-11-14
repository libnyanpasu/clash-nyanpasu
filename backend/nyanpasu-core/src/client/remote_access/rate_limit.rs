//! Bounded per-peer authentication rate limiting on an injected monotonic clock.

use std::collections::HashMap;

use nyanpasu_contracts::auth::AuthPeer;

use super::error::RemoteAuthError;

/// Policy governing per-peer failed authentication lockouts and memory bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerRateLimitConfig {
    /// Maximum tracked peer entries (`>= 1`).
    pub max_peers: usize,
    /// Consecutive failures inside `window_ms` before triggering a lockout (`>= 1`).
    pub max_failures: u32,
    /// Rolling failure window duration in monotonic milliseconds (`>= 1`).
    pub window_ms: u64,
    /// Lockout duration in monotonic milliseconds once `max_failures` is reached (`>= 1`).
    pub lockout_ms: u64,
}

impl PeerRateLimitConfig {
    /// Validate non-zero bounds for rate limiting configuration.
    pub fn validate(&self) -> Result<(), RemoteAuthError> {
        if self.max_peers == 0
            || self.max_failures == 0
            || self.window_ms == 0
            || self.lockout_ms == 0
        {
            return Err(RemoteAuthError::InvalidInput {
                reason: "rate limit policy max_peers, max_failures, window_ms, and lockout_ms must all be >= 1".to_owned(),
            });
        }
        Ok(())
    }
}

impl Default for PeerRateLimitConfig {
    fn default() -> Self {
        Self {
            max_peers: 1024,
            max_failures: 5,
            window_ms: 60_000,
            lockout_ms: 60_000,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PeerFailureState {
    window_start_mono_ms: u64,
    failures: u32,
    locked_until_mono_ms: Option<u64>,
}

impl PeerFailureState {
    fn is_expired(&self, now_mono_ms: u64, window_ms: u64) -> bool {
        if let Some(until) = self.locked_until_mono_ms {
            return now_mono_ms >= until;
        }
        now_mono_ms.saturating_sub(self.window_start_mono_ms) >= window_ms
    }

    fn next_prune_or_unlock_mono_ms(&self, window_ms: u64) -> u64 {
        self.locked_until_mono_ms
            .unwrap_or_else(|| self.window_start_mono_ms.saturating_add(window_ms))
    }
}

/// Bounded map tracking failed authentication attempts per `AuthPeer`.
///
/// Uses only monotonic clock timestamps (`u64` ms) so wall-clock jumps cannot prematurely
/// clear or extend lockouts. Expired entries are pruned lazily; when all `max_peers` slots
/// contain unexpired failure windows or active lockouts, unknown peers are refused
/// fail-closed until the earliest window or lockout expires so peer churn can neither erase
/// active lockouts nor reset failure counters.
#[derive(Debug)]
pub(super) struct PeerRateLimiter {
    config: PeerRateLimitConfig,
    peers: HashMap<AuthPeer, PeerFailureState>,
}

impl PeerRateLimiter {
    pub(super) fn new(config: PeerRateLimitConfig) -> Self {
        // Defense-in-depth clamp so internal construction cannot silently disable limiting.
        let safe_config = PeerRateLimitConfig {
            max_peers: config.max_peers.max(1),
            max_failures: config.max_failures.max(1),
            window_ms: config.window_ms.max(1),
            lockout_ms: config.lockout_ms.max(1),
        };
        Self {
            config: safe_config,
            peers: HashMap::new(),
        }
    }

    pub(super) fn check_allowed(&mut self, peer: AuthPeer, now_mono_ms: u64) -> Result<(), u64> {
        if let Some(state) = self.peers.get(&peer).copied() {
            if let Some(locked_until) = state.locked_until_mono_ms {
                if now_mono_ms < locked_until {
                    return Err(locked_until - now_mono_ms);
                }
                self.peers.remove(&peer);
                return Ok(());
            }
            if state.is_expired(now_mono_ms, self.config.window_ms) {
                self.peers.remove(&peer);
            }
            return Ok(());
        }

        // Unknown peer while at capacity: prune expired entries first; if all `max_peers` slots
        // hold unexpired failure windows or active lockouts, fail closed rather than resetting or
        // skipping tracked peers.
        if self.peers.len() >= self.config.max_peers {
            self.prune_expired(now_mono_ms);
            if self.peers.len() >= self.config.max_peers {
                let earliest_free = self
                    .peers
                    .values()
                    .map(|state| state.next_prune_or_unlock_mono_ms(self.config.window_ms))
                    .min()
                    .unwrap_or_else(|| now_mono_ms.saturating_add(1));
                return Err(earliest_free.saturating_sub(now_mono_ms).max(1));
            }
        }
        Ok(())
    }

    pub(super) fn record_failure(&mut self, peer: AuthPeer, now_mono_ms: u64) {
        if !self.peers.contains_key(&peer) && self.peers.len() >= self.config.max_peers {
            self.prune_expired(now_mono_ms);
            if self.peers.len() >= self.config.max_peers {
                return;
            }
        }

        let entry = self.peers.entry(peer).or_insert(PeerFailureState {
            window_start_mono_ms: now_mono_ms,
            failures: 0,
            locked_until_mono_ms: None,
        });

        if let Some(until) = entry.locked_until_mono_ms {
            if now_mono_ms < until {
                return;
            }
            entry.locked_until_mono_ms = None;
            entry.failures = 0;
            entry.window_start_mono_ms = now_mono_ms;
        } else if now_mono_ms.saturating_sub(entry.window_start_mono_ms) >= self.config.window_ms {
            entry.failures = 0;
            entry.window_start_mono_ms = now_mono_ms;
        }

        entry.failures = entry.failures.saturating_add(1);
        if entry.failures >= self.config.max_failures {
            entry.locked_until_mono_ms = Some(now_mono_ms.saturating_add(self.config.lockout_ms));
        }
    }

    pub(super) fn record_success(&mut self, peer: AuthPeer) {
        self.peers.remove(&peer);
    }

    #[cfg(test)]
    pub(super) fn tracked_peer_count(&self) -> usize {
        self.peers.len()
    }

    fn prune_expired(&mut self, now_mono_ms: u64) {
        let window_ms = self.config.window_ms;
        self.peers
            .retain(|_, state| !state.is_expired(now_mono_ms, window_ms));
    }
}
