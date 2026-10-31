//! Host-provided adapters consumed while assembling the application facade.

use std::sync::Arc;

use futures::future::BoxFuture;

use super::{
    app_update::changelog::ChangelogHttp, application_workflow::ports::RuntimeBuildPort,
    debug_http::DebugHttpControl,
};
use crate::{
    service::profile_file::ports::SelfProxyPortSource,
    state::profiles::ports::{ProfileFsPort, ProfileMaterializationPort, SubscriptionFetcher},
};

/// Matching profile-store capabilities returned by the host's profile factory.
pub struct ProfileFilePorts {
    /// Filesystem access shared by the profiles owner and facade.
    pub fs: Arc<dyn ProfileFsPort>,
    /// Subscription downloading supplied to the profiles owner.
    pub fetcher: Arc<dyn SubscriptionFetcher>,
    /// Transactional file materialization supplied to the profiles owner.
    pub materialization: Arc<dyn ProfileMaterializationPort>,
}

/// Builds matching host profile-store ports after session setup and before profiles startup.
pub type ProfileFileFactory =
    dyn Fn(Arc<dyn SelfProxyPortSource>) -> ProfileFilePorts + Send + Sync;

/// Builds host changelog HTTP access from the session proxy source after app-updater startup.
pub type ChangelogHttpFactory =
    dyn Fn(Arc<dyn SelfProxyPortSource>) -> Arc<dyn ChangelogHttp> + Send + Sync;

/// Starts the host HTTP owner after profiles and before logs, returning its lifecycle control.
pub type DebugHttpFactory = dyn Fn() -> BoxFuture<'static, Result<Arc<dyn DebugHttpControl>, super::debug_http::Error>>
    + Send
    + Sync;

/// Host-built capabilities and factories consumed during facade assembly.
pub struct ClientHostAdapters {
    /// Captures content on demand; script construction and execution must use blocking threads.
    pub runtime_build: Arc<dyn RuntimeBuildPort>,
    /// Supplies matching filesystem, download and materialization ports for the profile store.
    pub profile_files: Arc<ProfileFileFactory>,
    /// Supplies release-note HTTP access bound to the session proxy source.
    pub changelog_http: Arc<ChangelogHttpFactory>,
    /// Supplies HTTP lifecycle control whose shutdown must drain the owner before returning.
    pub debug_http: Arc<DebugHttpFactory>,
}
