//! Real profile file capabilities for host integration tests.
use crate::{
    client::{
        ClientSetupArgs, NyanpasuClient,
        host_adapters::{ClientHostAdapters, ProfileFileFactory, ProfileFilePorts},
        test_support::{self, runtime_build::TestRuntimeBuildPort},
    },
    service::profile_file::{fs_service::ProfileFileService, ports::SelfProxyPortSource},
    state::profiles::ports::SubscriptionFetcher,
};
use nyanpasu_paths::PathResolver;
use std::sync::Arc;
use tempfile::TempDir;

pub(crate) fn test_profile_file_ports(
    paths: &PathResolver,
    ports: Arc<dyn SelfProxyPortSource>,
) -> ProfileFilePorts {
    let service = Arc::new(ProfileFileService::new(
        paths.clone(),
        ports,
        nyanpasu_core::client::bundle_metadata::TEST_BUNDLE_METADATA,
    ));
    ProfileFilePorts {
        fs: service.clone(),
        fetcher: service.clone(),
        materialization: service,
    }
}
pub(crate) fn test_profile_file_factory(paths: &PathResolver) -> Arc<ProfileFileFactory> {
    let paths = paths.clone();
    Arc::new(move |ports| test_profile_file_ports(&paths, ports))
}
pub(crate) fn test_host_adapters(paths: &PathResolver) -> ClientHostAdapters {
    test_support::test_adapters(
        test_profile_file_factory(paths),
        Arc::new(TestRuntimeBuildPort::from_directory(
            paths.app_profiles_dir().into_std_path_buf(),
            crate::client::RuntimePaths::from_resolver(paths),
        )),
    )
}
pub(crate) async fn test_client_args_with_endpoint(
    dir: &TempDir,
    endpoint: nyanpasu_core::actor_v2::endpoint::EndpointHandle,
) -> ClientSetupArgs {
    let paths = nyanpasu_core::utils::test_support::test_paths(dir.path(), dir.path().join("data"));
    test_support::test_client_args_with_endpoint(dir, endpoint, test_host_adapters(&paths)).await
}
pub(crate) async fn test_client_with_fetcher(
    dir: &TempDir,
    fetcher: Arc<dyn SubscriptionFetcher>,
) -> NyanpasuClient {
    let paths = nyanpasu_core::utils::test_support::test_paths(dir.path(), dir.path().join("data"));
    let mut adapters = test_host_adapters(&paths);
    adapters.profile_files = Arc::new(move |ports| ProfileFilePorts {
        fetcher: fetcher.clone(),
        ..test_profile_file_ports(&paths, ports)
    });
    test_support::facade::test_client_with_profile_ports(dir, adapters).await
}
