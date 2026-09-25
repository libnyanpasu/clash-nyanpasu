//! Shared vocabulary and bounded retry policy; no infrastructure or mutable globals.
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConvergenceHealth {
    Healthy,
    Pending,
    RetryScheduled,
    WaitingDependency,
    Blocked,
    RecoveryRequired,
}

pub(crate) const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(30),
];

#[derive(Debug, Clone)]
pub(crate) struct RetryBudget {
    pub remaining: u8,
    pub attempts: u32,
}
impl Default for RetryBudget {
    fn default() -> Self {
        Self {
            remaining: RETRY_DELAYS.len() as u8,
            attempts: 0,
        }
    }
}
impl RetryBudget {
    pub fn next_delay(&self) -> Option<Duration> {
        (self.remaining > 0).then(|| RETRY_DELAYS[RETRY_DELAYS.len() - usize::from(self.remaining)])
    }
}
