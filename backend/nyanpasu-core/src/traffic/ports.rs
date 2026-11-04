//! Consumer-owned ports for the traffic actor.
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    sync::Arc,
    time::Duration,
};

use nyanpasu_geodata::IpIndex;
use nyanpasu_tsdb::Profile;
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{SpawnError, TrafficClient};
use crate::clash::ws::ClashConnectionsFrame;

/// Dependencies for a traffic owner whose factory supplies its storage backend.
pub struct TrafficStartArgs {
    pub profile: Profile,
    pub profiles: Arc<dyn ProfileSelection>,
    pub retention: Arc<dyn RetentionPolicy>,
    pub clock: Arc<dyn Clock>,
    pub frames: watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
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
