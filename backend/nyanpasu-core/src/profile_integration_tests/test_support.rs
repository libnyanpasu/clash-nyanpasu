//! Real profile file capabilities for application integration tests.
use std::sync::Arc;

use nyanpasu_contracts::{
    net::{SelfProxyPortSource, SystemProxySource},
    profiles::ports::SubscriptionFetcher,
};
use nyanpasu_net::{device_headers::DeviceHeaders, subscription_http::SubscriptionHttpFetcher};
use nyanpasu_paths::PathResolver;
use nyanpasu_profile_io::fs_service::ProfileFileService;
use tempfile::TempDir;

use crate::client::{
    ClientSetupArgs, NyanpasuClient,
    host_adapters::{ClientHostAdapters, ProfileFileFactory, ProfileFilePorts},
    test_support,
    test_support::runtime_build::TestRuntimeBuildPort,
};

pub(super) struct NoSystemProxy;

impl SystemProxySource for NoSystemProxy {
    fn system_proxy(&self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

fn test_profile_file_ports(
    paths: &PathResolver,
    ports: Arc<dyn SelfProxyPortSource>,
) -> ProfileFilePorts {
    let service = Arc::new(ProfileFileService::new(paths.clone()));
    let fetcher = Arc::new(SubscriptionHttpFetcher::new(
        ports,
        nyanpasu_contracts::bundle_metadata::TEST_BUNDLE_METADATA,
        Arc::new(NoSystemProxy),
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
pub(super) fn test_host_adapters(paths: &PathResolver) -> ClientHostAdapters {
    test_support::test_adapters(
        test_profile_file_factory(paths),
        Arc::new(TestRuntimeBuildPort::from_directory(
            paths.app_profiles_dir().into_std_path_buf(),
            crate::client::RuntimePaths::from_resolver(paths),
        )),
    )
}
pub(super) async fn test_client_args_with_endpoint(
    dir: &TempDir,
    endpoint: crate::actor_v2::endpoint::EndpointHandle,
) -> ClientSetupArgs {
    let paths = crate::utils::test_support::test_paths(dir.path(), dir.path().join("data"));
    test_support::test_client_args_with_endpoint(dir, endpoint, test_host_adapters(&paths)).await
}
pub(super) async fn test_client_with_fetcher(
    dir: &TempDir,
    fetcher: Arc<dyn SubscriptionFetcher>,
) -> NyanpasuClient {
    let paths = crate::utils::test_support::test_paths(dir.path(), dir.path().join("data"));
    let mut adapters = test_host_adapters(&paths);
    adapters.profile_files = Arc::new(move |ports| ProfileFilePorts {
        fetcher: fetcher.clone(),
        ..test_profile_file_ports(&paths, ports)
    });
    test_support::facade::test_client_with_profile_ports(dir, adapters).await
}
