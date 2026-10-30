//! Host-provided adapters consumed while assembling the application facade.

use std::sync::Arc;

use futures::future::BoxFuture;
use nyanpasu_platform::enhance::ScriptDirs;

use super::{app_update::changelog::ChangelogHttp, debug_http::DebugHttpControl};
use crate::{
    service::profile_file::ports::SelfProxyPortSource,
    state::profiles::ports::{ProfileFsPort, ProfileMaterializationPort, SubscriptionFetcher},
};

pub struct ProfileFilePorts {
    pub fs: Arc<dyn ProfileFsPort>,
    pub fetcher: Arc<dyn SubscriptionFetcher>,
    pub materialization: Arc<dyn ProfileMaterializationPort>,
}

pub type ProfileFileFactory =
    dyn Fn(Arc<dyn SelfProxyPortSource>) -> ProfileFilePorts + Send + Sync;

pub type ChangelogHttpFactory =
    dyn Fn(Arc<dyn SelfProxyPortSource>) -> Arc<dyn ChangelogHttp> + Send + Sync;

pub type DebugHttpFactory = dyn Fn() -> BoxFuture<'static, Result<Arc<dyn DebugHttpControl>, super::debug_http::Error>>
    + Send
    + Sync;

pub struct ClientHostAdapters {
    pub script_dirs: ScriptDirs,
    pub profile_files: Arc<ProfileFileFactory>,
    pub changelog_http: Arc<ChangelogHttpFactory>,
    pub debug_http: Arc<DebugHttpFactory>,
}
