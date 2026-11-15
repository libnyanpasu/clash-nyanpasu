use std::sync::Arc;

use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    actor::{Args, AuditActor, AuditArgs, Message},
    model::{AuditCursor, AuditFilter, AuditPage},
    ports::Error as StoreError,
};

/// Audit owner startup failure; the channel receiver is dropped on failure.
#[derive(Debug, snafu::Snafu)]
pub enum Error {
    /// The actor could not start.
    #[snafu(display("failed to spawn audit owner"))]
    SpawnActor { source: ractor::SpawnErr },
}

/// Cheap handle to the serial audit owner. Dropping its last handle stops it.
#[derive(Clone)]
pub struct AuditClient(Arc<Inner>);

struct Inner {
    actor: ActorRef<Message>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}

impl AuditClient {
    /// Starts the owner and registers its shutdown drain with the injected tracker.
    ///
    /// # Errors
    /// Returns `SpawnActor` if the actor cannot start.
    pub async fn spawn(
        audit: AuditArgs,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Result<Self, Error> {
        let (actor, _) = Actor::spawn(
            None,
            AuditActor,
            Args {
                audit,
                shutdown: shutdown.clone(),
            },
        )
        .await
        .map_err(|source| Error::SpawnActor { source })?;
        nyanpasu_owner_support::shutdown::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self(Arc::new(Inner { actor })))
    }

    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> Message,
    ) -> Result<T, StoreError> {
        match self.0.actor.call(message, None).await {
            Ok(CallResult::Success(value)) => Ok(value),
            _ => Err(StoreError::AccessStore),
        }
    }

    /// Queries persisted rows newest first, exclusively before `before`, with
    /// `limit` clamped to 1..=500. Queued diagnostic rows are not flushed by reads.
    ///
    /// # Errors
    /// Returns the store's query error, or `AccessStore` when unavailable.
    pub async fn query(
        &self,
        filter: AuditFilter,
        before: Option<AuditCursor>,
        limit: usize,
    ) -> Result<AuditPage, StoreError> {
        self.call(|reply| Message::Query(filter, before, limit, reply))
            .await?
    }

    /// Runs one tick and waits for its storage result, without advancing time.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn tick(&self) -> Result<(), StoreError> {
        self.call(Message::TickNow).await?
    }
}
