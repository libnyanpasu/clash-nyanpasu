//! Consumer-owned ports for the profiles actor (design §7, D10). Concrete
//! implementations live in `crate::service::profile_file`.

use nyanpasu_config::profile::{
    ExternalProfilePath, ManagedProfilePath, Profiles, RemoteProfileOptions, SubscriptionInfo,
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
    /// Server-advertised `profile-update-interval`, normalized from hours to minutes.
    pub suggested_update_interval_minutes: Option<u64>,
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

#[derive(Debug, Clone)]
pub(crate) enum MaterializationResource {
    File { content: String },
    Symlink { target: ExternalProfilePath },
}

/// Opaque durable transaction handle. Phase and storage paths stay adapter-owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedMaterialization {
    operation_id: String,
}

impl PreparedMaterialization {
    pub(crate) fn new(operation_id: String) -> Self {
        Self { operation_id }
    }

    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }
}

/// Opaque durable cleanup handle. Phase and storage paths stay adapter-owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedCleanup {
    operation_id: String,
}

impl PreparedCleanup {
    pub(crate) fn new(operation_id: String) -> Self {
        Self { operation_id }
    }

    pub(crate) fn operation_id(&self) -> &str {
        &self.operation_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CleanupOutcome {
    Removed,
    AlreadyAbsent,
    FencedActivePath,
    FencedHashMismatch,
}

/// Internal degradation details. S08 owns any public IPC representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileDegradation {
    pub phase: ProfileDegradationPhase,
    pub code: ProfileDegradationCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProfileDegradationPhase {
    Cleanup,
    Reconcile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProfileDegradationCode {
    JournalInvalid,
    MaterializationDeferred,
    CleanupDeferred,
}

impl ProfileDegradationCode {
    /// Retryability is derived from the code, not stored per instance.
    pub(crate) const fn retryable(self) -> bool {
        match self {
            Self::JournalInvalid => false,
            Self::MaterializationDeferred | Self::CleanupDeferred => true,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct MaterializationReconcileReport {
    pub discarded: usize,
    pub promoted: usize,
    pub completed: usize,
    pub compensated: usize,
    pub cleanups_completed: usize,
    pub cleanups_fenced: usize,
    pub degradations: Vec<ProfileDegradation>,
}

/// Crate-internal transactional materialization and durable cleanup. Blocking
/// callers must invoke this port through `spawn_blocking` from the actor layer.
///
/// Preparation receives the durable `Profiles::revision` that will be committed
/// with the operation, never PersistentStateManager's reset-on-restart MVCC
/// version. Task #26 must bump `Profiles::revision` on every successful forward
/// or compensating state commit before preparing the operation.
///
/// Protocols are:
/// - state-first: prepare(next) -> state CAS -> promote -> complete/compensate;
/// - file-first: prepare(next) -> promote -> state CAS -> complete/compensate;
/// - cleanup: prepare(next) -> state CAS -> activate -> retry;
/// - recovery: reconcile(loaded profiles) before watchers or mutations.
#[cfg_attr(test, mockall::automock)]
pub(crate) trait ProfileMaterializationPort: Send + Sync + 'static {
    fn prepare_state_first(
        &self,
        path: &ManagedProfilePath,
        resource: MaterializationResource,
        expected_revision: u64,
    ) -> anyhow::Result<PreparedMaterialization>;

    fn prepare_file_first(
        &self,
        path: &ManagedProfilePath,
        resource: MaterializationResource,
        expected_revision: u64,
    ) -> anyhow::Result<PreparedMaterialization>;

    fn promote(&self, prepared: &PreparedMaterialization) -> anyhow::Result<()>;

    fn complete(&self, prepared: &PreparedMaterialization) -> anyhow::Result<()>;

    fn compensate(&self, prepared: &PreparedMaterialization) -> anyhow::Result<()>;

    fn prepare_cleanup(
        &self,
        path: &ManagedProfilePath,
        expected_revision: u64,
    ) -> anyhow::Result<PreparedCleanup>;

    fn activate_cleanup(&self, cleanup: &PreparedCleanup) -> anyhow::Result<()>;

    fn cancel_cleanup(&self, cleanup: &PreparedCleanup) -> anyhow::Result<()>;

    fn retry_cleanup(
        &self,
        cleanup: &PreparedCleanup,
        profiles: &Profiles,
    ) -> anyhow::Result<CleanupOutcome>;

    fn reconcile(&self, profiles: &Profiles) -> anyhow::Result<MaterializationReconcileReport>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_handles_expose_only_crate_internal_operation_ids() {
        let materialization = PreparedMaterialization::new("materialize".into());
        let cleanup = PreparedCleanup::new("cleanup".into());

        assert_eq!(materialization.operation_id(), "materialize");
        assert_eq!(cleanup.operation_id(), "cleanup");
    }

    #[test]
    fn degradation_retryability_is_derived_from_code() {
        assert!(!ProfileDegradationCode::JournalInvalid.retryable());
        assert!(ProfileDegradationCode::MaterializationDeferred.retryable());
        assert!(ProfileDegradationCode::CleanupDeferred.retryable());
    }
}
