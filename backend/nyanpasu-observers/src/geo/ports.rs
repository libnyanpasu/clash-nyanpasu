//! Consumer-owned port for the core's country database.
use std::{path::PathBuf, sync::Arc};

use nyanpasu_contracts::BoxedCause;
use nyanpasu_geodata::{GeoError, IpIndex};
use ractor::ActorRef;
use snafu::Snafu;
use tokio::task::JoinHandle;

use super::actor::Message;

/// Which country database the core reads: `geodata-mode` in its running config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeodataMode {
    /// `Country.mmdb`, `geoip.db` or `geoip.metadb`.
    Mmdb,
    /// `GeoIP.dat`.
    Dat,
}

/// Starts mode observation; the owner aborts and awaits the returned task on shutdown.
pub trait ModeSource: Send + Sync + 'static {
    fn start(&self, reporter: ModeReporter) -> JoinHandle<()>;
}

/// Delivers database modes without exposing the owner's mailbox.
#[derive(Clone)]
pub struct ModeReporter(Arc<dyn Fn(GeodataMode) -> bool + Send + Sync>);

impl ModeReporter {
    pub(super) fn new(actor: ActorRef<Message>) -> Self {
        Self(Arc::new(move |mode| {
            actor.cast(Message::Mode(mode)).is_ok()
        }))
    }

    /// Returns false when the owner can no longer receive mode reports.
    pub fn report(&self, mode: GeodataMode) -> bool {
        (self.0)(mode)
    }

    /// Observes delivery and controls acceptance in adapter tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_for_test(callback: impl Fn(GeodataMode) -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(callback))
    }
}

/// What a published index was built from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexKey {
    pub mode: GeodataMode,
    pub sha256: [u8; 32],
}

/// The result of loading a database relative to the currently published index.
pub enum Loaded {
    /// The database still has the content of the current index.
    Unchanged,
    /// The core's home has no database for this mode.
    Missing,
    Index {
        key: IndexKey,
        index: Arc<IpIndex>,
    },
}

/// A database read, index build, or watch failure from the injected source.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum GeoIndexError {
    #[snafu(display("could not list the core's home"))]
    ListHome { source: std::io::Error },
    #[snafu(display("could not read {}", path.display()))]
    ReadDatabase {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("could not index {}", path.display()))]
    IndexDatabase { path: PathBuf, source: GeoError },
    #[snafu(display("could not watch the core's home"))]
    WatchHome { source: BoxedCause },
}

/// Called after a country database in the core's home is written.
pub struct OnChange(Box<dyn Fn() + Send + Sync>);

impl OnChange {
    pub fn new(callback: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Box::new(callback))
    }

    pub fn notify(&self) {
        (self.0)()
    }
}

/// Loads the country index and reports database writes to its serial owner.
#[cfg_attr(test, mockall::automock)]
pub trait CountryIndexSource: Send + Sync + 'static {
    /// Blocking: reads and hashes the database, then reopens or builds its index.
    fn load(&self, mode: GeodataMode, current: Option<IndexKey>) -> Result<Loaded, GeoIndexError>;

    /// Notifies `changed` after a country database in the core's home is written; watching ends
    /// when the returned guard drops.
    fn watch(&self, changed: OnChange) -> Result<Box<dyn Send>, GeoIndexError>;
}

/// A home without databases, for tests whose subject is not the index.
#[cfg(any(test, feature = "test-support"))]
pub struct NoopCountryIndexSource;

#[cfg(any(test, feature = "test-support"))]
impl CountryIndexSource for NoopCountryIndexSource {
    fn load(&self, _: GeodataMode, _: Option<IndexKey>) -> Result<Loaded, GeoIndexError> {
        Ok(Loaded::Missing)
    }

    fn watch(&self, _: OnChange) -> Result<Box<dyn Send>, GeoIndexError> {
        Ok(Box::new(()))
    }
}
