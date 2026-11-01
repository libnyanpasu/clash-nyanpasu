//! Facade assembly from explicitly supplied profile capabilities.

use super::{
    temp_config_path, test_backup_deps, test_materialization_port, test_typed_config_clients,
    unused_test_adapters,
};
#[cfg(unix)]
use crate::state::profiles::ports::ProfileMaterializationPort;
#[cfg(test)]
use crate::state::profiles::ports::SubscriptionFetcher;
use crate::{
    actor_v2::test_support::test_v2_clients,
    client::{
        NoopSystemDnsCache, NyanpasuClient, RuntimePaths, SessionPortResolver, SystemDnsCache,
        application::ApplicationClient,
        bundle_metadata::BundleMetadata,
        clash_config::ClashConfigClient,
        core_lifecycle,
        direct_egress::DirectEgressProbe,
        effects,
        host_adapters::ClientHostAdapters,
        hotkey, logs, profiles,
        session_state::SessionStateClient,
        system_proxy::ports::{MockOsProxyPort, OsProxyPort},
    },
    state::profiles::{
        NewProfileRequest,
        ports::{MockProfileFsPort, MockSubscriptionFetcher},
    },
    utils::test_support::test_paths,
};
use nyanpasu_config::{
    application::ReleaseChannel,
    profile::{
        ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
        ProfileDefinition, ProfileMetadata, ProfileSource,
    },
};
use std::{path::PathBuf, sync::Arc};
use tempfile::TempDir;
pub async fn test_client(dir: &TempDir) -> NyanpasuClient {
    test_client_with_system_dns(dir, Arc::new(NoopSystemDnsCache)).await
}

pub(crate) async fn test_client_with_system_dns(
    dir: &TempDir,
    system_dns: Arc<dyn SystemDnsCache>,
) -> NyanpasuClient {
    test_client_with_ports(dir, system_dns, Arc::new(MockOsProxyPort::new())).await
}

pub(crate) async fn test_client_with_ports(
    dir: &TempDir,
    system_dns: Arc<dyn SystemDnsCache>,
    os_proxy: Arc<dyn OsProxyPort>,
) -> NyanpasuClient {
    let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
    test_client_from_typed_clients(
        dir,
        application,
        session_state,
        clash_config,
        system_dns,
        Arc::new(crate::client::direct_egress::MockDirectEgressProbe::new()),
        Arc::new(crate::geo::NoopCountryIndexSource),
        os_proxy,
    )
    .await
}

