//! Typed client for the country index owner.
use std::sync::Arc;

use anyhow::Result;
use nyanpasu_geodata::IpIndex;
use ractor::{Actor, ActorRef};
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::actor::{GeoIndexActor, GeoIndexArgs, Message};

#[derive(Clone)]
pub struct GeoIndexClient(Arc<Inner>);

struct Inner {
    actor: ActorRef<Message>,
    index: watch::Receiver<Option<Arc<IpIndex>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}

impl GeoIndexClient {
    #[cfg(test)]
    pub(crate) async fn stop_for_test(&self) -> Result<()> {
        self.0.actor.stop_and_wait(None, None).await?;
        Ok(())
    }

    pub async fn spawn(
        args: GeoIndexArgs,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Result<Self> {
        let (published, index) = watch::channel(None);
        let (actor, _) = Actor::spawn(None, GeoIndexActor, (args, published)).await?;
        crate::client::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self(Arc::new(Inner { actor, index })))
    }

    /// The index of the database the running core reads; `None` until it loads, or while the
    /// core's home has no such database.
    pub fn subscribe(&self) -> watch::Receiver<Option<Arc<IpIndex>>> {
        self.0.index.clone()
    }

    #[cfg(test)]
    pub(super) fn send(&self, message: Message) {
        self.0
            .actor
            .cast(message)
            .expect("the geo index actor is running");
    }
}
