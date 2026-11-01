//! Revocable session-end input supplied by the application composition root.

use nyanpasu_utils::process::{ProcessActivity, ProcessActivitySource, ProcessActivityState};
use tokio::sync::watch;

/// Publishes whether the operating system is preparing to end the session.
#[derive(Clone, Debug)]
pub struct SessionEndSource {
    activity: ProcessActivitySource,
    lifetime: watch::Sender<()>,
}

/// Reads session-end intent without cancelling work already owned by an actor.
#[derive(Clone, Debug)]
pub struct SessionEnding {
    activity: ProcessActivity,
    lifetime: watch::Receiver<()>,
}

impl Default for SessionEndSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionEndSource {
    /// Creates an independent session signal, initially inactive.
    pub fn new() -> Self {
        Self {
            activity: ProcessActivitySource::new(),
            lifetime: watch::channel(()).0,
        }
    }

    /// Changes the session intent, retaining it even before a reader subscribes.
    pub fn set_pending(&self, pending: bool) {
        self.activity.set(if pending {
            ProcessActivityState::Quiescing
        } else {
            ProcessActivityState::Active
        });
    }

    /// Creates a reader of this source's current and future intent.
    pub fn subscribe(&self) -> SessionEnding {
        SessionEnding {
            activity: self.activity.subscribe(),
            lifetime: self.lifetime.subscribe(),
        }
    }
}

impl SessionEnding {
    /// Whether a session-end query is pending or has been confirmed.
    pub fn is_pending(&self) -> bool {
        self.activity.state() == ProcessActivityState::Quiescing
    }

    /// Waits for a change; false means the source has been dropped.
    pub async fn changed(&mut self) -> bool {
        tokio::select! {
            biased;
            _ = self.activity.changed() => true,
            _ = self.lifetime.changed() => false,
        }
    }

    pub(crate) fn process_activity(&self) -> ProcessActivity {
        self.activity.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn signal_is_revocable_and_instance_local() {
        use futures_util::FutureExt;

        let source = SessionEndSource::new();
        let other = SessionEndSource::new();
        let mut reader = source.subscribe();
        assert!(!reader.is_pending());
        source.set_pending(true);
        assert!(reader.changed().await);
        assert!(reader.is_pending());
        assert!(!other.subscribe().is_pending());
        source.set_pending(false);
        assert!(reader.changed().await);
        assert!(!reader.is_pending());
        assert!(reader.changed().now_or_never().is_none());
        drop(source);
        assert!(!reader.changed().await);
    }

    #[test]
    fn publishing_before_subscription_is_retained() {
        let source = SessionEndSource::new();
        source.set_pending(true);
        let reader = source.subscribe();
        let activity = reader.process_activity();
        assert!(reader.is_pending());
        assert_eq!(activity.state(), ProcessActivityState::Quiescing);
        source.set_pending(false);
        assert!(!reader.is_pending());
        assert_eq!(activity.state(), ProcessActivityState::Active);
    }
}