pub(crate) async fn test_client_from_typed_clients(
    dir: &TempDir,
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
    system_dns: Arc<dyn SystemDnsCache>,
    direct_egress: Arc<dyn DirectEgressProbe>,
    geo_index: Arc<dyn crate::geo::CountryIndexSource>,
    os_proxy: Arc<dyn OsProxyPort>,
) -> NyanpasuClient {
    let profiles = profiles::ProfilesClient::new(
        crate::state::mutation::MutationCoordinator::isolated(),
        temp_config_path(dir, "profiles.yaml"),
        Arc::new(MockProfileFsPort::new()),
        Arc::new(MockSubscriptionFetcher::new()),
        test_materialization_port(),
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .expect("profiles client should be created");
    let ports = Arc::new(SessionPortResolver::default());
    let (core_v2, service) = test_v2_clients().await;
    let (backup_paths, storage) = test_backup_deps(&dir);
    NyanpasuClient::with_parts(
        None,
        BundleMetadata {
            is_portable: false,
            is_fixed_webview: false,
            release_channel: ReleaseChannel::Stable,
            sourceforge_project: None,
            ..crate::client::bundle_metadata::TEST_BUNDLE_METADATA
        },
        logs::test_setup(
            test_paths(dir.path(), dir.path().join("data"))
                .app_logs_dir()
                .into_std_path_buf(),
        ),
        profiles.jobs(),
        application,
        session_state,
        clash_config,
        profiles,
        Arc::new(MockProfileFsPort::new()),
        ports,
        dir.path().join("profiles"),
        PathBuf::new(),
        backup_paths,
        storage,
        RuntimePaths::from_resolver(&test_paths(dir.path(), dir.path().join("data"))),
        ClientHostAdapters {
            runtime_build: Arc::new(
                crate::client::test_support::runtime_build::TestRuntimeBuildPort::from_directory(
                    dir.path().join("profiles"),
                    RuntimePaths::from_resolver(&test_paths(dir.path(), dir.path().join("data"))),
                ),
            ),
            ..unused_test_adapters(&test_paths(dir.path(), dir.path().join("data")))
        },
        Arc::new(crate::client::event_sink::NoopUiEventSink),
        None,
        None,
        core_v2,
        service,
        system_dns,
        direct_egress,
        geo_index,
        os_proxy,
        Arc::new(core_lifecycle::adapters::FsBinaryInstaller),
        Arc::new(crate::client::core_version::MockCoreVersionReader::new()),
        Arc::new(effects::ports::NoopApplicationEffects),
        Arc::new(hotkey::ports::MockWindowControl::new()),
        Arc::new(crate::client::test_support::AnyAccelerator),
        None,
        tokio_util::sync::CancellationToken::new(),
        tokio_util::task::TaskTracker::new(),
        crate::client::session_end::SessionEndSource::new().subscribe(),
    )
    .await
    .unwrap()
}

#[cfg(test)]
pub(crate) async fn test_client_with_direct_egress(
    dir: &TempDir,
    direct_egress: Arc<dyn DirectEgressProbe>,
) -> NyanpasuClient {
    let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
    test_client_from_typed_clients(
        dir,
        application,
        session_state,
        clash_config,
        Arc::new(NoopSystemDnsCache),
        direct_egress,
        Arc::new(crate::geo::NoopCountryIndexSource),
        Arc::new(MockOsProxyPort::new()),
    )
    .await
}

/// Assembles a facade with caller-provided profile ports and runtime build capability.
pub async fn test_client_with_profile_ports(
    dir: &TempDir,
    host_adapters: ClientHostAdapters,
) -> NyanpasuClient {
    let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
    let paths = test_paths(dir.path(), dir.path().join("data"));
    let ports = Arc::new(SessionPortResolver::default());
    let file_ports = (host_adapters.profile_files)(ports.clone());
    let profiles = profiles::ProfilesClient::new(
        crate::state::mutation::MutationCoordinator::isolated(),
        temp_config_path(dir, "profiles.yaml"),
        file_ports.fs.clone(),
        file_ports.fetcher,
        file_ports.materialization,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .expect("profiles client should be created");
    let (core_v2, service) = test_v2_clients().await;
    let (backup_paths, storage) = test_backup_deps(&dir);
    let client = NyanpasuClient::with_parts(
        None,
        BundleMetadata {
            is_portable: false,
            is_fixed_webview: false,
            release_channel: ReleaseChannel::Stable,
            sourceforge_project: None,
            ..crate::client::bundle_metadata::TEST_BUNDLE_METADATA
        },
        logs::test_setup(
            test_paths(dir.path(), dir.path().join("data"))
                .app_logs_dir()
                .into_std_path_buf(),
        ),
        profiles.jobs(),
        application,
        session_state,
        clash_config,
        profiles,
        file_ports.fs.clone(),
        ports,
        paths.app_profiles_dir().into_std_path_buf(),
        PathBuf::new(),
        backup_paths,
        storage,
        RuntimePaths::from_resolver(&paths),
        host_adapters,
        Arc::new(crate::client::event_sink::NoopUiEventSink),
        None,
        None,
        core_v2,
        service,
        Arc::new(NoopSystemDnsCache),
        Arc::new(crate::client::direct_egress::MockDirectEgressProbe::new()),
        Arc::new(crate::geo::NoopCountryIndexSource),
        Arc::new(MockOsProxyPort::new()),
        Arc::new(core_lifecycle::adapters::FsBinaryInstaller),
        Arc::new(crate::client::core_version::MockCoreVersionReader::new()),
        Arc::new(effects::ports::NoopApplicationEffects),
        Arc::new(hotkey::ports::MockWindowControl::new()),
        Arc::new(crate::client::test_support::AnyAccelerator),
        None,
        tokio_util::sync::CancellationToken::new(),
        tokio_util::task::TaskTracker::new(),
        crate::client::session_end::SessionEndSource::new().subscribe(),
    )
    .await
    .unwrap();
    client
}

pub fn local_config_request(name: &str) -> NewProfileRequest {
    NewProfileRequest {
        metadata: ProfileMetadata {
            name: name.into(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new("pending.yaml").unwrap(),
                            updated_at: None,
                        },
                    },
                },
                transforms: vec![],
            }),
        },
        folder: None,
    }
}

