//! Revocable session-end input supplied by the application composition root.

use tokio::sync::watch;

/// Publishes whether the operating system is preparing to end the session.
#[derive(Clone, Debug)]
pub struct SessionEndSource(watch::Sender<bool>);

/// Reads session-end intent without cancelling work already owned by an actor.
#[derive(Clone, Debug)]
pub struct SessionEnding(watch::Receiver<bool>);

impl Default for SessionEndSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionEndSource {
    /// Creates an independent session signal, initially inactive.
    pub fn new() -> Self {
        Self(watch::channel(false).0)
    }

    /// Changes the session intent, retaining it even before a reader subscribes.
    pub fn set_pending(&self, pending: bool) {
        self.0.send_if_modified(|current| {
            if *current == pending {
                false
            } else {
                *current = pending;
                true
            }
        });
    }

    /// Creates a reader of this source's current and future intent.
    pub fn subscribe(&self) -> SessionEnding {
        SessionEnding(self.0.subscribe())
    }
}

impl SessionEnding {
    /// Whether a session-end query is pending or has been confirmed.
    pub fn is_pending(&self) -> bool {
        *self.0.borrow()
    }

    /// Waits for a change; false means the source has been dropped.
    pub async fn changed(&mut self) -> bool {
        self.0.changed().await.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn signal_is_revocable_and_instance_local() {
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
    }

    #[test]
    fn publishing_before_subscription_is_retained() {
        let source = SessionEndSource::new();
        source.set_pending(true);
        assert!(source.subscribe().is_pending());
    }
}
