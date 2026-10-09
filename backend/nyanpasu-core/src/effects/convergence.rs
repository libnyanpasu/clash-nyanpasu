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

pub const RETRY_DELAYS: [Duration; 3] = [
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

/// Backoff between attempts that ended waiting on a dependency (T10 §1.8).
/// The last entry repeats.
pub(crate) const REESTABLISH_WAIT_DELAYS: [Duration; 5] = [
    Duration::from_secs(5),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

/// How an attempt ended, as far as its target's schedule is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeClass {
    /// It built, checked or submitted the target. Scheduled by the retry
    /// budget, and the only thing besides a new identity that ends waiting.
    Application,
    /// It met an absent dependency before trying anything. It spends no
    /// budget and backs off.
    Dependency,
}

/// The delay before the next attempt and the new wait count. Entering an
/// attempt resets nothing: a dependency can go missing inside one, so only
/// an application result leaves the backoff (and its delay is the retry
/// budget's to choose, so none is given here).
pub fn next_wait(waits: u8, class: OutcomeClass) -> (Duration, u8) {
    match class {
        OutcomeClass::Application => (Duration::ZERO, 0),
        OutcomeClass::Dependency => {
            let last = REESTABLISH_WAIT_DELAYS.len() - 1;
            let waits = usize::from(waits).min(last);
            (
                REESTABLISH_WAIT_DELAYS[waits],
                u8::try_from((waits + 1).min(last)).expect("the backoff table is short"),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S17: consecutive dependency results back off 5, 5, 10, 30, 60, 60 s,
    /// and one application result starts the table over.
    #[test]
    fn dependency_waits_back_off_until_an_application_result_resets_them() {
        let mut waits = 0;
        let mut delays = Vec::new();
        for _ in 0..6 {
            let (delay, next) = next_wait(waits, OutcomeClass::Dependency);
            delays.push(delay.as_secs());
            waits = next;
        }
        assert_eq!(delays, [5, 5, 10, 30, 60, 60]);
        assert_eq!(usize::from(waits), REESTABLISH_WAIT_DELAYS.len() - 1);

        let (_, reset) = next_wait(waits, OutcomeClass::Application);
        assert_eq!(reset, 0);
        assert_eq!(next_wait(reset, OutcomeClass::Dependency).0.as_secs(), 5);
    }
}
