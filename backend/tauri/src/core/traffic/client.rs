//! Typed client for the traffic actor.
use std::sync::Arc;

use anyhow::{Context, Result};
use nyanpasu_traffic::{
    ClosedCursor, ClosedPage, Dimension, Metric, ReportRequest, TrafficFilter, TrafficQuery,
    TrafficRange, TrafficReport, TrafficSummary, UsageCursor, UsageGroup, UsagePage,
};
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

    /// Rankings, total and topology of what `request.query` selects, including what is not
    /// flushed yet.
    pub async fn report(&self, request: ReportRequest) -> Result<TrafficReport> {
        Ok(self.call(|reply| Message::Report(request, reply)).await??)
    }

    /// `limit` groups of what `query` selects, heaviest by `metric` first, strictly after `after`,
    /// including what is not flushed yet.
    pub async fn usage(
        &self,
        query: TrafficQuery,
        dimension: Dimension,
        metric: Metric,
        after: Option<UsageCursor>,
        limit: usize,
    ) -> Result<UsagePage> {
        Ok(self
            .call(|reply| Message::Usage(query, dimension, metric, after, limit, reply))
            .await??)
    }

    /// What `query` selects of `keys` that have traffic, in request order, including what is not
    /// flushed yet.
    pub async fn usage_by_keys(
        &self,
        query: TrafficQuery,
        dimension: Dimension,
        keys: Vec<String>,
    ) -> Result<Vec<UsageGroup>> {
        Ok(self
            .call(|reply| Message::UsageByKeys(query, dimension, keys, reply))
            .await??)
    }

    /// The closed connections a report over `range` and `filters` counts, newest first, strictly
    /// before `before`, including what is not flushed yet. A page may hold fewer than `limit`
    /// connections, even none, and still continue.
    pub async fn closed_connections(
        &self,
        range: TrafficRange,
        filters: Vec<TrafficFilter>,
        before: Option<ClosedCursor>,
        limit: usize,
    ) -> Result<ClosedPage> {
        Ok(self
            .call(|reply| Message::ClosedConnections(range, filters, before, limit, reply))
            .await??)
    }

    /// The ids of the live connections that satisfy every filter, sorted.
    pub async fn active_connection_ids(&self, filters: Vec<TrafficFilter>) -> Result<Vec<String>> {
        Ok(self
            .call(|reply| Message::ActiveIds(filters, reply))
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
