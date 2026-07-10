//! Consumer-owned ports for the profiles actor (design §7, D10). Concrete
//! implementations live in `crate::service::profile_file`.

use nyanpasu_config::profile::{
    ExternalProfilePath, ManagedProfilePath, RemoteProfileOptions, SubscriptionInfo,
};
use url::Url;

/// Filesystem access for materialized profile files. Paths are relative to the
/// app profiles dir; resolution is the implementation's concern.
#[cfg_attr(test, mockall::automock)]
pub trait ProfileFsPort: Send + Sync + 'static {
    fn read(&self, path: &ManagedProfilePath) -> anyhow::Result<String>;
    fn write_atomic(&self, path: &ManagedProfilePath, content: &str) -> anyhow::Result<()>;
    /// Idempotent: removing a missing file succeeds.
    fn remove(&self, path: &ManagedProfilePath) -> anyhow::Result<()>;
    /// Read an External binding target for Mirror synchronization.
    fn read_external(&self, target: &ExternalProfilePath) -> anyhow::Result<String>;
    /// Remote-updater write guard: the target must not be an unexpected
    /// symlink (clean-design §9 last paragraph).
    fn ensure_not_symlink(&self, path: &ManagedProfilePath) -> anyhow::Result<()>;
    /// Create or repair `path -> target` (External Symlink binding, clean-design §10.1).
    fn ensure_symlink(
        &self,
        path: &ManagedProfilePath,
        target: &ExternalProfilePath,
    ) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub struct FetchedSubscription {
    pub content: String,
    /// Server-provided display name (`profile-title` / `Content-Disposition`).
    pub filename: Option<String>,
    pub subscription: SubscriptionInfo,
}

/// Subscription download. Network timeouts are managed inside the
/// implementation (D9); content validation is the caller's concern (per
/// target profile kind, design fig. 13.3).
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait SubscriptionFetcher: Send + Sync + 'static {
    async fn fetch(
        &self,
        url: &Url,
        options: &RemoteProfileOptions,
    ) -> anyhow::Result<FetchedSubscription>;
}

/// Background-commit rebuild signal (design §6.4). Fire-and-forget; debouncing
/// is the receiver's concern.
#[cfg_attr(test, mockall::automock)]
pub trait RebuildNotifier: Send + Sync + 'static {
    fn request_rebuild(&self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_are_mockable_and_object_safe() {
        let _fs: Box<dyn ProfileFsPort> = Box::new(MockProfileFsPort::new());
        let _fetcher: Box<dyn SubscriptionFetcher> = Box::new(MockSubscriptionFetcher::new());
        let _notifier: Box<dyn RebuildNotifier> = Box::new(MockRebuildNotifier::new());
    }
}
