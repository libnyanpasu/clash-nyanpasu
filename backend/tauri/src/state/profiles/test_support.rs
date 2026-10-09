use std::path::PathBuf;

use nyanpasu_config::profile::{
    ExternalProfilePath, ManagedProfilePath, Profiles, RemoteProfileOptions,
};
use nyanpasu_core::profiles::{
    error::{ProfileFileError, SubscriptionFetchError},
    ports::{
        CleanupOutcome, FetchedSubscription, MaterializationReconcileReport,
        MaterializationResource, PreparedCleanup, PreparedMaterialization, ProfileFsPort,
        ProfileMaterializationPort, SubscriptionFetcher,
    },
};
use url::Url;

mockall::mock! {
    pub(crate) ProfileFsPort {}

    impl ProfileFsPort for ProfileFsPort {
        fn read(&self, path: &ManagedProfilePath) -> Result<String, ProfileFileError>;
        fn write_atomic(
            &self,
            path: &ManagedProfilePath,
            content: &str,
        ) -> Result<(), ProfileFileError>;
        fn remove(&self, path: &ManagedProfilePath) -> Result<(), ProfileFileError>;
        fn read_external(&self, target: &ExternalProfilePath) -> Result<String, ProfileFileError>;
        fn ensure_not_symlink(&self, path: &ManagedProfilePath) -> Result<(), ProfileFileError>;
        fn ensure_symlink(
            &self,
            path: &ManagedProfilePath,
            target: &ExternalProfilePath,
        ) -> Result<(), ProfileFileError>;
    }
}

mockall::mock! {
    pub(crate) SubscriptionFetcher {}

    #[async_trait::async_trait]
    impl SubscriptionFetcher for SubscriptionFetcher {
        async fn fetch(
            &self,
            url: &Url,
            options: &RemoteProfileOptions,
        ) -> Result<FetchedSubscription, SubscriptionFetchError>;
    }
}

mockall::mock! {
    pub(crate) ProfileMaterializationPort {}

    impl ProfileMaterializationPort for ProfileMaterializationPort {
        fn prepare_state_first(
            &self,
            path: &ManagedProfilePath,
            resource: MaterializationResource,
            expected_revision: u64,
        ) -> Result<PreparedMaterialization, ProfileFileError>;
        fn prepare_file_first(
            &self,
            path: &ManagedProfilePath,
            resource: MaterializationResource,
            expected_revision: u64,
        ) -> Result<PreparedMaterialization, ProfileFileError>;
        fn promote(&self, prepared: &PreparedMaterialization) -> Result<(), ProfileFileError>;
        fn complete(&self, prepared: &PreparedMaterialization) -> Result<(), ProfileFileError>;
        fn compensate(&self, prepared: &PreparedMaterialization) -> Result<(), ProfileFileError>;
        fn prepare_cleanup(
            &self,
            path: &ManagedProfilePath,
            expected_revision: u64,
        ) -> Result<PreparedCleanup, ProfileFileError>;
        fn activate_cleanup(&self, cleanup: &PreparedCleanup) -> Result<(), ProfileFileError>;
        fn cancel_cleanup(&self, cleanup: &PreparedCleanup) -> Result<(), ProfileFileError>;
        fn retry_cleanup(
            &self,
            cleanup: &PreparedCleanup,
            profiles: &Profiles,
        ) -> Result<CleanupOutcome, ProfileFileError>;
        fn reconcile(
            &self,
            profiles: &Profiles,
        ) -> Result<MaterializationReconcileReport, ProfileFileError>;
    }
}

/// A stand-in for whatever a mocked port fails with.
pub(crate) fn mock_profile_file_error(reason: &'static str) -> ProfileFileError {
    ProfileFileError::WriteFile {
        path: PathBuf::from("mock").into(),
        source: std::io::Error::other(reason),
    }
}

/// A stand-in for whatever a mocked fetcher fails with.
pub(crate) fn mock_subscription_fetch_error() -> SubscriptionFetchError {
    SubscriptionFetchError::SubscriptionHttpStatus { status: 500 }
}
