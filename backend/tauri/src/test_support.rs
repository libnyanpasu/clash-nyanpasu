//! Real profile file capabilities for host integration tests.
use std::sync::Arc;

use nyanpasu_contracts::net::SelfProxyPortSource;
use nyanpasu_core::client::{
    ClientSetupArgs,
    host_adapters::{ClientHostAdapters, ProfileFileFactory, ProfileFilePorts},
    test_support,
    test_support::runtime_build::TestRuntimeBuildPort,
};
use nyanpasu_net::{device_headers::DeviceHeaders, subscription_http::SubscriptionHttpFetcher};
use nyanpasu_paths::PathResolver;
use nyanpasu_profile_io::fs_service::ProfileFileService;
use tempfile::TempDir;

fn test_profile_file_ports(
    paths: &PathResolver,
    ports: Arc<dyn SelfProxyPortSource>,
) -> ProfileFilePorts {
    let service = Arc::new(ProfileFileService::new(paths.clone()));
    let fetcher = Arc::new(SubscriptionHttpFetcher::new(
        ports,
        nyanpasu_contracts::bundle_metadata::TEST_BUNDLE_METADATA,
        Arc::new(nyanpasu_os::system_proxy_source::OsSystemProxySource),
        DeviceHeaders {
            hwid: "test-device".into(),
            ..Default::default()
        },
    ));
    ProfileFilePorts {
        fs: service.clone(),
        fetcher,
        materialization: service,
    }
}
fn test_profile_file_factory(paths: &PathResolver) -> Arc<ProfileFileFactory> {
    let paths = paths.clone();
    Arc::new(move |ports| test_profile_file_ports(&paths, ports))
}
fn test_host_adapters(paths: &PathResolver) -> ClientHostAdapters {
    test_support::test_adapters(
        test_profile_file_factory(paths),
        Arc::new(TestRuntimeBuildPort::from_directory(
            paths.app_profiles_dir().into_std_path_buf(),
            nyanpasu_core::client::RuntimePaths::from_resolver(paths),
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