#[cfg(test)]
pub(crate) async fn test_client_with_fetcher(
    dir: &TempDir,
    fetcher: Arc<dyn SubscriptionFetcher>,
) -> NyanpasuClient {
    let paths = test_paths(dir.path(), dir.path().join("data"));
    let file_ports = super::profile_files::scripted_profile_file_ports(fetcher);
    let runtime_build = super::runtime_build::TestRuntimeBuildPort::from_profile_port(
        file_ports.fs.clone(),
        RuntimePaths::from_resolver(&paths),
    );
    let adapters = super::test_adapters(
        super::profile_files::fixed_profile_file_factory(file_ports),
        Arc::new(runtime_build),
    );
    test_client_with_profile_ports(dir, adapters).await
}

#[cfg(unix)]
pub async fn test_client_with_materialization(
    dir: &TempDir,
    materialization: Arc<dyn ProfileMaterializationPort>,
) -> NyanpasuClient {
    let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
    let profiles = profiles::ProfilesClient::new(
        crate::state::mutation::MutationCoordinator::isolated(),
        temp_config_path(dir, "profiles.yaml"),
        Arc::new(MockProfileFsPort::new()),
        Arc::new(MockSubscriptionFetcher::new()),
        materialization,
        tokio_util::sync::CancellationToken::new(),
        &tokio_util::task::TaskTracker::new(),
    )
    .await
    .expect("profiles client");
    let ports = Arc::new(SessionPortResolver::default());
    let (core_v2, service) = test_v2_clients().await;
    let (backup_paths, storage) = test_backup_deps(dir);
    NyanpasuClient::with_parts(
        None,
        BundleMetadata {
            is_portable: false,
            is_fixed_webview: false,
            release_channel: ReleaseChannel::Stable,
            sourceforge_project: None,
            ..crate::client::bundle_metadata::TEST_BUNDLE_METADATA
        },
        logs::test_setup(
            test_paths(dir.path(), dir.path().join("data"))
                .app_logs_dir()
                .into_std_path_buf(),
        ),
        profiles.jobs(),
        application,
        session_state,
        clash_config,
        profiles,
        Arc::new(MockProfileFsPort::new()),
        ports,
        dir.path().join("profiles"),
        PathBuf::new(),
        backup_paths,
        storage,
        RuntimePaths::from_resolver(&test_paths(dir.path(), dir.path().join("data"))),
        ClientHostAdapters {
            runtime_build: Arc::new(
                crate::client::test_support::runtime_build::TestRuntimeBuildPort::from_directory(
                    dir.path().join("profiles"),
                    RuntimePaths::from_resolver(&test_paths(dir.path(), dir.path().join("data"))),
                ),
            ),
            ..unused_test_adapters(&test_paths(dir.path(), dir.path().join("data")))
        },
        Arc::new(crate::client::event_sink::NoopUiEventSink),
        None,
        None,
        core_v2,
        service,
        Arc::new(NoopSystemDnsCache),
        Arc::new(crate::client::direct_egress::MockDirectEgressProbe::new()),
        Arc::new(crate::geo::NoopCountryIndexSource),
        Arc::new(MockOsProxyPort::new()),
        Arc::new(core_lifecycle::adapters::FsBinaryInstaller),
        Arc::new(crate::client::core_version::MockCoreVersionReader::new()),
        Arc::new(effects::ports::NoopApplicationEffects),
        Arc::new(hotkey::ports::MockWindowControl::new()),
        Arc::new(crate::client::test_support::AnyAccelerator),
        None,
        tokio_util::sync::CancellationToken::new(),
        tokio_util::task::TaskTracker::new(),
        crate::client::session_end::SessionEndSource::new().subscribe(),
    )
    .await
    .unwrap()
}
