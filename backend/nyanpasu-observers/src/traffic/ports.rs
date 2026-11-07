//! Consumer-owned ports for the traffic actor.
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    sync::Arc,
    time::Duration,
};

use nyanpasu_geodata::IpIndex;
use nyanpasu_traffic::Frame;
use nyanpasu_tsdb::Profile;
use ractor::ActorRef;
use tokio::{sync::watch, task::JoinHandle};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    SpawnError, TrafficClient,
    actor::{self, Message},
};

/// Converts one raw snapshot with the index, times and local addresses sampled by the pump.
pub trait FrameNormalizer: Send + Sync + 'static {
    type Raw: Send + Sync + 'static;

    fn normalize(
        &self,
        raw: &Self::Raw,
        wall_ms: i64,
        mono: Duration,
        index: Option<&IpIndex>,
        local: LocalSourceIps,
    ) -> Frame;
}

type FeedStarter = dyn FnOnce(
        ActorRef<Message>,
        watch::Receiver<Option<Arc<IpIndex>>>,
        Arc<dyn LocalSourceLocation>,
        Arc<dyn Clock>,
    ) -> JoinHandle<()>
    + Send;

/// A raw watch feed whose pump starts once, after the traffic owner opens its storage.
pub struct TrafficFeed {
    start: Box<FeedStarter>,
}

impl TrafficFeed {
    /// Captures the feed and normalizer without spawning a task or consuming a frame.
    pub fn new<N: FrameNormalizer>(
        frames: watch::Receiver<Option<Arc<N::Raw>>>,
        normalizer: Arc<N>,
    ) -> Self {
        Self {
            start: Box::new(move |actor, geo, local_source, clock| {
                tokio::spawn(actor::pump(
                    actor,
                    frames,
                    geo,
                    local_source,
                    clock,
                    normalizer,
                ))
            }),
        }
    }

    pub(super) fn start(
        self,
        actor: ActorRef<Message>,
        geo: watch::Receiver<Option<Arc<IpIndex>>>,
        local_source: Arc<dyn LocalSourceLocation>,
        clock: Arc<dyn Clock>,
    ) -> JoinHandle<()> {
        (self.start)(actor, geo, local_source, clock)
    }
}

/// Dependencies for a traffic owner whose factory supplies its storage backend.
pub struct TrafficStartArgs {
    pub profile: Profile,
    pub profiles: Arc<dyn ProfileSelection>,
    pub retention: Arc<dyn RetentionPolicy>,
    pub clock: Arc<dyn Clock>,
    pub frames: TrafficFeed,
    /// The country index regions are looked up in; `None` until the core's database loads.
    pub geo: watch::Receiver<Option<Arc<IpIndex>>>,
    pub local_source: Arc<dyn LocalSourceLocation>,
}

/// Starts a traffic owner with the factory's storage backend and startup policy.
#[async_trait::async_trait]
pub trait TrafficOwnerFactory: Send + Sync {
    /// Opens the dataset during owner startup and registers its shutdown with `tasks`.
    ///
    /// # Errors
    /// Returns the owner's existing spawn failure if startup cannot complete.
    async fn spawn(
        self: Arc<Self>,
        args: TrafficStartArgs,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Result<TrafficClient, SpawnError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LocalSourceIps {
    pub ipv4: Option<Ipv4Addr>,
    pub ipv6: Option<Ipv6Addr>,
}

/// A nonblocking view of the permitted, caller-probed local public addresses.
pub trait LocalSourceLocation: Send + Sync + 'static {
    fn addresses(&self) -> LocalSourceIps;
}

/// The selected profile; labels the connections first seen while it is selected.
#[cfg_attr(test, mockall::automock)]
pub trait ProfileSelection: Send + Sync + 'static {
    fn current(&self) -> Option<String>;
}

/// How long recorded traffic is kept, read afresh at every flush.
#[cfg_attr(test, mockall::automock)]
pub trait RetentionPolicy: Send + Sync + 'static {
    /// `None` keeps it forever.
    fn retention(&self) -> Option<Duration>;
}

/// The wall clock: buckets, close times and retention are all counted from it.
#[cfg_attr(test, mockall::automock)]
pub trait Clock: Send + Sync + 'static {
    /// Milliseconds since the Unix epoch.
    fn now_ms(&self) -> i64;
}

/// Opens the same dataset at startup and after tsdb retires a failed backend.
pub trait TrafficStorage<S: nyanpasu_tsdb::storage::Storage>: Send + Sync + 'static {
    fn open(&self) -> Result<S, nyanpasu_tsdb::storage::StorageError>;

    /// Startup policy may recreate disposable statistics; recovery never deletes the dataset.
    fn initialize(
        &self,
        profile: nyanpasu_tsdb::Profile,
    ) -> Result<nyanpasu_traffic::store::Opened<S>, nyanpasu_tsdb::RecoveryError> {
        let storage = self
            .open()
            .map_err(|source| nyanpasu_tsdb::RecoveryError::Reopen { source })?;
        nyanpasu_traffic::TrafficDb::open(storage, profile)
    }
}
