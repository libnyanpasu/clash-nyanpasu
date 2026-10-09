//! Consumer-owned port for the core's country database.
use std::{path::PathBuf, sync::Arc};

use nyanpasu_geodata::{GeoError, IpIndex};
use snafu::Snafu;

/// Which country database the core reads: `geodata-mode` in its running config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeodataMode {
    /// `Country.mmdb`, `geoip.db` or `geoip.metadb`.
    Mmdb,
    /// `GeoIP.dat`.
    Dat,
}

/// What a published index was built from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexKey {
    pub mode: GeodataMode,
    pub sha256: [u8; 32],
}

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

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(super)))]
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
    WatchHome {
        source: notify_debouncer_full::notify::Error,
    },
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

#[cfg_attr(test, mockall::automock)]
pub trait CountryIndexSource: Send + Sync + 'static {
    /// Blocking: reads and hashes the database, then reopens or builds its index.
    fn load(&self, mode: GeodataMode, current: Option<IndexKey>) -> Result<Loaded, GeoIndexError>;

    /// Notifies `changed` after a country database in the core's home is written; watching ends
    /// when the returned guard drops.
    fn watch(&self, changed: OnChange) -> Result<Box<dyn Send>, GeoIndexError>;
}
