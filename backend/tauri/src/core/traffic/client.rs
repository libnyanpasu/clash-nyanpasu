//! Typed client for the traffic actor.
use std::sync::Arc;

use anyhow::{Context, Result};
use nyanpasu_traffic::{ClosedCursor, ClosedPage, GroupBy, Topology, TrafficSummary, Usage};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::actor::{Message, TrafficActor, TrafficArgs};

#[derive(Clone)]
pub struct TrafficClient(Arc<Inner>);

struct Inner {
    actor: ActorRef<Message>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}

impl TrafficClient {
    pub async fn spawn(
        args: TrafficArgs,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Result<Self> {
        let (actor, _) = Actor::spawn(None, TrafficActor, args).await?;
        crate::client::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self(Arc::new(Inner { actor })))
    }

    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> Message,
    ) -> Result<T> {
        match self
            .0
            .actor
            .call(message, None)
            .await
            .context("traffic actor unavailable")?
        {
            CallResult::Success(value) => Ok(value),
            _ => anyhow::bail!("traffic actor reply dropped"),
        }
    }

    pub async fn summary(&self) -> Result<TrafficSummary> {
        Ok(self.call(Message::Summary).await??)
    }

    /// Top `limit` groups of the session so far, including what is not flushed yet.
    pub async fn usage(&self, group: GroupBy, limit: usize) -> Result<Usage> {
        Ok(self
            .call(|reply| Message::Usage(group, limit, reply))
            .await??)
    }

    /// Top `limit` paths of the session so far, including what is not flushed yet.
    pub async fn topology(&self, limit: usize) -> Result<Topology> {
        Ok(self.call(|reply| Message::Topology(limit, reply)).await??)
    }

    /// Newest first, strictly before `before`, including what is not flushed yet.
    pub async fn closed_connections(
        &self,
        before: Option<ClosedCursor>,
        limit: usize,
    ) -> Result<ClosedPage> {
        Ok(self
            .call(|reply| Message::ClosedConnections(before, limit, reply))
            .await??)
    }

    /// Delivers what the pump would: a frame, or `None` for a lost feed.
    #[cfg(test)]
    pub(super) async fn observe(&self, frame: Option<nyanpasu_traffic::Frame>) -> Result<()> {
        match frame {
            Some(frame) => self.call(|reply| Message::Observe(frame, reply)).await,
            None => self.call(Message::Disconnected).await,
        }
    }

    #[cfg(test)]
    pub(super) async fn flush(&self) -> Result<()> {
        self.call(Message::Flush).await
    }
}
