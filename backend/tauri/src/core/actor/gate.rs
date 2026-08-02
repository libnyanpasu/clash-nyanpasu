use std::{collections::VecDeque, time::Instant};

use ractor::RpcReplyPort;

use super::OperationId;

pub(super) struct OperationGate {
    pub(super) active: Option<ActiveOperation>,
    pub(super) waiters: VecDeque<OperationWaiter>,
}

impl OperationGate {
    pub(super) fn new() -> Self {
        Self {
            active: None,
            waiters: VecDeque::new(),
        }
    }

    pub(super) fn acquire(
        &mut self,
        id: OperationId,
        reply: RpcReplyPort<Result<(), OperationError>>,
    ) {
        if self.active.is_some() {
            self.waiters.push_back(OperationWaiter { id, reply });
            return;
        }
        self.grant(id, reply);
    }

    pub(super) fn release(&mut self, id: OperationId) {
        if self.active.as_ref().is_some_and(|active| active.id == id) {
            if let Some(active) = self.active.take() {
                tracing::debug!(?id, held_for = ?active.acquired_at.elapsed(), "released core operation");
            }
            self.grant_next();
            return;
        }
        let before = self.waiters.len();
        self.waiters.retain(|waiter| waiter.id != id);
        if before == self.waiters.len() {
            tracing::debug!(?id, "ignored stale core operation release");
        }
    }

    pub(super) fn is_active(&self, id: OperationId) -> bool {
        self.active.as_ref().is_some_and(|active| active.id == id)
    }

    pub(super) fn is_idle(&self) -> bool {
        self.active.is_none()
    }

    pub(super) fn shutdown(&mut self) {
        self.active = None;
        for waiter in self.waiters.drain(..) {
            let _ = waiter.reply.send(Err(OperationError::ShuttingDown));
        }
    }

    fn grant(&mut self, id: OperationId, reply: RpcReplyPort<Result<(), OperationError>>) {
        self.active = Some(ActiveOperation {
            id,
            acquired_at: Instant::now(),
        });
        if reply.send(Ok(())).is_err() {
            self.active = None;
            self.grant_next();
        }
    }

    fn grant_next(&mut self) {
        while let Some(waiter) = self.waiters.pop_front() {
            self.active = Some(ActiveOperation {
                id: waiter.id,
                acquired_at: Instant::now(),
            });
            if waiter.reply.send(Ok(())).is_ok() {
                return;
            }
            self.active = None;
        }
    }
}

pub(super) struct ActiveOperation {
    pub(super) id: OperationId,
    pub(super) acquired_at: Instant,
}

pub(super) struct OperationWaiter {
    pub(super) id: OperationId,
    pub(super) reply: RpcReplyPort<Result<(), OperationError>>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum OperationError {
    #[error("timed out acquiring the core operation")]
    AcquireTimeout,
    #[error("core actor is shutting down")]
    ShuttingDown,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> OperationId {
        OperationId::new(value).unwrap()
    }

    fn waiter() -> (
        RpcReplyPort<Result<(), OperationError>>,
        tokio::sync::oneshot::Receiver<Result<(), OperationError>>,
    ) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        (sender.into(), receiver)
    }

    #[tokio::test]
    async fn waiters_are_granted_in_fifo_order() {
        let mut gate = OperationGate::new();
        let (first_tx, first_rx) = waiter();
        let (second_tx, mut second_rx) = waiter();
        let (third_tx, mut third_rx) = waiter();
        gate.acquire(id(1), first_tx);
        gate.acquire(id(2), second_tx);
        gate.acquire(id(3), third_tx);
        assert!(first_rx.await.unwrap().is_ok());
        assert!(second_rx.try_recv().is_err());
        assert!(third_rx.try_recv().is_err());
        gate.release(id(1));
        assert!(second_rx.await.unwrap().is_ok());
        assert!(third_rx.try_recv().is_err());
        gate.release(id(2));
        assert!(third_rx.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn dropping_a_waiting_guard_cancels_it() {
        let mut gate = OperationGate::new();
        let (first_tx, first_rx) = waiter();
        let (second_tx, second_rx) = waiter();
        let (third_tx, third_rx) = waiter();
        gate.acquire(id(1), first_tx);
        gate.acquire(id(2), second_tx);
        gate.acquire(id(3), third_tx);
        assert!(first_rx.await.unwrap().is_ok());
        drop(second_rx);
        gate.release(id(2));
        gate.release(id(1));
        assert!(third_rx.await.unwrap().is_ok());
        assert!(gate.is_active(id(3)));
    }

    #[tokio::test]
    async fn guard_dropped_right_after_grant_releases_active() {
        let mut gate = OperationGate::new();
        let (first_tx, first_rx) = waiter();
        let (second_tx, second_rx) = waiter();
        let (third_tx, third_rx) = waiter();
        gate.acquire(id(1), first_tx);
        gate.acquire(id(2), second_tx);
        gate.acquire(id(3), third_tx);
        assert!(first_rx.await.unwrap().is_ok());
        gate.release(id(1));
        assert!(second_rx.await.unwrap().is_ok());
        gate.release(id(2));
        assert!(third_rx.await.unwrap().is_ok());
        assert!(gate.is_active(id(3)));
    }

    #[tokio::test]
    async fn stale_release_is_idempotent_noop() {
        let mut gate = OperationGate::new();
        let (active_tx, active_rx) = waiter();
        gate.acquire(id(1), active_tx);
        assert!(active_rx.await.unwrap().is_ok());
        gate.release(id(99));
        assert!(gate.is_active(id(1)));
        gate.release(id(1));
        gate.release(id(1));
        assert!(gate.is_idle());
    }

    #[tokio::test]
    async fn shutdown_drains_all_waiters() {
        let mut gate = OperationGate::new();
        let (first_tx, first_rx) = waiter();
        let (second_tx, second_rx) = waiter();
        let (third_tx, third_rx) = waiter();
        gate.acquire(id(1), first_tx);
        gate.acquire(id(2), second_tx);
        gate.acquire(id(3), third_tx);
        assert!(first_rx.await.unwrap().is_ok());
        gate.shutdown();
        assert!(matches!(
            second_rx.await.unwrap(),
            Err(OperationError::ShuttingDown)
        ));
        assert!(matches!(
            third_rx.await.unwrap(),
            Err(OperationError::ShuttingDown)
        ));
        assert!(gate.is_idle());
    }
}
